#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The vault mirror. Doc 16 sections 3.1 and 7 point 2.
//!
//! Two copies of one page and either can move. The decision table is the whole
//! risk in this step, so it is tested as a table, and the end to end half runs
//! over a real folder because a mirror that never touches a disk has not been
//! tested.

use std::collections::BTreeSet;

use serde_json::json;
use tessera_core::vault::{self, Action, VaultFile};
use tessera_store::blob::BlobStore;
use tessera_store::repo::{self, NewPage, PageRow};
use tessera_store::{Store, new_id, now_iso8601};

/// A row as the mirror sees it: what it says, and what it last agreed with the
/// file it mirrors.
fn row(id: &str, title: &str, path: &str, body: &str, agreed: &str) -> PageRow {
    PageRow {
        id: id.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        file_path: path.to_string(),
        source_card_id: None,
        citations_carried: json!([]),
        synced_hash: BlobStore::hash(agreed.as_bytes()),
        created_at: now_iso8601(),
        updated_at: now_iso8601(),
    }
}

fn file(path: &str, body: &str) -> VaultFile {
    VaultFile {
        path: path.to_string(),
        body: body.to_string(),
    }
}

// ------------------------------------------------------- the decision table

#[test]
fn neither_moved_so_the_agreement_is_recorded_and_nothing_is_written() {
    let rows = [row("p1", "A", "vault/a.md", "same", "same")];
    let files = [file("vault/a.md", "same")];
    assert_eq!(
        vault::plan(&rows, &files),
        vec![Action::Agreed {
            page_id: "p1".into(),
            body: "same".into()
        }]
    );
}

#[test]
fn the_row_moved_so_the_file_is_written() {
    let rows = [row("p1", "A", "vault/a.md", "new", "old")];
    let files = [file("vault/a.md", "old")];
    assert_eq!(
        vault::plan(&rows, &files),
        vec![Action::WriteFile {
            page_id: "p1".into(),
            path: "vault/a.md".into(),
            body: "new".into(),
            expect: Some("old".into()),
        }]
    );
}

#[test]
fn the_file_moved_so_the_row_takes_its_text() {
    // Doc 16 section 3.1: the vault is the person's even without the app
    // running, so an edit made in a text editor is an edit.
    let rows = [row("p1", "A", "vault/a.md", "old", "old")];
    let files = [file("vault/a.md", "typed in an editor")];
    assert_eq!(
        vault::plan(&rows, &files),
        vec![Action::AdoptBody {
            page_id: "p1".into(),
            path: "vault/a.md".into(),
            body: "typed in an editor".into()
        }]
    );
}

#[test]
fn both_moved_so_the_file_wins_the_page_and_the_row_gets_a_conflict_copy() {
    // Doc 16 section 7 point 2: last write wins with a conflict copy. Without
    // an mtime to trust there is no "last", so the rule becomes: the file wins
    // the page, because a person who edited it outside the app meant to, and
    // the row's text is kept beside it rather than dropped.
    let rows = [row(
        "p1",
        "Liquidity risk",
        "vault/liquidity-risk.md",
        "in the app",
        "before",
    )];
    let files = [file("vault/liquidity-risk.md", "in the editor")];
    assert_eq!(
        vault::plan(&rows, &files),
        vec![Action::Conflict {
            page_id: "p1".into(),
            path: "vault/liquidity-risk.md".into(),
            body: "in the editor".into(),
            conflict_path: "vault/liquidity-risk (conflict).md".into(),
            conflict_title: "Liquidity risk (conflict)".into(),
            conflict_body: "in the app".into(),
        }]
    );
}

#[test]
fn a_missing_file_is_rewritten_rather_than_taken_as_a_deletion() {
    // An unmounted folder, a sync tool mid pass and a deliberate deletion all
    // look identical from here, and only one of them wants the page gone.
    let rows = [row("p1", "A", "vault/a.md", "text", "text")];
    assert_eq!(
        vault::plan(&rows, &[]),
        vec![Action::WriteFile {
            page_id: "p1".into(),
            path: "vault/a.md".into(),
            body: "text".into(),
            expect: None,
        }]
    );
}

#[test]
fn a_file_nobody_claims_becomes_a_page() {
    let files = [file("vault/liquidity.md", "# Liquidity risk\n\nWritten by hand.")];
    assert_eq!(
        vault::plan(&[], &files),
        vec![Action::Adopt {
            path: "vault/liquidity.md".into(),
            title: "Liquidity risk".into(),
            body: "# Liquidity risk\n\nWritten by hand.".into()
        }],
        "the heading names the page; the file name is the fallback"
    );

    let unheaded = [file("vault/subfolder/some-notes.md", "no heading here")];
    assert_eq!(
        vault::plan(&[], &unheaded),
        vec![Action::Adopt {
            path: "vault/subfolder/some-notes.md".into(),
            title: "some-notes".into(),
            body: "no heading here".into()
        }],
        "a file in a subfolder is a page like any other"
    );
}

