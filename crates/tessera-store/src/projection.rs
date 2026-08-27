//! Projections over the event log.
//!
//! Doc 10 section 3: "Run, Step, and the card's progress are projections of the
//! Event table. Any UI state can be rebuilt from events; replay is a first class
//! operation."
//!
//! The scope matters. Card *content* (answer, visual, citations) is a source of
//! truth entity in doc 01 section 3 and is written by its own repository call.
//! Card *progress* (status, confidence, which run produced it) is a projection,
//! and so are Run status and cost. Those are what [`rebuild`] restores.
//!
//! Doc 10 section 4 requires the projection to be updated in the same
//! transaction as the event write, so a crash cannot leave them apart. That is
//! enforced structurally: [`apply`] takes a `&Transaction` and is only ever
//! called from inside the append transaction.

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;

use crate::error::{Result, StoreError};

/// The parts of an event a projection needs. Keeps [`apply`] usable both from
/// the append path and from a replay over stored rows.
pub struct Projected<'a> {
    pub event_type: &'a str,
    pub payload: &'a Value,
    pub card_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub timestamp: &'a str,
}

fn field<'a>(p: &'a Value, event_type: &str, name: &'static str) -> Result<&'a str> {
    p.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| StoreError::ProjectionFieldMissing {
            event_type: event_type.to_string(),
            field: name,
        })
}

fn set_card_status(tx: &Transaction, card_id: &str, status: &str, at: &str) -> Result<()> {
    tx.execute(
        "UPDATE card SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, at, card_id],
    )?;
    Ok(())
}

