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

// ----------------------------------------------------------- wikilinks

use tessera_core::wikilink;

#[test]
fn a_link_resolves_to_the_page_it_names_and_survives_its_rename() {
    // Doc 16 section 2.2's whole point: the assessed package resolved by title
    // string, so a rename silently broke every link into a page. These resolve
    // to the id.
    let mut f = fixture();
    let target = write_page(&mut f, "Liquidity risk", "The rule.");
    let from = write_page(&mut f, "Reading notes", "See [[Liquidity risk]] for the rule.");
    vault::save_links(
        &mut f.store,
        &f.profile.clone(),
        &from,
        "See [[Liquidity risk]] for the rule.",
    )
    .expect("links");

    let back = repo::backlinks(&f.store, "page", &target).expect("backlinks");
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].page_id, from);
    assert_eq!(back[0].page_title, "Reading notes");

    // The target is renamed. The body still says the old words, and the link
    // still arrives.
    repo::rename_page(
        &mut f.store,
        &target,
        "Liquidity coverage",
        "vault/liquidity-coverage.md",
    )
    .expect("rename");
    let after = repo::backlinks(&f.store, "page", &target).expect("backlinks");
    assert_eq!(after.len(), 1, "the rename broke the link into the page");

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_link_that_names_no_page_falls_to_the_concept_that_carries_the_term() {
    // Doc 16 section 3.1: "a wikilink whose title matches a Concept term or
    // alias links to the concept".
    let mut f = fixture();
    let concept = new_id();
    f.store
        .conn()
        .execute(
            "INSERT INTO concept (id, profile_id, term, aliases, doctrine_pack_id, status,
                 created_at, updated_at)
             VALUES (?1, ?2, 'Countercyclical buffer', '[\"CCyB\"]', ?3, 'confirmed', ?4, ?4)",
            rusqlite::params![concept, f.profile, f.pack, now_iso8601()],
        )
        .expect("concept");

    let body = "Both [[Countercyclical buffer]] and [[CCyB|the buffer]] point at the term.";
    let page = write_page(&mut f, "Reading notes", body);
    vault::save_links(&mut f.store, &f.profile.clone(), &page, body).expect("links");

    let back = repo::backlinks(&f.store, "concept", &concept).expect("backlinks");
    assert_eq!(back.len(), 2, "the alias is a way in as much as the term is");
    assert_eq!(back[1].display_text, "the buffer", "the body shows what it says");

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_link_to_nothing_waits_for_the_page_rather_than_being_dropped() {
    // Doc 16 section 3.1: unresolved links are kept and created on click. The
    // title is what the click needs, and `[[Basel III|the accord]]` shows
    // something else, which is why the row stores both.
    let mut f = fixture();
    let body = "I should read [[Basel III|the accord]].";
    let page = write_page(&mut f, "Reading notes", body);
    vault::save_links(&mut f.store, &f.profile.clone(), &page, body).expect("links");

    let links = repo::page_links(&f.store, &page).expect("links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target_kind"], "unresolved");
    assert_eq!(links[0]["target_title"], "Basel III");
    assert_eq!(links[0]["display_text"], "the accord");

    // The page arrives, by hand or from the vault, and the link lights up.
    let arrived = write_page(&mut f, "Basel III", "The accord.");
    let resolved = repo::resolve_pending_links(&f.store, "page", &arrived, "Basel III").expect("resolve");
    assert_eq!(resolved, 1);
    assert_eq!(
        repo::backlinks(&f.store, "page", &arrived)
            .expect("backlinks")
            .len(),
        1
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_body_edited_to_drop_a_link_drops_the_backlink_with_it() {
    // Replace rather than merge: the body is the truth about what it links to.
    let mut f = fixture();
    let target = write_page(&mut f, "Liquidity risk", "The rule.");
    let from = write_page(&mut f, "Reading notes", "See [[Liquidity risk]].");
    let profile = f.profile.clone();
    vault::save_links(&mut f.store, &profile, &from, "See [[Liquidity risk]].").expect("links");
    assert_eq!(repo::backlinks(&f.store, "page", &target).expect("b").len(), 1);

    vault::save_links(&mut f.store, &profile, &from, "I removed that link.").expect("links");
    assert!(
        repo::backlinks(&f.store, "page", &target).expect("b").is_empty(),
        "a link the person deleted still shows in the target's backlinks"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn backlinks_are_an_index_lookup_rather_than_a_scan() {
    // Doc 16 phase 12c accepts on exactly this. A backlinks panel that scanned
    // every body in the vault would work on ten pages and stop working on a
    // thousand, which is the size at which a person starts needing it.
    let f = fixture();
    let plan: String = f
        .store
        .conn()
        .query_row(
            "EXPLAIN QUERY PLAN
             SELECT l.from_page_id FROM page_link l JOIN page p ON p.id = l.from_page_id
              WHERE l.target_kind = 'page' AND l.target_id = 'x'",
            [],
            |r| r.get(3),
        )
        .expect("plan");
    assert!(
        plan.contains("USING INDEX page_link_target"),
        "the backlink query stopped using its index: {plan}"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn every_link_in_a_vault_is_reachable_from_the_page_it_names() {
    // Doc 16's backlink completeness, gated at 1.00 and absolute. The eval half
    // of it needs the synthetic vault at 12a-iv; this is the same property
    // asserted exhaustively over a vault the test builds, where the answer is
    // known rather than sampled.
    let mut f = fixture();
    let profile = f.profile.clone();
    let titles: Vec<String> = (0..12).map(|n| format!("Page {n}")).collect();
    let ids: Vec<String> = titles
        .iter()
        .map(|t| write_page(&mut f, t, "placeholder"))
        .collect();

    // Every page links to every page after it, so the counts differ per target
    // and an off by one would show.
    let mut expected: std::collections::BTreeMap<String, usize> = Default::default();
    for (i, id) in ids.iter().enumerate() {
        let body = titles[i + 1..]
            .iter()
            .map(|t| format!("[[{t}]]"))
            .collect::<Vec<_>>()
            .join(" and ");
        repo::edit_page(&mut f.store, id, &body).expect("edit");
        vault::save_links(&mut f.store, &profile, id, &body).expect("links");
        for t in &titles[i + 1..] {
            *expected.entry(t.clone()).or_default() += 1;
        }
    }

    for (i, id) in ids.iter().enumerate() {
        let found = repo::backlinks(&f.store, "page", id).expect("backlinks").len();
        assert_eq!(
            found,
            expected.get(&titles[i]).copied().unwrap_or(0),
            "page {} has the wrong number of links into it",
            titles[i]
        );
    }
    let rows: i64 = f
        .store
        .conn()
        .query_row("SELECT count(*) FROM page_link", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        rows as usize,
        expected.values().sum::<usize>(),
        "a link went missing"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_vault_file_that_links_is_read_and_its_links_stored() {
    // The mirror and the parser meet here: a file written in an editor arrives
    // as a page, and what it links to arrives with it.
    let mut f = fixture();
    let target = write_page(&mut f, "Liquidity risk", "The rule.");
    sync(&mut f);

    std::fs::write(
        f.root.join("vault/from-the-editor.md"),
        "# From the editor\n\nSee [[Liquidity risk]].",
    )
    .expect("write");
    let report = sync(&mut f);
    assert_eq!(report.created, 1, "{report:?}");

    let back = repo::backlinks(&f.store, "page", &target).expect("backlinks");
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].page_title, "From the editor");

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn the_parser_and_the_store_agree_about_what_a_link_is() {
    // The bodies below are the ones a vault about this feature would contain.
    let mut f = fixture();
    let profile = f.profile.clone();
    let body = "Write `[[Title]]` to link.\n\n```\n[[Also ignored]]\n```\n\nBut [[Real one]] counts.";
    let page = write_page(&mut f, "How linking works", body);
    vault::save_links(&mut f.store, &profile, &page, body).expect("links");

    let links = repo::page_links(&f.store, &page).expect("links");
    assert_eq!(links.len(), 1, "code samples became links: {links:?}");
    assert_eq!(links[0]["target_title"], "Real one");
    assert_eq!(wikilink::parse(body).len(), 1);

    std::fs::remove_dir_all(&f.root).ok();
}

// ------------------------------------------------------------- the editor

#[test]
fn writing_a_page_makes_a_file_and_editing_its_title_moves_it() {
    // Doc 16 phase 12c's editor. A rename keeps the id, which is what the links
    // hold, and moves the file, which is what a person sees in their vault.
    let mut f = fixture();
    let profile = f.profile.clone();
    let pack = f.pack.clone();

    let id = vault::write_page(
        &mut f.store,
        &profile,
        Some(&pack),
        None,
        "Liquidity risk",
        "# Liquidity risk\n\nThe rule is in article 12.",
    )
    .expect("written");
    assert!(f.root.join("vault/liquidity-risk.md").exists());

    let renamed = vault::write_page(
        &mut f.store,
        &profile,
        Some(&pack),
        Some(&id),
        "Liquidity coverage",
        "# Liquidity coverage\n\nThe rule is in article 12.",
    )
    .expect("renamed");
    assert_eq!(renamed, id, "the rename made a different page");
    assert!(f.root.join("vault/liquidity-coverage.md").exists());
    assert!(
        !f.root.join("vault/liquidity-risk.md").exists(),
        "the old file was left behind, so the next sync adopts it as a second page"
    );

    // And the mirror has nothing to do, because the write already agreed.
    let report = sync(&mut f);
    assert_eq!(
        (report.written, report.created, report.conflicts),
        (0, 0, 0),
        "{report:?}"
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_title_another_page_has_is_refused_rather_than_colliding() {
    let mut f = fixture();
    let profile = f.profile.clone();
    let pack = f.pack.clone();
    vault::write_page(&mut f.store, &profile, Some(&pack), None, "Liquidity risk", "one").expect("first");

    let clash = vault::write_page(&mut f.store, &profile, Some(&pack), None, "LIQUIDITY RISK", "two");
    assert!(matches!(
        clash,
        Err(vault::SaveError::Refused(vault::TITLE_TAKEN_BY_A_PAGE))
    ));

    let untitled = vault::write_page(&mut f.store, &profile, Some(&pack), None, "   ", "two");
    assert!(matches!(
        untitled,
        Err(vault::SaveError::Refused(vault::NO_TITLE_GIVEN))
    ));

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_page_written_for_an_unresolved_link_lights_the_link_up() {
    // Doc 16 section 3.1: "Unresolved links create the page on click."
    let mut f = fixture();
    let profile = f.profile.clone();
    let pack = f.pack.clone();
    let from = vault::write_page(
        &mut f.store,
        &profile,
        Some(&pack),
        None,
        "Reading notes",
        "I should read [[Basel III|the accord]].",
    )
    .expect("written");
    assert_eq!(
        repo::page_links(&f.store, &from).expect("links")[0]["target_kind"],
        "unresolved"
    );

    let arrived = vault::write_page(
        &mut f.store,
        &profile,
        Some(&pack),
        None,
        "Basel III",
        "# Basel III\n\n",
    )
    .expect("written");

    assert_eq!(
        repo::page_links(&f.store, &from).expect("links")[0]["target_kind"],
        "page",
        "the link that named this page did not light up"
    );
    assert_eq!(
        repo::backlinks(&f.store, "page", &arrived)
            .expect("backlinks")
            .len(),
        1
    );

    std::fs::remove_dir_all(&f.root).ok();
}

#[test]
fn a_deleted_page_leaves_no_file_and_nothing_to_retrieve() {
    // Doc 16 section 2.1: a citation names a Passage that carries its own text,
    // so an answer that quoted the page is untouched. What must go is the
    // index, or the page is retrieved and cited as a source nobody can open.
    let mut f = fixture();
    let profile = f.profile.clone();
    let pack = f.pack.clone();
    let id = vault::write_page(
        &mut f.store,
        &profile,
        Some(&pack),
        None,
        "Liquidity risk",
        "# Liquidity risk\n\nThe buffer is 2.5 %.",
    )
    .expect("written");
    let indexed: i64 = f
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM index_entry WHERE folder_id = 'vault'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert!(indexed > 0, "a written page was never indexed");

    vault::delete_page(&mut f.store, &id).expect("deleted");

    assert!(repo::read_page(&f.store, &id).expect("read").is_none());
    assert!(!f.root.join("vault/liquidity-risk.md").exists());
    let left: i64 = f
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM index_entry WHERE folder_id = 'vault'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(left, 0, "the page is gone and its chunks are still retrievable");

    std::fs::remove_dir_all(&f.root).ok();
}
