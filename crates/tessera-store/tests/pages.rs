#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Vault storage. Doc 16 sections 3.1 and 4.
//!
//! A page is a document the person wrote, or one saved from a card they read,
//! and the two rules that make it worth having are both here: the title is
//! unique per profile case insensitively, and a rename keeps the id, because
//! the id is what a wikilink resolves to. Doc 16 section 2.2 lists resolution
//! by title string as one of the assessed package's mistakes, and this is the
//! table that makes it avoidable.

use rusqlite::params;
use serde_json::json;
use tessera_store::repo::{self, NewPage};
use tessera_store::{Store, new_id, now_iso8601};

struct Fixture {
    store: Store,
    profile: String,
    pack: String,
}

fn fixture() -> Fixture {
    let store = Store::open_in_memory().expect("store");
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());

    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .expect("pack");
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .expect("profile");

    Fixture { store, profile, pack }
}

fn page(f: &mut Fixture, title: &str, body: &str) -> String {
    let slug = title.to_lowercase().replace(' ', "-");
    repo::create_page(
        &mut f.store,
        NewPage {
            profile_id: &f.profile,
            title,
            body,
            file_path: &format!("vault/{slug}.md"),
            source_card_id: None,
            citations_carried: json!([]),
            doctrine_pack_id: Some(&f.pack),
        },
    )
    .expect("page")
}

/// A board with one answered card on it, for the pages that came from one.
fn card(f: &mut Fixture) -> String {
    let now = now_iso8601();
    let board = repo::create_board(
        &mut f.store,
        repo::NewBoard {
            profile_id: &f.profile,
            title: "A board",
            doctrine_pack_id: &f.pack,
            default_depth: "deep",
            named_by_user: false,
            parent_board_id: None,
            seed_label: None,
            context: None,
        },
    )
    .expect("board");
    let card_id = new_id();
    f.store
        .conn()
        .execute(
            "INSERT INTO card (id, board_id, kind, question, depth, status, created_at, updated_at)
             VALUES (?1, ?2, 'root', 'what is the buffer?', 'deep', 'done', ?3, ?3)",
            params![card_id, board, now],
        )
        .expect("card");
    card_id
}

fn event_types(f: &Fixture) -> Vec<String> {
    f.store
        .events(None)
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect()
}

#[test]
fn a_page_written_by_hand_says_so_in_the_log() {
    let mut f = fixture();
    let id = page(&mut f, "Liquidity risk", "The rule is in article 12.");

    let row = repo::read_page(&f.store, &id).expect("read").expect("the page");
    assert_eq!(row.title, "Liquidity risk");
    assert_eq!(row.body, "The rule is in article 12.");
    assert_eq!(row.file_path, "vault/liquidity-risk.md");
    assert_eq!(row.source_card_id, None);
    assert!(!row.synced_hash.is_empty(), "the mirror compares by hash");

    let types = event_types(&f);
    assert!(types.contains(&"page.created.v1".to_string()), "{types:?}");
    assert!(
        !types.contains(&"page.created_from_card.v1".to_string()),
        "a page nobody saved from a card claimed a card: {types:?}"
    );
}

#[test]
fn a_page_saved_from_a_card_carries_its_citations_and_names_it() {
    // Doc 16 section 2.2 is the whole reason this column exists: the assessed
    // package pointed a citation at the note, so two hops later the regulation
    // was out of reach. A page carries the passages, and they are copied once
    // rather than re-derived from the page's own text.
    let mut f = fixture();
    let card_id = card(&mut f);
    let carried = json!([{ "ordinal": 1, "passage_id": "p-0001" }]);
    let id = repo::create_page(
        &mut f.store,
        NewPage {
            profile_id: &f.profile,
            title: "The buffer",
            body: "The buffer is 2.5 %.",
            file_path: "vault/the-buffer.md",
            source_card_id: Some(&card_id),
            citations_carried: carried.clone(),
            doctrine_pack_id: Some(&f.pack),
        },
    )
    .expect("page");

    let row = repo::read_page(&f.store, &id).expect("read").expect("the page");
    assert_eq!(row.source_card_id.as_deref(), Some(card_id.as_str()));
    assert_eq!(row.citations_carried, carried);

    let types = event_types(&f);
    assert!(
        types.contains(&"page.created_from_card.v1".to_string()),
        "{types:?}"
    );
}

#[test]
fn two_pages_cannot_share_a_title_whatever_the_capitals() {
    // Doc 16 section 3.1: unique per profile, case insensitive. The person
    // keeps the capitals they typed and only the comparison ignores them.
    let mut f = fixture();
    page(&mut f, "Liquidity risk", "one");

    let clash = repo::create_page(
        &mut f.store,
        NewPage {
            profile_id: &f.profile,
            title: "LIQUIDITY RISK",
            body: "two",
            file_path: "vault/liquidity-risk-2.md",
            source_card_id: None,
            citations_carried: json!([]),
            doctrine_pack_id: Some(&f.pack),
        },
    );
    assert!(clash.is_err(), "a second page took a title that was taken");

    // And the collision left nothing behind, because the row and its event are
    // one transaction.
    assert_eq!(repo::list_pages(&f.store, &f.profile, 10).expect("list").len(), 1);
    assert_eq!(
        event_types(&f).iter().filter(|t| *t == "page.created.v1").count(),
        1,
        "a refused write announced itself anyway"
    );
}

