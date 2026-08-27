#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Retrieval persistence. Doc 05 sections 5, 7 and 12.
//!
//! Each test here covers a gate or a promise rather than an implementation
//! detail: a mirrored page becomes one Source, a sensitive folder's text never
//! reaches the row, a denial does not name what it refused, and the log replays.

use rusqlite::params;
use serde_json::json;
use tessera_store::repo::{self, NewPassage, RetrievalRef};
use tessera_store::{Store, new_id, now_iso8601};

struct Fixture {
    store: Store,
    profile: String,
    board: String,
    card: String,
    run: String,
}

fn fixture() -> Fixture {
    let mut store = Store::open_in_memory().expect("store");
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

    let board = repo::create_board(
        &mut store,
        repo::NewBoard {
            profile_id: &profile,
            title: "A board",
            doctrine_pack_id: &pack,
            default_depth: "deep",
            named_by_user: false,
            parent_board_id: None,
            seed_label: None,
            context: None,
        },
    )
    .expect("board");

    let card = repo::create_card(
        &mut store,
        repo::NewCard {
            board_id: &board,
            parent_card_id: None,
            kind: "root",
            question: "What applies here?",
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )
    .expect("card");

    let run = repo::start_run(
        &store,
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

    Fixture {
        store,
        profile,
        board,
        card,
        run,
    }
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

fn events(store: &Store, board: &str) -> Vec<String> {
    store
        .events(Some(board))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect()
}

fn passage<'a>(locator: &'a str, text: &'a str) -> NewPassage<'a> {
    NewPassage {
        class: "web",
        title: "A page",
        locator,
        issuer: Some("ledgerline.invalid"),
        published_at: Some("2025-05-22"),
        freshness_class: "web_commentary",
        trust_rank: 6,
        version_ref: None,
        content_hash: "abc123",
        text,
        location: json!({ "kind": "heading", "title": "Point 1" }),
        text_withheld: false,
    }
}

#[test]
fn a_mirrored_page_becomes_one_source() {
    // Doc 05 section 12: zero duplicate Sources for mirrored pages. Four
    // spellings of one page is the shape this arrives in: a redirect, a
    // tracking parameter, a trailing slash, a capitalised host.
    let Fixture {
        mut store,
        profile,
        board,
        card,
        run,
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "web",
        sq_id: Some("sq-1"),
    };

    let spellings = [
        "https://ledgerline.invalid/capital/buffers",
        "http://www.ledgerline.invalid/capital/buffers/",
        "HTTPS://Ledgerline.Invalid/capital/buffers?utm_source=news",
        "https://ledgerline.invalid/capital/buffers#section-2",
    ];
    let passages: Vec<NewPassage<'_>> = spellings
        .iter()
        .map(|l| passage(l, "The buffer is 2.5 percent."))
        .collect();

    let retained = repo::record_retrieval(&mut store, &profile, here, &passages, "full", 12).expect("record");

    assert_eq!(
        count(&store, "source"),
        1,
        "the mirrored page produced more than one source"
    );
    assert_eq!(retained.sources_created, 1);
    assert_eq!(retained.sources_deduplicated, 3);
    // Every passage is kept: four retrievals of one page are four passages of
    // one source, not one passage.
    assert_eq!(count(&store, "passage"), 4);
    assert_eq!(retained.source_ids.len(), 1);
}

#[test]
fn a_deduplicated_source_says_so_in_the_log() {
    let Fixture {
        mut store,
        profile,
        board,
        card,
        run,
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "web",
        sq_id: None,
    };
    let passages = [
        passage("https://a.invalid/one", "First."),
        passage("https://a.invalid/one/", "Second."),
    ];
    repo::record_retrieval(&mut store, &profile, here, &passages, "full", 5).expect("record");

    let seen = events(&store, &board);
    assert_eq!(seen.iter().filter(|e| *e == "source.created.v1").count(), 1);
    assert_eq!(seen.iter().filter(|e| *e == "source.deduplicated.v1").count(), 1);
}

#[test]
fn a_sensitive_passage_keeps_its_location_and_loses_its_text() {
    // Doc 01 open question 2 as resolved: a sensitive folder stores offsets
    // rather than verbatim text. A citation into it stays checkable by the
    // person who owns the folder and carries nothing to anyone a bundle reaches.
    let Fixture {
        mut store,
        profile,
        board,
        card,
        run,
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "local",
        sq_id: None,
    };

    let secret = NewPassage {
        class: "local_document",
        text: "The board minute nobody outside the firm may read.",
        text_withheld: true,
        ..passage("internal/Sensitive/minutes.md", "unused")
    };
    repo::record_retrieval(&mut store, &profile, here, &[secret], "full", 1).expect("record");

    let (text, location, withheld): (Option<String>, String, i64) = store
        .conn()
        .query_row(
            "SELECT text, location, text_withheld FROM passage LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("passage");

    assert_eq!(text, None, "the sensitive text was stored");
    assert_eq!(withheld, 1);
    assert!(
        location.contains("heading"),
        "the location was lost with the text"
    );
}

#[test]
fn the_events_tell_the_whole_story_in_order() {
    // Doc 05 section 7. `started` lands before anything is fetched, so an
    // assignment that hangs is visible in the log rather than absent from it.
    let Fixture {
        mut store,
        profile,
        board,
        card,
        run,
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "web",
        sq_id: Some("sq-1"),
    };

    repo::start_retrieval(&mut store, here, "capital buffers").expect("started");
    repo::record_retrieval(
        &mut store,
        &profile,
        here,
        &[passage("https://a.invalid/x", "Something.")],
        "partial",
        42,
    )
    .expect("record");

    let seen = events(&store, &board);
    let started = seen
        .iter()
        .position(|e| e == "retrieval.started.v1")
        .expect("started");
    let created = seen
        .iter()
        .position(|e| e == "source.created.v1")
        .expect("created");
    let completed = seen
        .iter()
        .position(|e| e == "retrieval.completed.v1")
        .expect("completed");
    assert!(started < created && created < completed, "{seen:?}");
}

#[test]
fn a_retrieval_event_is_attributed_to_a_retriever_rather_than_an_agent() {
    // Doc 01 section 6.3 gives retrievers their own emitter type, because
    // "which of these events came from something that reached outside the
    // profile" has to be answerable from the log directly.
    let Fixture {
        mut store,
        board,
        card,
        run,
        ..
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "web",
        sq_id: None,
    };
    repo::start_retrieval(&mut store, here, "a query").expect("started");

    let emitter: String = store
        .conn()
        .query_row(
            "SELECT emitter_type FROM event WHERE event_type = 'retrieval.started.v1'",
            [],
            |r| r.get(0),
        )
        .expect("event");
    assert_eq!(emitter, "retriever");
}

#[test]
fn a_hook_denial_names_the_category_and_never_the_item() {
    // Doc 05 section 10: the caveat names the exclusion category without
    // naming the excluded thing. An event recording the path would put the
    // secret into the log the exclusion exists to keep it out of.
    let Fixture {
        mut store,
        board,
        card,
        run,
        ..
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "local",
        sq_id: None,
    };
    repo::record_hook_denial(&mut store, here, "exclude_paths", "an excluded folder").expect("denial");

    let payload: String = store
        .conn()
        .query_row(
            "SELECT payload FROM event WHERE event_type = 'hook.denied.v1'",
            [],
            |r| r.get(0),
        )
        .expect("event");
    assert!(payload.contains("an excluded folder"));
    assert!(
        !payload.contains("Sensitive"),
        "the denial leaked what it refused"
    );
}

#[test]
fn retrieval_survives_a_replay() {
    // The projections rebuild from the log, so anything a retrieval wrote has
    // to be reconstructable or the audit trail is decorative.
    let Fixture {
        mut store,
        profile,
        board,
        card,
        run,
    } = fixture();
    let here = RetrievalRef {
        run_id: &run,
        board_id: &board,
        card_id: &card,
        retriever_id: "web",
        sq_id: None,
    };

    repo::start_retrieval(&mut store, here, "a query").expect("started");
    repo::record_retrieval(
        &mut store,
        &profile,
        here,
        &[passage("https://a.invalid/x", "Something.")],
        "full",
        7,
    )
    .expect("record");

    let before = events(&store, &board);
    let applied = store.rebuild_projections().expect("rebuild");
    assert!(applied > 0);
    assert_eq!(events(&store, &board), before, "the log changed under a replay");
}
