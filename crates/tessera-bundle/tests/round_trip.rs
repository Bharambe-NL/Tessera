#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! A board out and back in. Doc 01 section 7, doc 12 phase 10.
//!
//! Doc 12's walkthrough rows 10 and 11 are "exports a bundle" and "imports it on
//! a second machine", so every test here uses two stores. One store would prove
//! that the writer and the reader agree with each other, which is the one thing
//! that is true whatever they do.

use std::io::Cursor;

use rusqlite::params;
use serde_json::{Value, json};
use tessera_bundle::{ExportOptions, export, import, preflight};
use tessera_schema::Registry;
use tessera_store::{Store, new_id, now_iso8601, repo};

/// One compiled schema set per test. Cheap enough at this scale, and it keeps
/// the fixture from carrying a registry through every helper.
fn registry() -> Registry {
    Registry::load().expect("schemas")
}

struct Profile {
    store: Store,
    profile: String,
    pack: String,
}

/// An empty profile with a pack, which is the least a board needs to exist.
fn profile(pack_code: &str) -> Profile {
    let mut store = Store::open_in_memory().expect("store");
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());
    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, ?2, '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?3)",
            params![pack, pack_code, now],
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
    let _ = &mut store;
    Profile { store, profile, pack }
}

/// A board with one answered card, one citation, one source, one concept.
///
/// Written through `repo` rather than by hand wherever `repo` has a writer, so
/// the fixture is a board the product could have produced.
fn seeded(p: &mut Profile, source_class: &str, locator: &str) -> String {
    let board = repo::create_board(
        &mut p.store,
        repo::NewBoard {
            profile_id: &p.profile,
            title: "Capital rules",
            doctrine_pack_id: &p.pack,
            default_depth: "deep",
            named_by_user: true,
            parent_board_id: None,
            seed_label: None,
            context: None,
        },
    )
    .expect("board");

    let card = repo::create_card(
        &mut p.store,
        repo::NewCard {
            board_id: &board,
            parent_card_id: None,
            kind: "root",
            question: "What buffer applies?",
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )
    .expect("card");

    let now = now_iso8601();
    let run = repo::start_run(
        &p.store,
        repo::NewRun {
            board_id: &board,
            card_id: Some(&card),
            kind: "card",
            depth: Some("deep"),
            policy_snapshot: &json!({}),
            pack_version: "1.0",
        },
    )
    .expect("run");

    repo::write_answer(
        &mut p.store,
        repo::CardRef {
            card_id: &card,
            board_id: &board,
            run_id: &run,
        },
        "The buffer is 2.5 per cent.",
        &json!([]),
        &json!({ "agent_id": "synthesizer" }),
        json!({ "card_id": card }),
    )
    .expect("answer");

    repo::write_citation(
        &mut p.store,
        &p.profile,
        repo::CardRef {
            card_id: &card,
            board_id: &board,
            run_id: &run,
        },
        repo::NewCitation {
            ordinal: 1,
            source_title: "The rule",
            source_class,
            locator,
            issuer: None,
            freshness_class: "stable",
            trust_rank: 1,
            passage_text: "The buffer is 2.5 per cent.",
            claim_span: json!({ "start": 0, "end": 27 }),
            binding: "answer",
        },
    )
    .expect("citation");

    repo::finish_card(
        &mut p.store,
        repo::CardRef {
            card_id: &card,
            board_id: &board,
            run_id: &run,
        },
        0.9,
        &[(1, "supported".to_string())],
        &json!({}),
        &[],
    )
    .expect("finish");

    // One concept, linked to the card, so the term collision rule has something
    // to collide with on the way in.
    let concept = new_id();
    p.store
        .conn()
        .execute(
            "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status, created_at, updated_at)
             VALUES (?1, ?2, 'capital buffer', ?3, 'confirmed', ?4, ?4)",
            params![concept, p.profile, p.pack, now],
        )
        .expect("concept");
    p.store
        .conn()
        .execute(
            "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation,
                 proposed_by, status, created_at)
             VALUES (?1, ?2, 'card', ?3, 'mentions', 'indexer', 'confirmed', ?4)",
            params![new_id(), concept, card, now],
        )
        .expect("link");

    board
}

