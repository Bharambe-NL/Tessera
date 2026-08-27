#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! M1 acceptance (doc 12 phase 1): "a scripted sequence of events rebuilds Card
//! state identically after a restart."
//!
//! The test writes a realistic run's worth of events, records the projected card
//! state, closes the profile, reopens it from disk, throws every projection
//! away, folds the log back over them, and asserts the state is the same.

use rusqlite::params;
use serde_json::json;
use tessera_store::{NewEvent, Provenance, Source, Store, StoreError, new_id, now_iso8601};

struct Fixture {
    root: std::path::PathBuf,
    board_id: String,
    card_a: String,
    card_b: String,
    run_a: String,
}

fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("tessera-test-{}", new_id()))
}

/// Insert the source of truth rows a run needs. These are entities, not
/// projections, so replay must leave them untouched.
fn seed(store: &Store) -> Fixture {
    let now = now_iso8601();
    let profile_id = new_id();
    let pack_id = new_id();
    let board_id = new_id();
    let card_a = new_id();
    let card_b = new_id();
    let run_a = new_id();

    let c = store.conn();
    c.execute(
        "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                    flag_rules, retrievers, exercise_templates, created_at)
         VALUES (?1, 'general', '1.0.0', '[]', '[]', '{}', '[]', '[]', '[]', ?2)",
        params![pack_id, now],
    )
    .expect("pack");
    c.execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                              retriever_config, created_at, updated_at)
         VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
        params![profile_id, pack_id, now],
    )
    .expect("profile");
    c.execute(
        "INSERT INTO board (id, profile_id, title, doctrine_pack_id, default_depth, created_at, updated_at)
         VALUES (?1, ?2, 'Capital treatment', ?3, 'deep', ?4, ?4)",
        params![board_id, profile_id, pack_id, now],
    )
    .expect("board");

    for (id, question) in [
        (&card_a, "What changed in the capital rule?"),
        (&card_b, "And for the trading book?"),
    ] {
        c.execute(
            "INSERT INTO card (id, board_id, kind, question, depth, status, created_at, updated_at)
             VALUES (?1, ?2, 'root', ?3, 'deep', 'queued', ?4, ?4)",
            params![id, board_id, question, now],
        )
        .expect("card");
    }

    c.execute(
        "INSERT INTO run (id, board_id, card_id, kind, depth, model_policy_snapshot,
                          doctrine_pack_version, status, started_at)
         VALUES (?1, ?2, ?3, 'card', 'deep', '{}', '1.0.0', 'running', ?4)",
        params![run_a, board_id, card_a, now],
    )
    .expect("run");

    Fixture {
        root: store.root().to_path_buf(),
        board_id,
        card_a,
        card_b,
        run_a,
    }
}

