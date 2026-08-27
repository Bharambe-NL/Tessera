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