/// Save a page from the board's card, the way doc 16 section 3.2's Save as
/// page does: the card's citations carried, the chip set on the card, and a
/// second page it links to.
fn saved_page(p: &mut Profile, board: &str) -> (String, String) {
    let card: String = p
        .store
        .conn()
        .query_row("SELECT id FROM card WHERE board_id = ?1", [board], |r| r.get(0))
        .expect("card");
    let carried: Vec<Value> = p
        .store
        .conn()
        .prepare("SELECT ordinal, passage_id FROM citation WHERE card_id = ?1")
        .and_then(|mut s| {
            s.query_map([&card], |r| {
                Ok(json!({ "ordinal": r.get::<_, i64>(0)?, "passage_id": r.get::<_, String>(1)? }))
            })?
            .collect()
        })
        .expect("citations");

    let other = repo::create_page(
        &mut p.store,
        repo::NewPage {
            profile_id: &p.profile,
            title: "Liquidity risk",
            body: "Nothing yet.",
            file_path: "vault/liquidity-risk.md",
            source_card_id: None,
            citations_carried: json!([]),
            doctrine_pack_id: Some(&p.pack),
        },
    )
    .expect("other page");

    let page = repo::create_page(
        &mut p.store,
        repo::NewPage {
            profile_id: &p.profile,
            title: "Capital buffer",
            body: "The buffer is 2.5 per cent. See [[Liquidity risk]] and [[Nothing here]].",
            file_path: "vault/capital-buffer.md",
            source_card_id: Some(&card),
            citations_carried: json!(carried),
            doctrine_pack_id: Some(&p.pack),
        },
    )
    .expect("page");
    repo::set_card_page(&p.store, &card, &page).expect("chip");
    repo::replace_page_links(
        &mut p.store,
        &page,
        &[
            repo::NewPageLink {
                target_kind: "page".into(),
                target_id: Some(other.clone()),
                target_title: "Liquidity risk".into(),
                display_text: "Liquidity risk".into(),
                position: 28,
            },
            repo::NewPageLink {
                target_kind: "unresolved".into(),
                target_id: None,
                target_title: "Nothing here".into(),
                display_text: "Nothing here".into(),
                position: 51,
            },
        ],
    )
    .expect("links");
    (page, other)
}

fn round_trip(
    from: &mut Profile,
    board: &str,
    options: &ExportOptions,
    to: &mut Profile,
) -> (Value, tessera_bundle::ImportOutcome) {
    let mut archive = Cursor::new(Vec::new());
    let manifest = export(&mut from.store, &registry(), board, options, &mut archive).expect("export");
    archive.set_position(0);
    let outcome = import(&mut to.store, &to.profile.clone(), archive).expect("import");
    (manifest, outcome)
}

fn one(store: &Store, sql: &str, id: &str) -> i64 {
    store.conn().query_row(sql, [id], |r| r.get(0)).expect("count")
}