/// Fold one event into the projections.
///
/// Unknown event types are not an error here: most of the vocabulary carries
/// audit meaning without moving a projection. The guard against typos lives in
/// the append path, which checks the vocabulary before the insert.
pub fn apply(tx: &Transaction, ev: &Projected<'_>) -> Result<()> {
    let p = ev.payload;

    match ev.event_type {
        // ------------------------------------------------------------- card --
        "card.requested.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            set_card_status(tx, card_id, "queued", ev.timestamp)?;
        }

        // The Router is the first agent to touch the card, so this is where it
        // starts running and where its depth and run are fixed.
        "card.routed.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            tx.execute(
                "UPDATE card SET status = 'running', updated_at = ?1,
                        depth = COALESCE(?2, depth),
                        run_id = COALESCE(?3, run_id)
                 WHERE id = ?4",
                params![
                    ev.timestamp,
                    p.get("depth_chosen").and_then(Value::as_str),
                    ev.run_id.or_else(|| p.get("run_id").and_then(Value::as_str)),
                    card_id
                ],
            )?;
        }

        // Doc 07 section B8.6: the Verifier decides card_confidence and
        // card_status, and the harness emits card.answered.v1 after it returns.
        "verify.completed.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            tx.execute(
                "UPDATE card SET confidence = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    p.get("card_confidence").and_then(Value::as_f64),
                    ev.timestamp,
                    card_id
                ],
            )?;
        }

        "card.answered.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            // Doc 01 section 4.2 says `flagged` means any open Flag exists; doc
            // 07 section B5 says it means any flag of severity warn or block.
            // The second wins (BN-015): an info flag is a chip on the header,
            // not a queue item, and "unverified" on every fast card would drown
            // the Flags queue in things nobody has to decide.
            //
            // Reading the flag table rather than trusting the payload keeps the
            // projection and the Verifier consistent on replay.
            let open: i64 = tx.query_row(
                "SELECT COUNT(*) FROM flag
                 WHERE card_id = ?1 AND status = 'open' AND severity != 'info'",
                params![card_id],
                |r| r.get(0),
            )?;
            set_card_status(
                tx,
                card_id,
                if open > 0 { "flagged" } else { "done" },
                ev.timestamp,
            )?;

            // Doc 05 section 8.5: the card records `builds_on` for every
            // own_card passage that was cited or used. It rides on this event
            // rather than on one of its own, because it is only known once the
            // Synthesizer has finished and because a projection field that no
            // event carries cannot survive a replay.
            if let Some(builds_on) = p.get("builds_on").filter(|v| v.is_array()) {
                tx.execute(
                    "UPDATE card SET builds_on = ?1 WHERE id = ?2",
                    params![builds_on.to_string(), card_id],
                )?;
            }
        }

        // A flag raised at any stage flips the card, including one raised by the
        // Router before retrieval spends anything.
        "flag.raised.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            let severity = p.get("severity").and_then(Value::as_str).unwrap_or("info");
            if severity != "info" {
                set_card_status(tx, card_id, "flagged", ev.timestamp)?;
            }
        }

        // Doc 09 section 5: dismiss reveals the content and records the decision.
        // A card whose last open flag was dismissed goes back to done.
        "review.decided.v1" => {
            if let Some(card_id) = ev.card_id.or_else(|| p.get("card_id").and_then(Value::as_str)) {
                let open: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM flag
                     WHERE card_id = ?1 AND status = 'open' AND severity != 'info'",
                    params![card_id],
                    |r| r.get(0),
                )?;
                let current: Option<String> = tx
                    .query_row("SELECT status FROM card WHERE id = ?1", params![card_id], |r| {
                        r.get(0)
                    })
                    .ok();
                // Never resurrect a failed card by clearing its flags.
                if current.as_deref() != Some("failed") {
                    set_card_status(
                        tx,
                        card_id,
                        if open > 0 { "flagged" } else { "done" },
                        ev.timestamp,
                    )?;
                }
            }
        }

        "card.failed.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            set_card_status(tx, card_id, "failed", ev.timestamp)?;
        }

        "card.superseded.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            if let Some(by) = p.get("superseded_by").and_then(Value::as_str) {
                tx.execute(
                    "UPDATE card SET supersedes = ?1, updated_at = ?2 WHERE id = ?3",
                    params![card_id, ev.timestamp, by],
                )?;
            }
        }

        // ------------------------------------------------------------- run ---
        "model.call.v1" => {
            // Doc 01 section 6.1: Run.cost is what the Profile's spend page and
            // the composer's estimate read.
            let Some(run_id) = ev.run_id.or_else(|| p.get("run_id").and_then(Value::as_str)) else {
                return Ok(());
            };
            let input = p.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
            let output = p.get("output_tokens").and_then(Value::as_i64).unwrap_or(0);
            let provider = p.get("provider").and_then(Value::as_str).unwrap_or("unknown");

            let current: Option<String> = tx
                .query_row("SELECT cost FROM run WHERE id = ?1", params![run_id], |r| {
                    r.get(0)
                })
                .ok();
            let Some(current) = current else {
                return Ok(());
            };

            let mut cost: Value = serde_json::from_str(&current)?;
            bump(&mut cost, "input_tokens", input);
            bump(&mut cost, "output_tokens", output);
            bump(&mut cost, "calls", 1);
            if let Some(by) = cost.get_mut("by_provider").and_then(Value::as_object_mut) {
                let entry = by.entry(provider.to_string()).or_insert_with(|| Value::from(0));
                let was = entry.as_i64().unwrap_or(0);
                *entry = Value::from(was + input + output);
            }
            tx.execute(
                "UPDATE run SET cost = ?1 WHERE id = ?2",
                params![serde_json::to_string(&cost)?, run_id],
            )?;
        }

        // ------------------------------------------------------- learning ---
        //
        // Doc 17 section 2.2's table, as a fold. The learning columns on
        // `concept` are projections from the day they exist: doc 17 section 9
        // ends "every mastery change is traceable to an event", and a column
        // written outside the log would be a claim about a learner that no
        // replay could check.

        // Doc 17 section 2.2: "a card that links the concept is read" moves it
        // from unseen to exposed. Exploration on ordinary boards counts, which
        // is what makes the map fill without anyone opening a lesson.
        "card.viewed.v1" => {
            let card_id = ev
                .card_id
                .map(Ok)
                .unwrap_or_else(|| field(p, ev.event_type, "card_id"))?;
            tx.execute(
                "UPDATE concept SET learning_state = 'exposed', last_evidence_at = ?1,
                        updated_at = ?1
                 WHERE (learning_state IS NULL OR learning_state = 'unseen')
                   AND id IN (SELECT concept_id FROM concept_link
                              WHERE target_type = 'card' AND target_ref = ?2)",
                params![ev.timestamp, card_id],
            )?;
        }

        // Doc 17 section 2.1: a rating is "a claim, never evidence". It sets
        // `self_rating` and moves the state to `rated`, and doc 17 section 2.4
        // keeps it away from mastery above 0.5, which is the mastery rule's
        // business rather than this fold's.
        "concept.rated.v1" => {
            let concept_id = field(p, ev.event_type, "concept_id")?;
            let rating = p.get("rating").and_then(Value::as_i64);
            tx.execute(
                "UPDATE concept SET self_rating = ?1, last_evidence_at = ?2, updated_at = ?2,
                        learning_state = CASE
                            WHEN learning_state IN ('checked', 'mastered', 'decayed') THEN learning_state
                            ELSE 'rated' END
                 WHERE id = ?3",
                params![rating, ev.timestamp, concept_id],
            )?;
        }

        // Doc 17 section 2.3's transitions, including the ones that go left: "a
        // failed check can move mastered back to checked". The event says where
        // it moved to, because only the rule that moved it knows why, and the
        // mastery it settled on rides with it so the number is traceable to an
        // event as doc 17 section 9 requires.
        "concept.state_changed.v1" => {
            let concept_id = field(p, ev.event_type, "concept_id")?;
            let to = field(p, ev.event_type, "to")?;
            tx.execute(
                "UPDATE concept SET learning_state = ?1,
                        mastery = COALESCE(?2, mastery),
                        difficulty_level = COALESCE(?3, difficulty_level),
                        last_evidence_at = ?4, updated_at = ?4
                 WHERE id = ?5",
                params![
                    to,
                    p.get("mastery").and_then(Value::as_f64),
                    p.get("difficulty_level").and_then(Value::as_i64),
                    ev.timestamp,
                    concept_id
                ],
            )?;
        }

        // Doc 17 section 2.1: "learning paths this concept belongs to". Kept as
        // a set rather than a list, because loading the same path twice is
        // loading it once.
        "path.loaded.v1" => {
            let path_id = field(p, ev.event_type, "path_id")?;
            for concept_id in p
                .get("concept_ids")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let current: Option<String> = tx
                    .query_row(
                        "SELECT path_ids FROM concept WHERE id = ?1",
                        params![concept_id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .flatten();
                let mut paths: Vec<String> = current
                    .as_deref()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_default();
                if !paths.iter().any(|p| p == path_id) {
                    paths.push(path_id.to_string());
                }
                tx.execute(
                    "UPDATE concept SET path_ids = ?1, updated_at = ?2 WHERE id = ?3",
                    params![serde_json::to_string(&paths)?, ev.timestamp, concept_id],
                )?;
            }
        }

        // Doc 01 section 4.10's rule, one layer up: an agent proposes an edge
        // and a person confirms it. A path ships its edges already confirmed,
        // so the status the edge was created with rides on the proposal event
        // and a replay does not demote them.
        "concept.edge_proposed.v1" => {
            let edge_id = field(p, ev.event_type, "edge_id")?;
            tx.execute(
                "UPDATE concept_edge SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    p.get("status").and_then(Value::as_str).unwrap_or("proposed"),
                    ev.timestamp,
                    edge_id
                ],
            )?;
        }

        "concept.edge_confirmed.v1" => {
            let edge_id = field(p, ev.event_type, "edge_id")?;
            tx.execute(
                "UPDATE concept_edge SET status = 'confirmed', updated_at = ?1 WHERE id = ?2",
                params![ev.timestamp, edge_id],
            )?;
        }

        "mission.updated.v1" => {
            let mission_id = field(p, ev.event_type, "mission_id")?;
            if let Some(status) = p.get("status").and_then(Value::as_str) {
                tx.execute(
                    "UPDATE mission SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    params![status, ev.timestamp, mission_id],
                )?;
            }
        }

        _ => {}
    }

    Ok(())
}