/// One card runs clean, the other picks up a warn flag on the way through.
fn script(store: &mut Store, f: &Fixture) {
    let agent = |id: &str| Provenance::agent(id, f.run_a.clone());

    let requested = store
        .append(
            NewEvent::new(
                "card.requested.v1",
                json!({ "question": "What changed?" }),
                Provenance::user(),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("card.requested");

    store
        .append(
            NewEvent::new(
                "card.routed.v1",
                json!({ "depth_chosen": "deep", "domain": "capital", "plan_required": true }),
                agent("router"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a)
            .caused_by(&requested.event_id),
        )
        .expect("card.routed");

    store
        .append(
            NewEvent::new(
                "model.call.v1",
                json!({ "stage": "route", "provider": "anthropic", "input_tokens": 900, "output_tokens": 240 }),
                agent("router"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("model.call");

    store
        .append(
            NewEvent::new(
                "model.call.v1",
                json!({ "stage": "synthesize", "provider": "anthropic", "input_tokens": 6100, "output_tokens": 800 }),
                agent("synthesizer"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("model.call 2");

    store
        .append(
            NewEvent::new(
                "card.synthesized.v1",
                json!({ "mode": "deep", "citation_count": 3, "unsupported_count": 0 }),
                agent("synthesizer"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("card.synthesized");

    store
        .append(
            NewEvent::new(
                "visual.produced.v1",
                json!({ "type": "table", "block_count": 9, "cited_blocks": 7, "no_claim_blocks": 2 }),
                agent("visualizer"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("visual.produced");

    store
        .append(
            NewEvent::new(
                "verify.completed.v1",
                json!({ "card_confidence": 0.82, "flag_count_by_severity": {} }),
                agent("verifier"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("verify.completed");

    store
        .append(
            NewEvent::new(
                "card.answered.v1",
                json!({}),
                Provenance::harness("harness", Some(f.run_a.clone())),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("card.answered");

    // Card B takes the flagged path. The flag row is written in the same
    // transaction as the event that announces it.
    store
        .append(
            NewEvent::new(
                "card.routed.v1",
                json!({ "depth_chosen": "fast" }),
                agent("router"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_b),
        )
        .expect("routed b");

    let flag_id = new_id();
    let card_b = f.card_b.clone();
    store
        .append_with(
            NewEvent::new(
                "flag.raised.v1",
                json!({ "rule_id": "stale_source", "severity": "warn", "stage": "verifier" }),
                agent("verifier"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_b),
            move |tx| {
                tx.execute(
                    "INSERT INTO flag (id, card_id, rule_id, severity, target, reason, status, created_at)
                     VALUES (?1, ?2, 'stale_source', 'warn', '{\"kind\":\"citation\",\"ref\":\"2\"}',
                             'The cited version was superseded.', 'open', ?3)",
                    params![flag_id, card_b, now_iso8601()],
                )?;
                Ok(())
            },
        )
        .expect("flag.raised");

    store
        .append(
            NewEvent::new(
                "verify.completed.v1",
                json!({ "card_confidence": 0.41 }),
                agent("verifier"),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_b),
        )
        .expect("verify b");

    store
        .append(
            NewEvent::new(
                "card.answered.v1",
                json!({}),
                Provenance::harness("harness", None),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_b),
        )
        .expect("answered b");
}

/// The projected state of every card, which is what replay must reproduce.
fn card_projection(store: &Store) -> Vec<(String, String, Option<f64>)> {
    let mut stmt = store
        .conn()
        .prepare("SELECT id, status, confidence FROM card ORDER BY id")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect")
}

fn run_cost(store: &Store, run_id: &str) -> serde_json::Value {
    let raw: String = store
        .conn()
        .query_row("SELECT cost FROM run WHERE id = ?1", params![run_id], |r| {
            r.get(0)
        })
        .expect("cost");
    serde_json::from_str(&raw).expect("cost json")
}

#[test]
fn events_rebuild_card_state_identically_after_a_restart() {
    let root = temp_root();

    let (before, cost_before, f) = {
        let mut store = Store::open(&root).expect("open");
        let f = seed(&store);
        script(&mut store, &f);

        let before = card_projection(&store);
        let cost_before = run_cost(&store, &f.run_a);

        // The run reached its clean end.
        assert_eq!(before.len(), 2);
        let a = before.iter().find(|c| c.0 == f.card_a).expect("card a");
        assert_eq!(a.1, "done", "a card with no open flags is done");
        assert_eq!(a.2, Some(0.82));

        let b = before.iter().find(|c| c.0 == f.card_b).expect("card b");
        assert_eq!(b.1, "flagged", "a card with an open warn flag is flagged");
        assert_eq!(b.2, Some(0.41));

        (before, cost_before, f)
    }; // store dropped: this is the restart

    // Reopen the profile from disk and throw every projection away.
    let mut store = Store::open(&f.root).expect("reopen");
    assert_eq!(
        store.schema_version().expect("version"),
        tessera_store::SCHEMA_VERSION
    );

    let events = store.event_count().expect("count");
    assert_eq!(events, 12, "the script wrote twelve events");

    let applied = store.rebuild_projections().expect("rebuild");
    assert_eq!(applied, events as u64, "replay must fold every event");

    let after = card_projection(&store);
    assert_eq!(after, before, "replay must reproduce card state exactly");
    assert_eq!(
        run_cost(&store, &f.run_a),
        cost_before,
        "replay must reproduce run cost"
    );

    // And the cost really was accumulated, not just equal to zero on both sides.
    assert_eq!(cost_before["input_tokens"], 7000);
    assert_eq!(cost_before["output_tokens"], 1040);
    assert_eq!(cost_before["calls"], 2);
    assert_eq!(cost_before["by_provider"]["anthropic"], 8040);

    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn content_survives_replay_untouched() {
    // Doc 01 section 3 makes Card an entity, not a projection. Replay resets
    // progress; it must never touch what the card says.
    let root = temp_root();
    let mut store = Store::open(&root).expect("open");
    let f = seed(&store);
    store
        .conn()
        .execute(
            "UPDATE card SET answer = 'The buffer rose to 2.5 percent.' WHERE id = ?1",
            params![f.card_a],
        )
        .expect("write answer");
    script(&mut store, &f);
    store.rebuild_projections().expect("rebuild");

    let answer: Option<String> = store
        .conn()
        .query_row("SELECT answer FROM card WHERE id = ?1", params![f.card_a], |r| {
            r.get(0)
        })
        .expect("read answer");
    assert_eq!(answer.as_deref(), Some("The buffer rose to 2.5 percent."));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_event_log_is_append_only() {
    let root = temp_root();
    let mut store = Store::open(&root).expect("open");
    let f = seed(&store);
    let ev = store
        .append(
            NewEvent::new(
                "board.created.v1",
                json!({ "title": "Capital" }),
                Provenance::user(),
            )
            .on_board(&f.board_id),
        )
        .expect("append");

    let update = store.conn().execute(
        "UPDATE event SET payload = '{}' WHERE event_id = ?1",
        params![ev.event_id],
    );
    assert!(
        update.is_err(),
        "updating an event must be refused by the database"
    );

    let delete = store
        .conn()
        .execute("DELETE FROM event WHERE event_id = ?1", params![ev.event_id]);
    assert!(
        delete.is_err(),
        "deleting an event must be refused by the database"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unknown_event_type_is_refused_before_it_is_stored() {
    let mut store = Store::open_in_memory().expect("open");
    let err = store
        .append(NewEvent::new(
            "card.reticulated.v1",
            json!({}),
            Provenance::user(),
        ))
        .expect_err("an event outside the vocabulary must not be stored");
    assert!(matches!(err, StoreError::UnknownEventType(_)));
    assert_eq!(store.event_count().expect("count"), 0);
}

#[test]
fn a_failed_side_write_rolls_back_the_event_too() {
    // Doc 10 section 4: the entity row and the event announcing it either both
    // land or neither does.
    let root = temp_root();
    let mut store = Store::open(&root).expect("open");
    let f = seed(&store);

    let result = store.append_with(
        NewEvent::new(
            "flag.raised.v1",
            json!({ "severity": "warn" }),
            Provenance::user(),
        )
        .on_board(&f.board_id)
        .on_card(&f.card_a),
        |tx| {
            // A flag against a card that does not exist. The foreign key rejects it.
            tx.execute(
                "INSERT INTO flag (id, card_id, rule_id, severity, target, reason, status, created_at)
                 VALUES (?1, 'no-such-card', 'r', 'warn', '{}', 'x', 'open', ?2)",
                params![new_id(), now_iso8601()],
            )?;
            Ok(())
        },
    );

    assert!(result.is_err(), "the side write must fail");
    assert_eq!(
        store.event_count().expect("count"),
        0,
        "and take the event with it"
    );

    let status: String = store
        .conn()
        .query_row("SELECT status FROM card WHERE id = ?1", params![f.card_a], |r| {
            r.get(0)
        })
        .expect("status");
    assert_eq!(status, "queued", "the projection must not have moved either");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_monotonic_index_has_no_gaps_and_no_repeats() {
    let mut store = Store::open_in_memory().expect("open");
    let f = seed(&store);
    for i in 0..25 {
        store
            .append(
                NewEvent::new("note.added.v1", json!({ "n": i }), Provenance::user()).on_board(&f.board_id),
            )
            .expect("append");
    }
    let indices: Vec<i64> = store
        .events(None)
        .expect("events")
        .iter()
        .map(|e| e.monotonic_index)
        .collect();
    assert_eq!(indices, (1..=25).collect::<Vec<_>>());
}

#[test]
fn a_rolled_back_append_does_not_burn_an_index() {
    let mut store = Store::open_in_memory().expect("open");
    let f = seed(&store);
    store
        .append(NewEvent::new("board.created.v1", json!({}), Provenance::user()).on_board(&f.board_id))
        .expect("first");

    let _ = store.append_with(
        NewEvent::new("note.added.v1", json!({}), Provenance::user()).on_board(&f.board_id),
        |tx| {
            tx.execute("INSERT INTO note (id) VALUES ('broken')", [])?;
            Ok(())
        },
    );

    let next = store
        .append(NewEvent::new("note.added.v1", json!({}), Provenance::user()).on_board(&f.board_id))
        .expect("third");
    assert_eq!(
        next.monotonic_index, 2,
        "the failed append must not consume an index"
    );
}

#[test]
fn test_provenance_is_preserved_through_the_log() {
    // The eval harness runs the real pipeline with test provenance (doc 02
    // section 10.1), and hooks filter on it (doc 10 section 5). It has to
    // survive the round trip.
    let mut store = Store::open_in_memory().expect("open");
    let f = seed(&store);
    store
        .append(
            NewEvent::new(
                "card.routed.v1",
                json!({ "depth_chosen": "deep" }),
                Provenance::agent("router", "r1").with_source(Source::Test),
            )
            .on_board(&f.board_id)
            .on_card(&f.card_a),
        )
        .expect("append");

    let ev = &store.events(Some(&f.board_id)).expect("events")[0];
    assert_eq!(ev.provenance.source, Source::Test);
    assert!(!ev.provenance.source.fires_policy_hooks());
}

// ---------------------------------------------------------------- learning ---

/// Every learning column on every concept, ordered, for comparing two folds.
fn concept_projection(store: &Store) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT id, learning_state, self_rating, mastery, difficulty_level, last_evidence_at,
                    path_ids FROM concept ORDER BY id",
        )
        .expect("prepare");
    stmt.query_map([], |r| {
        Ok(format!(
            "{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<f64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })
    .expect("query")
    .collect::<rusqlite::Result<Vec<_>>>()
    .expect("rows")
}

fn edge_and_mission_projection(store: &Store) -> Vec<String> {
    let c = store.conn();
    let mut out: Vec<String> = c
        .prepare("SELECT id, status FROM concept_edge ORDER BY id")
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(format!(
                    "edge {} {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                ))
            })?
            .collect()
        })
        .expect("edges");
    out.extend(
        c.prepare("SELECT id, status FROM mission ORDER BY id")
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok(format!(
                        "mission {} {}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("missions"),
    );
    out
}

#[test]
fn the_learning_columns_fold_the_same_way_twice() {
    // Doc 17 section 9: "every mastery change is traceable to an event". The
    // test of that sentence is this one: throw the columns away, fold the log
    // back over them, and land in the same place. It lands before anything
    // writes them on purpose, so the first thing that does has a replay to
    // answer to.
    let store = Store::open_in_memory().expect("store");
    let f = seed(&store);
    let mut store = store;
    let now = now_iso8601();

    let profile_id: String = store
        .conn()
        .query_row(
            "SELECT profile_id FROM board WHERE id = ?1",
            params![f.board_id],
            |r| r.get(0),
        )
        .expect("profile");
    let pack_id: String = store
        .conn()
        .query_row(
            "SELECT doctrine_pack_id FROM board WHERE id = ?1",
            params![f.board_id],
            |r| r.get(0),
        )
        .expect("pack");

    // Three concepts: one only ever read about, one rated, one checked and then
    // failed back down, which is doc 17 section 2.3's move to the left.
    let (seen, rated, checked) = (new_id(), new_id(), new_id());
    for (id, term) in [
        (&seen, "liquidity coverage ratio"),
        (&rated, "net stable funding ratio"),
        (&checked, "capital conservation buffer"),
    ] {
        store
            .conn()
            .execute(
                "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'confirmed', ?5, ?5)",
                params![id, profile_id, term, pack_id, now],
            )
            .expect("concept");
    }
    store
        .conn()
        .execute(
            "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation,
                 proposed_by, status, created_at)
             VALUES (?1, ?2, 'card', ?3, 'mentions', 'indexer', 'confirmed', ?4)",
            params![new_id(), seen, f.card_a, now],
        )
        .expect("link");

    let edge = new_id();
    store
        .conn()
        .execute(
            "INSERT INTO concept_edge (id, from_concept_id, to_concept_id, relation, proposed_by,
                 status, weight, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'prerequisite_of', 'learning_planner', 'proposed', 1.0, ?4, ?4)",
            params![edge, seen, checked, now],
        )
        .expect("edge");
    let mission = new_id();
    store
        .conn()
        .execute(
            "INSERT INTO mission (id, profile_id, statement, target_concept_ids,
                 created_at, updated_at)
             VALUES (?1, ?2, 'I sign off liquidity policies and keep guessing.', '[]', ?3, ?3)",
            params![mission, profile_id, now],
        )
        .expect("mission");

    let path = new_id();
    for event in [
        NewEvent::new(
            "card.viewed.v1",
            json!({ "card_id": f.card_a, "via": "open" }),
            Provenance::user(),
        )
        .on_board(&f.board_id)
        .on_card(&f.card_a),
        NewEvent::new(
            "concept.rated.v1",
            json!({ "concept_id": rated, "rating": 2 }),
            Provenance::user(),
        ),
        // Doc 17 section 2.4: the score moves with the evidence, so the check
        // is what writes it and the state event only says where the state went.
        NewEvent::new(
            "learn.check_answered.v1",
            json!({ "concept_ids": [checked], "level": 3, "correct": true, "repeated": false }),
            Provenance::user(),
        ),
        NewEvent::new(
            "concept.state_changed.v1",
            json!({
                "concept_id": checked, "from": "checked", "to": "mastered", "evidence": "check"
            }),
            Provenance::user(),
        ),
        // Doc 17 section 2.3: a failed check moves mastered back to checked.
        NewEvent::new(
            "learn.check_answered.v1",
            json!({ "concept_ids": [checked], "level": 3, "correct": false, "repeated": false }),
            Provenance::user(),
        ),
        NewEvent::new(
            "concept.state_changed.v1",
            json!({
                "concept_id": checked, "from": "mastered", "to": "checked",
                "evidence": "failed check"
            }),
            Provenance::user(),
        ),
        NewEvent::new(
            "path.loaded.v1",
            json!({ "path_id": path, "concept_ids": [seen, rated] }),
            Provenance::user(),
        ),
        // Loading the same path twice is loading it once.
        NewEvent::new(
            "path.loaded.v1",
            json!({ "path_id": path, "concept_ids": [seen, rated] }),
            Provenance::user(),
        ),
        NewEvent::new(
            "concept.edge_confirmed.v1",
            json!({ "edge_id": edge }),
            Provenance::user(),
        ),
        NewEvent::new(
            "mission.updated.v1",
            json!({ "mission_id": mission, "status": "done" }),
            Provenance::user(),
        ),
    ] {
        store.append(event).expect("append");
    }

    let before = concept_projection(&store);
    let before_rows = edge_and_mission_projection(&store);

    // The fold did something, so the comparison below is not two empties.
    let state_of = |id: &str| -> Option<String> {
        store
            .conn()
            .query_row(
                "SELECT learning_state FROM concept WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .expect("state")
    };
    assert_eq!(
        state_of(&seen).as_deref(),
        Some("exposed"),
        "reading a card exposes its concepts"
    );
    assert_eq!(state_of(&rated).as_deref(), Some("rated"));
    assert_eq!(
        state_of(&checked).as_deref(),
        Some("checked"),
        "a failed check moves left"
    );
    // A pass at level 3 from nothing, then a failure at the same level: doc 17
    // section 2.4's formula twice, and the number is that arithmetic rather
    // than anything an event stated.
    let score_of = |id: &str| -> Option<f64> {
        store
            .conn()
            .query_row("SELECT mastery FROM concept WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .expect("mastery")
    };
    let expected = tessera_store::mastery::after_check(
        Some(tessera_store::mastery::after_check(None, 3, true, false)),
        3,
        false,
        false,
    );
    let mastery = score_of(&checked);
    assert!(
        mastery.is_some_and(|m| (m - expected).abs() < 1e-9),
        "mastery is {mastery:?} rather than {expected}"
    );

    // Reading a card moved the exposed concept by doc 17's small step, and a
    // rating of 2 set the prior on the one with no evidence.
    assert_eq!(score_of(&seen), Some(tessera_store::mastery::EXPOSURE_STEP));
    assert_eq!(score_of(&rated), Some(0.35));
    let paths: Option<String> = store
        .conn()
        .query_row("SELECT path_ids FROM concept WHERE id = ?1", params![seen], |r| {
            r.get(0)
        })
        .expect("paths");
    assert_eq!(paths, Some(format!("[\"{path}\"]")), "the path is listed once");
    assert!(before_rows.contains(&format!("edge {edge} confirmed")));
    assert!(before_rows.contains(&format!("mission {mission} done")));

    // Throw the columns away and fold the whole log back over them.
    let applied = store.rebuild_projections().expect("rebuild");
    assert!(applied > 0, "replay folded nothing");

    assert_eq!(
        concept_projection(&store),
        before,
        "a second fold of the same log left the map somewhere else"
    );
    assert_eq!(edge_and_mission_projection(&store), before_rows);
}