#[test]
fn a_board_arrives_whole_on_a_second_machine() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let mut receiver = profile("general");

    let options = ExportOptions {
        exported_by: Some("A name".into()),
        ..Default::default()
    };
    let (manifest, outcome) = round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(manifest["format_version"], "1.0");
    assert_eq!(outcome.board_id, board);
    assert_eq!(outcome.board_title, "Capital rules");

    // Doc 01 section 7: imported rows keep their ids, so the recipient's copy
    // is the same board rather than a lookalike.
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM board WHERE id = ?1",
            &board
        ),
        1
    );
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM card WHERE board_id = ?1",
            &board
        ),
        1
    );

    // And the citation resolves, which is the whole reason passages travel: the
    // recipient can audit the claim without retrieving anything again.
    let text: String = receiver
        .store
        .conn()
        .query_row(
            "SELECT p.text FROM passage p
             JOIN citation ci ON ci.passage_id = p.id
             JOIN card c ON c.id = ci.card_id WHERE c.board_id = ?1",
            [&board],
            |r| r.get(0),
        )
        .expect("the citation does not resolve on the recipient's machine");
    assert_eq!(text, "The buffer is 2.5 per cent.");

    // Doc 01 section 7: the fork records where it came from.
    let forked: String = receiver
        .store
        .conn()
        .query_row(
            "SELECT forked_from_bundle_id FROM board WHERE id = ?1",
            [&board],
            |r| r.get(0),
        )
        .expect("forked_from_bundle_id");
    assert_eq!(forked, manifest["bundle_id"].as_str().unwrap());
}

#[test]
fn importing_the_same_bundle_twice_changes_nothing() {
    // The second import is usually an accident, and doc 01 section 7 says
    // import never overwrites. Duplicating the board would be the one outcome
    // worse than refusing.
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let mut receiver = profile("general");

    let mut archive = Cursor::new(Vec::new());
    export(
        &mut sender.store,
        &registry(),
        &board,
        &ExportOptions::default(),
        &mut archive,
    )
    .expect("export");

    for pass in 1..=2 {
        archive.set_position(0);
        let outcome =
            import(&mut receiver.store, &receiver.profile.clone(), archive.clone()).expect("import");
        if pass == 2 {
            assert_eq!(
                outcome.written.get("cards.jsonl"),
                None,
                "the second import wrote cards"
            );
            assert_eq!(outcome.skipped.get("cards.jsonl"), Some(&1));
        }
    }

    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM card WHERE board_id = ?1",
            &board
        ),
        1
    );
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM citation WHERE card_id IN (SELECT id FROM card WHERE board_id = ?1)",
            &board
        ),
        1
    );
}

#[test]
fn a_source_the_recipient_already_has_merges_by_dedupe_key() {
    // Doc 01 section 7. Two people citing the same page should end up with one
    // Source, or the Library fills with the same page over and over.
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");

    let mut receiver = profile("general");
    let mine = seeded(&mut receiver, "web", "https://example.test/rules");
    assert_ne!(mine, board);

    let (_, outcome) = round_trip(&mut sender, &board, &ExportOptions::default(), &mut receiver);
    assert_eq!(outcome.sources_merged, 1);
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM source WHERE profile_id = ?1",
            &receiver.profile
        ),
        1,
        "the same page arrived as a second Source"
    );

    // The incoming passage hangs from the source the recipient already had, so
    // the imported card's citation still resolves.
    let sources: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(DISTINCT p.source_id) FROM passage p
             JOIN citation ci ON ci.passage_id = p.id
             JOIN card c ON c.id = ci.card_id WHERE c.board_id = ?1",
            [&board],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(sources, 1);
}

#[test]
fn a_concept_term_collision_keeps_both_and_links_them() {
    // Doc 01 section 7: keep both, mark the incoming one proposed, link them
    // `related_to` for the user to reconcile. Merging silently would assert
    // that two people mean the same thing by one word.
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://a.test/one");
    let mut receiver = profile("general");
    seeded(&mut receiver, "web", "https://b.test/two");

    let (_, outcome) = round_trip(&mut sender, &board, &ExportOptions::default(), &mut receiver);
    assert_eq!(outcome.concepts_collided, 1);

    let both: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM concept WHERE profile_id = ?1 AND term = 'capital buffer'",
            [&receiver.profile],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(both, 2, "one of the two definitions was lost");

    let proposed: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM concept WHERE term = 'capital buffer' AND status = 'proposed'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(proposed, 1, "the incoming concept was not marked proposed");

    let linked: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM concept_link WHERE relation = 'related_to' AND target_type = 'concept'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(linked, 1, "nothing tells the user the two terms collided");
}