fn bump(cost: &mut Value, key: &str, by: i64) {
    if let Some(obj) = cost.as_object_mut() {
        let entry = obj.entry(key.to_string()).or_insert_with(|| Value::from(0));
        let was = entry.as_i64().unwrap_or(0);
        *entry = Value::from(was + by);
    }
}

/// Reset every projected column and fold the whole log back over them.
///
/// This is the replay path from doc 01 section 6 and the M1 acceptance test:
/// a scripted sequence of events must rebuild Card state identically after a
/// restart. Content columns are untouched, because they are not projections.
pub fn rebuild(tx: &Transaction) -> Result<u64> {
    // Back to the state a card is in the moment it is requested.
    tx.execute("UPDATE card SET status = 'queued', confidence = NULL", [])?;
    tx.execute(
        "UPDATE run SET cost = '{\"input_tokens\":0,\"output_tokens\":0,\"calls\":0,\"by_provider\":{}}'",
        [],
    )?;

    // Doc 17 section 2.1's learning columns, back to what a concept nobody has
    // learned anything about looks like. Null rather than `unseen`, because the
    // fold is what decides a concept has been seen and a reset that guessed
    // would survive as a fact.
    tx.execute(
        "UPDATE concept SET learning_state = NULL, self_rating = NULL, mastery = NULL,
                difficulty_level = NULL, last_evidence_at = NULL, path_ids = NULL",
        [],
    )?;
    // An edge and a mission carry a status the log moves, so they reset with
    // the rest. The row itself is content: a proposal a person made is not a
    // projection of anything, and only its state is.
    tx.execute("UPDATE concept_edge SET status = 'proposed'", [])?;
    tx.execute("UPDATE mission SET status = 'active'", [])?;

    let mut stmt = tx.prepare(
        "SELECT event_type, payload, card_id, run_id, timestamp
         FROM event ORDER BY monotonic_index ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut applied = 0u64;
    for (event_type, payload, card_id, run_id, timestamp) in &rows {
        let payload: Value = serde_json::from_str(payload)?;
        apply(
            tx,
            &Projected {
                event_type,
                payload: &payload,
                card_id: card_id.as_deref(),
                run_id: run_id.as_deref(),
                timestamp,
            },
        )?;
        applied += 1;
    }
    Ok(applied)
}