#[test]
fn a_file_whose_title_is_taken_is_named_rather_than_forced_in() {
    // Doc 16 section 3.1 lets one page have the title. Guessing which would be
    // a coin toss with the person's notes, so the mirror says which file it
    // left alone and why.
    let rows = [row(
        "p1",
        "Liquidity risk",
        "vault/liquidity-risk.md",
        "one",
        "one",
    )];
    let files = [
        file("vault/liquidity-risk.md", "one"),
        file("vault/other.md", "# LIQUIDITY RISK\n\ntwo"),
    ];
    let actions = vault::plan(&rows, &files);
    assert!(
        actions.contains(&Action::Skipped {
            path: "vault/other.md".into(),
            reason: vault::TITLE_TAKEN
        }),
        "{actions:?}"
    );
}

#[test]
fn two_unclaimed_files_with_one_title_between_them_leave_one_page() {
    // The same rule between two files rather than a file and a row. On a case
    // sensitive filesystem both of these exist; on a case insensitive one only
    // ever one does.
    let files = [file("vault/Buffer.md", "one"), file("vault/buffer.md", "two")];
    let actions = vault::plan(&[], &files);
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, Action::Adopt { .. }))
            .count(),
        1
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, Action::Skipped { .. }))
            .count(),
        1
    );
}

#[test]
fn a_plan_over_one_state_is_the_same_plan_twice() {
    // Sorted rows and a sorted listing, because a mirror that reorders its
    // decisions run to run is a mirror nobody can test.
    let rows = [
        row("p2", "B", "vault/b.md", "b", "b"),
        row("p1", "A", "vault/a.md", "a2", "a"),
    ];
    let files = [file("vault/b.md", "b"), file("vault/c.md", "# C")];
    assert_eq!(vault::plan(&rows, &files), vault::plan(&rows, &files));
}

// ------------------------------------------------------------------ slugs

#[test]
fn a_slug_is_a_file_name_no_platform_argues_with() {
    assert_eq!(vault::slug("Liquidity risk"), "liquidity-risk");
    assert_eq!(
        vault::slug("What is CAR3, article 12?"),
        "what-is-car3-article-12"
    );
    assert_eq!(vault::slug("  spaced  out  "), "spaced-out");
    assert_eq!(
        vault::slug("///"),
        "page",
        "a title of punctuation still needs a file"
    );
    // A vault of Dutch notes should not be a vault of empty names.
    assert_eq!(vault::slug("Vereiste eigen vermogen"), "vereiste-eigen-vermogen");
}

#[test]
fn two_titles_that_slug_alike_get_two_files() {
    let mut taken = BTreeSet::new();
    let first = vault::file_path("", "Liquidity risk", &taken);
    assert_eq!(first, "vault/liquidity-risk.md");
    taken.insert(first);

    let second = vault::file_path("", "Liquidity/risk", &taken);
    assert_eq!(
        second, "vault/liquidity-risk-2.md",
        "the page keeps its title, the file gets a number"
    );
}

#[test]
fn a_folder_makes_a_subpath_because_learning_records_need_one() {
    // Doc 17 section 5: `vault/learning/<mission>/<date>.md`.
    assert_eq!(
        vault::file_path("learning/a-mission", "2026-08-27", &BTreeSet::new()),
        "vault/learning/a-mission/2026-08-27.md"
    );
}

// --------------------------------------------------------------- end to end

struct Fixture {
    store: Store,
    profile: String,
    pack: String,
    root: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("tessera-vault-{}", new_id()));
    let store = Store::open(&root).expect("store");
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());
    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            rusqlite::params![pack, now],
        )
        .expect("pack");
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            rusqlite::params![profile, pack, now],
        )
        .expect("profile");
    Fixture {
        store,
        profile,
        pack,
        root,
    }
}

fn write_page(f: &mut Fixture, title: &str, body: &str) -> String {
    let path = vault::file_path("", title, &BTreeSet::new());
    repo::create_page(
        &mut f.store,
        NewPage {
            profile_id: &f.profile,
            title,
            body,
            file_path: &path,
            source_card_id: None,
            citations_carried: json!([]),
            doctrine_pack_id: Some(&f.pack),
        },
    )
    .expect("page")
}

fn sync(f: &mut Fixture) -> vault::SyncReport {
    let pack = f.pack.clone();
    let profile = f.profile.clone();
    vault::sync(&mut f.store, &profile, Some(&pack)).expect("sync")
}