#[test]
fn a_local_document_does_not_travel_unless_the_author_says_so() {
    // Doc 01 section 7's checklist. The default is that nothing local leaves.
    let mut sender = profile("general");
    let board = seeded(
        &mut sender,
        "local_document",
        "/home/someone/Private/Risk/rules.pdf",
    );

    let check = preflight(&sender.store, &board).expect("preflight");
    assert_eq!(check.local_documents.len(), 1);
    assert_eq!(check.local_documents[0].file_name, "rules.pdf");

    let mut receiver = profile("general");
    let (manifest, _) = round_trip(&mut sender, &board, &ExportOptions::default(), &mut receiver);

    assert_eq!(manifest["local_documents"][0]["included"], false);
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM source WHERE profile_id = ?1",
            &receiver.profile
        ),
        0,
        "a local document left without being cleared"
    );
    // The card still arrives; it just cites something the recipient cannot open.
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM card WHERE board_id = ?1",
            &board
        ),
        1
    );
}

#[test]
fn a_cleared_local_document_travels_without_its_folder() {
    let mut sender = profile("general");
    let board = seeded(
        &mut sender,
        "local_document",
        "/home/someone/Private/Risk/rules.pdf",
    );
    let source: String = sender
        .store
        .conn()
        .query_row(
            "SELECT id FROM source WHERE profile_id = ?1",
            [&sender.profile],
            |r| r.get(0),
        )
        .expect("source");

    let mut receiver = profile("general");
    let options = ExportOptions {
        local_documents: [source].into_iter().collect(),
        ..Default::default()
    };
    round_trip(&mut sender, &board, &options, &mut receiver);

    let locator: String = receiver
        .store
        .conn()
        .query_row(
            "SELECT locator FROM source WHERE profile_id = ?1",
            [&receiver.profile],
            |r| r.get(0),
        )
        .expect("source");
    assert_eq!(
        locator, "rules.pdf",
        "the sender's folder travelled with the file"
    );
}

#[test]
fn export_without_history_carries_no_events() {
    // Doc 01 section 7's "export without history".
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let mut receiver = profile("general");

    let options = ExportOptions {
        with_history: false,
        ..Default::default()
    };
    let (manifest, outcome) = round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(manifest["includes"]["events"], false);
    assert_eq!(outcome.written.get("events.jsonl"), Some(&0));
    // The board is still whole; only the account of how it was built is gone.
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM card WHERE board_id = ?1",
            &board
        ),
        1
    );
}

#[test]
fn the_senders_history_arrives_as_a_replay_and_not_as_the_recipients_own() {
    // The events did not happen on this machine. Appending them as `live` would
    // have the recipient's own log claim they built a board they were given.
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let mut receiver = profile("general");

    round_trip(&mut sender, &board, &ExportOptions::default(), &mut receiver);

    let replayed: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM event WHERE board_id = ?1 AND source = 'replay'",
            [&board],
            |r| r.get(0),
        )
        .expect("count");
    assert!(replayed > 0, "the sender's history did not arrive");

    let live_card_events: i64 = receiver
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM event WHERE board_id = ?1 AND source = 'live'
             AND event_type != 'board.imported.v1'",
            [&board],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(
        live_card_events, 0,
        "imported history is claimed as this profile's own"
    );
}

#[test]
fn a_bundle_never_carries_the_senders_profile_id() {
    // Doc 10 section 8: a bundle carries a display name and no other identity.
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");

    let mut archive = Cursor::new(Vec::new());
    export(
        &mut sender.store,
        &registry(),
        &board,
        &ExportOptions::default(),
        &mut archive,
    )
    .expect("export");

    let bytes = archive.into_inner();
    let text = String::from_utf8_lossy(&bytes);
    // The archive is deflated, so a plain search over the bytes proves nothing.
    // Reading it back is the only honest check.
    let _ = text;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    for name in ["board.json", "sources.jsonl"] {
        let mut file = zip.by_name(name).expect(name);
        let mut content = String::new();
        std::io::Read::read_to_string(&mut file, &mut content).expect("read");
        assert!(
            !content.contains(&sender.profile),
            "{name} carries the sender's profile id"
        );
    }
}