#[test]
fn a_rename_keeps_the_id_a_wikilink_resolves_to() {
    let mut f = fixture();
    let id = page(&mut f, "Liquidity risk", "one");

    repo::rename_page(
        &mut f.store,
        &id,
        "Liquidity coverage",
        "vault/liquidity-coverage.md",
    )
    .expect("rename");

    let row = repo::read_page(&f.store, &id)
        .expect("read")
        .expect("still there");
    assert_eq!(row.id, id, "the rename made a different page");
    assert_eq!(row.title, "Liquidity coverage");
    assert_eq!(row.file_path, "vault/liquidity-coverage.md");

    // And the old title no longer resolves, which is the point of resolving by
    // id rather than by title.
    assert!(
        repo::page_by_title(&f.store, &f.profile, "Liquidity risk")
            .expect("by title")
            .is_none()
    );
    assert_eq!(
        repo::page_by_title(&f.store, &f.profile, "liquidity COVERAGE")
            .expect("by title")
            .map(|p| p.id),
        Some(id)
    );
}

#[test]
fn an_edit_leaves_the_hash_that_records_the_last_agreement() {
    // Doc 16 section 7 point 2 wants last write wins with a conflict copy, and
    // deciding which copy moved needs three values: what the row says, what the
    // file says, and what they last agreed on. An edit is exactly the event
    // that makes the first two disagree, so moving the third would erase the
    // evidence the mirror reads.
    let mut f = fixture();
    let id = page(&mut f, "Liquidity risk", "one");
    let agreed = repo::read_page(&f.store, &id)
        .expect("read")
        .expect("page")
        .synced_hash;

    repo::edit_page(&mut f.store, &id, "one, with a correction").expect("edit");

    let after = repo::read_page(&f.store, &id).expect("read").expect("page");
    assert_eq!(after.body, "one, with a correction");
    assert_eq!(
        after.synced_hash, agreed,
        "the edit moved the record of what the file and the row agreed on"
    );
    assert!(event_types(&f).contains(&"page.edited.v1".to_string()));

    // And the mirror is what records the new agreement, once it has written it.
    repo::mark_page_synced(&f.store, &id, "one, with a correction").expect("synced");
    let synced = repo::read_page(&f.store, &id).expect("read").expect("page");
    assert_ne!(synced.synced_hash, agreed);
}

#[test]
fn a_deleted_page_leaves_the_answers_that_cited_it_alone() {
    // Doc 16 section 2.1: a citation names a Passage and the passage carries
    // its own verbatim text, so deleting the page cannot reach into an answer
    // that quoted it.
    let mut f = fixture();
    let id = page(&mut f, "Liquidity risk", "one");
    repo::delete_page(&mut f.store, &id).expect("delete");

    assert!(repo::read_page(&f.store, &id).expect("read").is_none());
    assert!(event_types(&f).contains(&"page.deleted.v1".to_string()));
}

#[test]
fn a_page_link_row_survives_its_page_and_nothing_else_does() {
    // The backlink query is `where target_id = ?`, so the target is a column
    // and never a scan over bodies. And a page that goes takes its outbound
    // links with it, because they were part of its text.
    let mut f = fixture();
    let from = page(&mut f, "Liquidity risk", "See [[The buffer]].");
    let to = page(&mut f, "The buffer", "2.5 %.");

    f.store
        .conn()
        .execute(
            "INSERT INTO page_link (id, from_page_id, target_kind, target_id, display_text,
                 position, created_at)
             VALUES (?1, ?2, 'page', ?3, 'The buffer', 4, ?4)",
            params![new_id(), from, to, now_iso8601()],
        )
        .expect("link");

    let backlinks: i64 = f
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM page_link WHERE target_kind = 'page' AND target_id = ?1",
            params![to],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(backlinks, 1);

    repo::delete_page(&mut f.store, &from).expect("delete");
    let left: i64 = f
        .store
        .conn()
        .query_row("SELECT count(*) FROM page_link", [], |r| r.get(0))
        .expect("count");
    assert_eq!(left, 0, "the link outlived the body it was written in");
}

#[test]
fn a_card_can_name_the_page_it_was_saved_as() {
    // Doc 16 section 4: the card header shows a page chip from this.
    let mut f = fixture();
    let card_id = card(&mut f);
    let page_id = page(&mut f, "The buffer", "2.5 %.");
    repo::set_card_page(&f.store, &card_id, &page_id).expect("chip");

    let named: Option<String> = f
        .store
        .conn()
        .query_row("SELECT page_id FROM card WHERE id = ?1", params![card_id], |r| {
            r.get(0)
        })
        .expect("read");
    assert_eq!(named.as_deref(), Some(page_id.as_str()));
}