#[test]
fn a_page_becomes_a_file_and_an_edited_file_becomes_the_page() {
    let mut f = fixture();
    let id = write_page(&mut f, "Liquidity risk", "The rule is in article 12.");

    let report = sync(&mut f);
    assert_eq!(report.written, 1, "{report:?}");
    let on_disk = f.root.join("vault/liquidity-risk.md");
    assert_eq!(
        std::fs::read_to_string(&on_disk).expect("read"),
        "The rule is in article 12."
    );

    // Nothing moved, so a second pass writes nothing and records the agreement.
    let again = sync(&mut f);
    assert_eq!((again.written, again.agreed), (0, 1), "{again:?}");

    // The person opens the file in their editor.
    std::fs::write(&on_disk, "The rule is in article 12, second paragraph.").expect("write");
    let after = sync(&mut f);
    assert_eq!(after.adopted, 1, "{after:?}");
    assert_eq!(
        repo::read_page(&f.store, &id).expect("read").expect("page").body,
        "The rule is in article 12, second paragraph."
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn an_edit_in_the_app_reaches_the_file() {
    let mut f = fixture();
    let id = write_page(&mut f, "Liquidity risk", "one");
    sync(&mut f);

    repo::edit_page(&mut f.store, &id, "one, corrected").expect("edit");
    let report = sync(&mut f);
    assert_eq!(report.written, 1, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(f.root.join("vault/liquidity-risk.md")).expect("read"),
        "one, corrected"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn two_edits_at_once_keep_both_texts() {
    // Doc 16 section 7 point 2. The failure this guards against is the quiet
    // one: a mirror that picks a winner and drops the loser looks identical to
    // a mirror that had nothing to decide.
    let mut f = fixture();
    let id = write_page(&mut f, "Liquidity risk", "original");
    sync(&mut f);

    repo::edit_page(&mut f.store, &id, "the app's version").expect("edit");
    std::fs::write(f.root.join("vault/liquidity-risk.md"), "the editor's version").expect("write");

    let report = sync(&mut f);
    assert_eq!(report.conflicts, 1, "{report:?}");

    // The page took the file's text, and the app's text is beside it.
    assert_eq!(
        repo::read_page(&f.store, &id).expect("read").expect("page").body,
        "the editor's version"
    );
    let copy = f.root.join("vault/liquidity-risk (conflict).md");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("the conflict copy"),
        "the app's version"
    );
    let saved = repo::page_by_title(&f.store, &f.profile, "Liquidity risk (conflict)")
        .expect("by title")
        .expect("the copy is a page of its own");
    assert_eq!(saved.body, "the app's version");

    // And the next pass has nothing left to argue about.
    let settled = sync(&mut f);
    assert_eq!((settled.conflicts, settled.written), (0, 0), "{settled:?}");

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_file_written_outside_the_app_becomes_a_page_with_its_heading_as_the_title() {
    let mut f = fixture();
    std::fs::create_dir_all(f.root.join("vault/reading")).expect("dir");
    std::fs::write(
        f.root.join("vault/reading/notes.md"),
        "# Basel III\n\nWhat I read on the train.",
    )
    .expect("write");

    let report = sync(&mut f);
    assert_eq!(report.created, 1, "{report:?}");
    let page = repo::page_by_title(&f.store, &f.profile, "Basel III")
        .expect("by title")
        .expect("the file became a page");
    assert_eq!(page.file_path, "vault/reading/notes.md", "a subfolder is kept");
    assert_eq!(page.body, "# Basel III\n\nWhat I read on the train.");

    // And it is not adopted twice.
    let again = sync(&mut f);
    assert_eq!((again.created, again.agreed), (0, 1), "{again:?}");

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_deleted_file_comes_back_rather_than_deleting_the_page() {
    let mut f = fixture();
    let id = write_page(&mut f, "Liquidity risk", "text");
    sync(&mut f);
    std::fs::remove_file(f.root.join("vault/liquidity-risk.md")).expect("remove");

    let report = sync(&mut f);
    assert_eq!(report.written, 1, "{report:?}");
    assert!(repo::read_page(&f.store, &id).expect("read").is_some());
    assert_eq!(
        std::fs::read_to_string(f.root.join("vault/liquidity-risk.md")).expect("read"),
        "text"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_vault_that_is_not_there_is_not_an_error() {
    // Most profiles have written no pages, and a sync at app start runs anyway.
    let mut f = fixture();
    let report = sync(&mut f);
    assert_eq!(report, vault::SyncReport::default());
    std::fs::remove_dir_all(&f.root).ok();
}