// ------------------------------------------------------------------ pages ---

#[test]
fn a_page_does_not_travel_unless_the_author_ticks_it() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let (page, _) = saved_page(&mut sender, &board);
    let mut receiver = profile("general");

    // The checklist offers it, because it was saved from a card on this board.
    let offered = preflight(&sender.store, &board).expect("preflight");
    assert_eq!(offered.pages.len(), 1);
    assert_eq!(offered.pages[0].page_id, page);
    assert_eq!(offered.pages[0].citations_carried, 1);

    let (manifest, outcome) = round_trip(&mut sender, &board, &ExportOptions::default(), &mut receiver);

    assert_eq!(manifest["counts"]["pages.jsonl"], 0);
    assert_eq!(
        one(&receiver.store, "SELECT COUNT(*) FROM page WHERE id = ?1", &page),
        0
    );
    assert_eq!(outcome.written.get("pages.jsonl"), None);

    // And the page the author kept leaves no trace at all: doc 16's vault is a
    // person's own writing, so a withheld title is not even listed the way a
    // withheld local document's file name is.
    assert!(!manifest.to_string().contains("Capital buffer"));

    // The chip on the card names a page the recipient does not have, so it is
    // cleared rather than carried into a foreign key that cannot hold.
    let chip: Option<String> = receiver
        .store
        .conn()
        .query_row("SELECT page_id FROM card WHERE board_id = ?1", [&board], |r| {
            r.get(0)
        })
        .expect("card");
    assert_eq!(chip, None);
}

#[test]
fn a_ticked_page_arrives_with_its_evidence_and_its_links() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let (page, other) = saved_page(&mut sender, &board);
    let mut receiver = profile("general");

    let options = ExportOptions {
        // Both: the linked page is not on the board, and ticking it by hand is
        // how a link keeps its target.
        pages: [page.clone(), other.clone()].into_iter().collect(),
        ..Default::default()
    };
    let (manifest, outcome) = round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(manifest["counts"]["pages.jsonl"], 2);
    assert_eq!(outcome.pages_collided, 0);
    assert_eq!(outcome.carried_evidence_dropped, 0);

    // Doc 16 section 2.2: the carried passage resolves on the recipient's
    // machine, which is what makes the page citable there at all.
    let carried: String = receiver
        .store
        .conn()
        .query_row("SELECT citations_carried FROM page WHERE id = ?1", [&page], |r| {
            r.get(0)
        })
        .expect("page");
    let entries: Vec<Value> = serde_json::from_str(&carried).expect("carried json");
    assert_eq!(entries.len(), 1);
    let passage = entries[0]["passage_id"].as_str().unwrap().to_string();
    assert_eq!(
        one(
            &receiver.store,
            "SELECT COUNT(*) FROM passage WHERE id = ?1",
            &passage
        ),
        1
    );

    // The resolved link still points at the page it named, and the unresolved
    // one is still waiting for the same title.
    let kinds: Vec<(String, Option<String>, String)> = receiver
        .store
        .conn()
        .prepare(
            "SELECT target_kind, target_id, target_title FROM page_link
                  WHERE from_page_id = ?1 ORDER BY position",
        )
        .and_then(|mut s| {
            s.query_map([&page], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect()
        })
        .expect("links");
    assert_eq!(kinds.len(), 2);
    assert_eq!(kinds[0], ("page".into(), Some(other), "Liquidity risk".into()));
    assert_eq!(kinds[1].0, "unresolved");
    assert_eq!(kinds[1].2, "Nothing here");

    // And the card's chip points back at the page that arrived.
    let chip: Option<String> = receiver
        .store
        .conn()
        .query_row("SELECT page_id FROM card WHERE board_id = ?1", [&board], |r| {
            r.get(0)
        })
        .expect("card");
    assert_eq!(chip, Some(page));
}

