//! Learn sessions and the cards a Tutor packet carries.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::exercises::cards_for_exercise;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

// ------------------------------------------------------------ learn mode ---
//
// Doc 14 section 2's LearnSession. One per board at a time, and a board can have
// several over time, so the reads below take the newest rather than assuming one.

/// Open a session. Doc 14 section 3.3's `learn.started.v1`.
pub fn start_learn_session(store: &mut Store, board_id: &str, topic: &str) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row, board, subject, at) = (id.clone(), board_id.to_string(), topic.to_string(), now.clone());

    store.append_with(
        NewEvent::new(
            "learn.started.v1",
            json!({ "session_id": id, "board_id": board_id, "topic": topic }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "INSERT INTO learn_session (id, board_id, topic, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'intake', ?4, ?4)",
                params![row, board, subject, at],
            )?;
            // Doc 14 section 2 and doc 01: the board's mode is what the Router
            // reads, so it moves with the session rather than being inferred.
            tx.execute(
                "UPDATE board SET mode = 'learn', updated_at = ?1 WHERE id = ?2",
                params![at, board_id],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// The newest session on a board, whatever its status.
pub fn read_learn_session(store: &Store, board_id: &str) -> Result<Option<Value>> {
    // The session assembles inside the row closure rather than coming back as an
    // eight tuple. Five of the eight columns are json text that has to be parsed
    // before anyone can use it, so a tuple would only carry them as far as here.
    let parse = |s: String, fallback: Value| serde_json::from_str(&s).unwrap_or(fallback);
    let session = store
        .conn()
        .query_row(
            "SELECT id, topic, status, intake, plan, checks, opened, mastery
             FROM learn_session WHERE board_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![board_id],
            |r| {
                Ok(json!({
                    "session_id": r.get::<_, String>(0)?,
                    "board_id": board_id,
                    "topic": r.get::<_, String>(1)?,
                    "status": r.get::<_, String>(2)?,
                    "intake": parse(r.get::<_, String>(3)?, json!([])),
                    "plan": parse(r.get::<_, String>(4)?, json!([])),
                    "checks": parse(r.get::<_, String>(5)?, json!([])),
                    "opened": parse(r.get::<_, String>(6)?, json!([])),
                    "mastery": parse(r.get::<_, String>(7)?, json!({})),
                }))
            },
        )
        .optional()?;

    // Doc 17 section 2.4 moved the score onto the concept row, and the eight
    // `learn.*` shapes and the Tutor packet keep the count they always had, so
    // it is derived here where every reader passes rather than at each of them.
    let session = session.map(|mut s| {
        s["mastery"] = session_mastery(&s);
        s
    });
    Ok(session)
}

/// What one turn changed about a session, with the event that says so.
///
/// A struct rather than a column and a value, because doc 14 section 3.3 has
/// every trigger move the status and append to one list at once, and two writes
/// would let a session's status and its content disagree.
pub struct LearnUpdate<'a> {
    pub session_id: &'a str,
    pub board_id: &'a str,
    pub status: Option<&'a str>,
    /// Column name to the whole new value. Doc 14 section 2's five json columns.
    pub set: Vec<(&'a str, Value)>,
    pub event: &'a str,
    pub payload: Value,
    /// Who did this, named at every write rather than defaulted.
    pub actor: Actor<'a>,
}

/// Who a session write belongs to.
///
/// Doc 12's walkthrough asks for the right actor on every act, and a Learn
/// session is the one place where two of them take turns inside one feature:
/// the learner names a topic and answers, the tutor plans and asks.
pub enum Actor<'a> {
    Learner,
    /// Agent id and the run it decided in.
    Agent(&'a str, &'a str),
}

pub fn update_learn_session(store: &mut Store, u: LearnUpdate<'_>) -> Result<()> {
    let now = now_iso8601();
    let (id, status, at) = (
        u.session_id.to_string(),
        u.status.map(str::to_string),
        now.clone(),
    );
    // Column names come from this file and never from a caller's input, so the
    // format below cannot carry anything a caller wrote.
    let set: Vec<(String, String)> = u
        .set
        .iter()
        .filter(|(column, _)| matches!(*column, "intake" | "plan" | "checks" | "opened" | "mastery"))
        .map(|(column, value)| ((*column).to_string(), value.to_string()))
        .collect();

    let who = match u.actor {
        Actor::Learner => Provenance::user(),
        Actor::Agent(agent_id, run_id) => Provenance::agent(agent_id, run_id),
    };

    store.append_with(
        NewEvent::new(u.event, u.payload, who).on_board(u.board_id),
        move |tx| {
            for (column, value) in &set {
                tx.execute(
                    &format!("UPDATE learn_session SET {column} = ?1, updated_at = ?2 WHERE id = ?3"),
                    params![value, at, id],
                )?;
            }
            if let Some(status) = &status {
                tx.execute(
                    "UPDATE learn_session SET status = ?1, updated_at = ?2 WHERE id = ?3",
                    params![status, at, id],
                )?;
            }
            Ok(())
        },
    )?;
    Ok(())
}

/// Doc 14 section 3.6's mastery, derived from the session's own checks.
///
/// The count is what it always was: plus one for a correct check on a concept,
/// minus one for a wrong one, floored at zero, because a score below zero would
/// say something about a learner that counting cannot support. What changed is
/// where it comes from. Doc 17 section 2.4 moves mastery onto the concept row
/// as a score across every session, and a second number under the same name
/// beside it would be two answers to one question; so the session's number is
/// computed from the checks it already records rather than stored again.
///
/// A session written before the checks carried their concepts falls back to the
/// column, which is why the column stays. Doc 17's history is a transcript and
/// is never backfilled: what an old session recorded is what it recorded.
pub fn session_mastery(session: &Value) -> Value {
    let checks = session["checks"].as_array().cloned().unwrap_or_default();
    let derivable = checks
        .iter()
        .any(|c| c["concept_ids"].as_array().is_some_and(|ids| !ids.is_empty()));
    if !derivable {
        return session["mastery"].clone();
    }

    let mut map = serde_json::Map::new();
    for check in &checks {
        let correct = check["correct"].as_bool().unwrap_or(false);
        for id in check["concept_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let now = map.get(id).and_then(Value::as_i64).unwrap_or(0);
            let next = if correct { now + 1 } else { (now - 1).max(0) };
            map.insert(id.to_string(), json!(next));
        }
    }
    Value::Object(map)
}

/// The cards a Tutor packet carries. Doc 14 section 3.2.
///
/// Question, answer, visual labels and citations. Never the passages behind
/// them: doc 14 section 3.1 keeps retrieval out of the Tutor's scope, and a
/// packet carrying a passage would be the first step towards it writing content.
pub fn cards_for_tutor(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    Ok(tutor_shape(cards_for_exercise(store, board_id, 12)?))
}

/// Doc 17 section 4's second source: verified cards anywhere on the map.
///
/// Reached only when the lesson board has none. The order is the doc's: the
/// lesson board first, then the map, and a card the learner has never seen is
/// still a card the Verifier stood behind, which is what the rule is about.
/// `exclude` is the lesson board, so a card is never offered twice.
pub fn cards_for_concepts(
    store: &Store,
    concept_ids: &[String],
    exclude_board_id: &str,
    limit: i64,
) -> Result<Vec<Value>> {
    if concept_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = concept_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT DISTINCT l.target_ref
         FROM concept_link l
         JOIN card c ON c.id = l.target_ref
         WHERE l.target_type = 'card'
           AND l.status = 'confirmed'
           AND l.concept_id IN ({placeholders})
           AND c.board_id <> ?
           AND c.status IN ('done', 'flagged')
           AND c.answer IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM flag f
             WHERE f.card_id = c.id AND f.status = 'open' AND f.severity = 'block'
           )
         ORDER BY c.created_at
         LIMIT ?",
    );

    let conn = store.conn();
    let mut stmt = conn.prepare(&sql)?;
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    for id in concept_ids {
        binds.push(Box::new(id.clone()));
    }
    binds.push(Box::new(exclude_board_id.to_string()));
    binds.push(Box::new(limit));
    let ids: Vec<String> = stmt
        .query_map(
            rusqlite::params_from_iter(binds.iter().map(|b| b.as_ref())),
            |r| r.get(0),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut out = Vec::new();
    for card_id in ids {
        out.extend(cards_by_id(store, &card_id)?);
    }
    Ok(tutor_shape(out))
}

/// One card in the exercise packet's shape, by id.
fn cards_by_id(store: &Store, card_id: &str) -> Result<Vec<Value>> {
    let board_id: Option<String> = store
        .conn()
        .query_row("SELECT board_id FROM card WHERE id = ?1", params![card_id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(board_id) = board_id else {
        return Ok(Vec::new());
    };
    Ok(cards_for_exercise(store, &board_id, 32)?
        .into_iter()
        .filter(|c| c["card_id"].as_str() == Some(card_id))
        .collect())
}

/// The tutor packet's flat card shape. Doc 14 section 3.4.
fn tutor_shape(cards: Vec<Value>) -> Vec<Value> {
    cards
        .into_iter()
        .map(|c| {
            let labels: Vec<Value> = c["visual"]["block_index"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b["label"].as_str().map(|l| json!(l)))
                .collect();
            json!({
                "card_id": c["card_id"],
                "question": c["question"],
                "answer": c["answer"],
                "visual_labels": labels,
                "citations": c["citations"],
            })
        })
        .collect()
}