#[test]
fn a_link_whose_target_stayed_behind_arrives_unresolved() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let (page, other) = saved_page(&mut sender, &board);
    let mut receiver = profile("general");

    let options = ExportOptions {
        pages: [page.clone()].into_iter().collect(),
        ..Default::default()
    };
    round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(
        one(&receiver.store, "SELECT COUNT(*) FROM page WHERE id = ?1", &other),
        0
    );
    let (kind, title): (String, String) = receiver
        .store
        .conn()
        .query_row(
            "SELECT target_kind, target_title FROM page_link
             WHERE from_page_id = ?1 ORDER BY position LIMIT 1",
            [&page],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("link");
    // Doc 16 section 3.1 keeps an unresolved link rather than dropping it, and
    // clicking it creates the page: a link that arrives pointing at nothing
    // would be a link the recipient can neither follow nor make good.
    assert_eq!(kind, "unresolved");
    assert_eq!(title, "Liquidity risk");
}

#[test]
fn a_page_title_collision_keeps_both() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "web", "https://example.test/rules");
    let (page, _) = saved_page(&mut sender, &board);
    let mut receiver = profile("general");

    // The recipient wrote their own page about the same thing, in their own
    // words, under the same name and the same file.
    repo::create_page(
        &mut receiver.store,
        repo::NewPage {
            profile_id: &receiver.profile.clone(),
            title: "capital buffer",
            body: "What I think about it.",
            file_path: "vault/capital-buffer.md",
            source_card_id: None,
            citations_carried: json!([]),
            doctrine_pack_id: None,
        },
    )
    .expect("their page");

    let options = ExportOptions {
        pages: [page.clone()].into_iter().collect(),
        ..Default::default()
    };
    let (_, outcome) = round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(outcome.pages_collided, 1);
    // Both, the way a colliding concept keeps both: one word, two people, and
    // nothing here can tell which of them is right.
    let pages: i64 = receiver
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM page", [], |r| r.get(0))
        .expect("pages");
    assert_eq!(pages, 2);
    let (title, path): (String, String) = receiver
        .store
        .conn()
        .query_row("SELECT title, file_path FROM page WHERE id = ?1", [&page], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("the imported page");
    assert_eq!(title, "Capital buffer (conflict)");
    assert_eq!(path, "vault/capital-buffer 2.md");
    let mine: String = receiver
        .store
        .conn()
        .query_row(
            "SELECT body FROM page WHERE file_path = 'vault/capital-buffer.md'",
            [],
            |r| r.get(0),
        )
        .expect("their own page");
    assert_eq!(mine, "What I think about it.");
}

#[test]
fn carried_evidence_the_author_withheld_does_not_arrive_dangling() {
    let mut sender = profile("general");
    let board = seeded(&mut sender, "local_document", "/home/someone/Private/rules.pdf");
    let (page, _) = saved_page(&mut sender, &board);
    let mut receiver = profile("general");

    // The page's evidence is a local document, and the author cleared the page
    // without clearing the document.
    let options = ExportOptions {
        pages: [page.clone()].into_iter().collect(),
        ..Default::default()
    };
    let (_, outcome) = round_trip(&mut sender, &board, &options, &mut receiver);

    assert_eq!(outcome.carried_evidence_dropped, 1);
    let carried: String = receiver
        .store
        .conn()
        .query_row("SELECT citations_carried FROM page WHERE id = ?1", [&page], |r| {
            r.get(0)
        })
        .expect("page");
    // Empty rather than pointing at a passage that is not there: doc 16 makes
    // carried evidence the reason a page can support a claim, so evidence that
    // did not travel has to stop claiming to.
    assert_eq!(carried, "[]");
}
