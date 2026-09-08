//! Exercises, the attempts against them, and the cards they draw from.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

/// The cards an exercise may draw from. Doc 08 section 2.
///
/// "Cards (status done or flagged with only warn flags; blocked content is
/// excluded)". A blocked card is one the Verifier held back, and a question
/// whose answer is held back is a question with no right answer.
pub fn cards_for_exercise(store: &Store, board_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.question, c.answer, c.findings, c.visual_id
         FROM card c
         WHERE c.board_id = ?1
           AND c.status IN ('done', 'flagged')
           AND c.answer IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM flag f
             WHERE f.card_id = c.id AND f.status = 'open' AND f.severity = 'block'
           )
         ORDER BY c.created_at
         LIMIT ?2",
    )?;

    /// One card row, before its visual and citations are read.
    type CardRow = (String, String, String, Option<String>, Option<String>);
    let rows: Vec<CardRow> = stmt
        .query_map(params![board_id, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for (card_id, question, answer, findings, visual_id) in rows {
        // Doc 08 section 4 carries the visual's type and block index, never its
        // payload: the payload is what a visual draws, the labels are what an
        // item can ask about.
        let visual: Value = match visual_id {
            Some(id) => conn
                .query_row(
                    "SELECT type, block_index FROM visual WHERE id = ?1",
                    params![id],
                    |r| {
                        let t: String = r.get(0)?;
                        let blocks: String = r.get(1)?;
                        Ok(json!({
                            "type": t,
                            "block_index": serde_json::from_str::<Value>(&blocks)
                                .unwrap_or_else(|_| json!([])),
                        }))
                    },
                )
                .optional()?
                .unwrap_or(Value::Null),
            None => Value::Null,
        };

        let mut citations = conn.prepare(
            "SELECT c.ordinal, s.title FROM citation c
             JOIN passage p ON p.id = c.passage_id
             JOIN source s ON s.id = p.source_id
             WHERE c.card_id = ?1
             ORDER BY c.ordinal",
        )?;
        let cites: Vec<Value> = citations
            .query_map(params![card_id], |r| {
                Ok(json!({ "n": r.get::<_, i64>(0)?, "source_title": r.get::<_, String>(1)? }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        out.push(json!({
            "card_id": card_id,
            "question": question,
            "answer": answer,
            "findings": findings
                .and_then(|f| serde_json::from_str::<Value>(&f).ok())
                .unwrap_or_else(|| json!([])),
            "visual": visual,
            "citations": cites,
        }));
    }
    Ok(out)
}

/// Write one Exercise. Doc 08 section 7's `exercise.generated.v1`.
/// Everything one exercise row holds. Doc 08 section 5's output, plus where it
/// came from.
pub struct NewExercise<'a> {
    pub board_id: &'a str,
    pub run_id: &'a str,
    pub template_id: &'a str,
    pub audience_id: Option<&'a str>,
    pub scope: &'a [String],
    pub items: &'a Value,
    pub produced_by: &'a Value,
}

pub fn write_exercise(store: &mut Store, e: NewExercise<'_>) -> Result<String> {
    let NewExercise {
        board_id,
        run_id,
        template_id,
        audience_id,
        scope,
        items,
        produced_by,
    } = e;
    let id = new_id();
    let kinds: Vec<&str> = items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["kind"].as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let (row, board, scope_json, template, audience, items_json, produced, now) = (
        id.clone(),
        board_id.to_string(),
        serde_json::to_string(scope).unwrap_or_else(|_| "[]".into()),
        template_id.to_string(),
        audience_id.map(str::to_string),
        items.to_string(),
        produced_by.to_string(),
        now_iso8601(),
    );

    store.append_with(
        NewEvent::new(
            "exercise.generated.v1",
            json!({
                "exercise_id": id,
                "board_id": board_id,
                "item_count": items.as_array().map(Vec::len).unwrap_or(0),
                "kinds": kinds,
                "audience_id": audience_id,
            }),
            Provenance::agent("exercise", run_id.to_string()),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "INSERT INTO exercise (id, board_id, scope, template_id, audience_id, items,
                                       produced_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    row, board, scope_json, template, audience, items_json, produced, now
                ],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// Record one attempt. Doc 08 section 7: `attempt.recorded.v1` comes from the
/// UI, because grading a multiple choice answer needs no agent.
///
/// Attempts stay local to the profile and are excluded from bundles by default,
/// which the initial migration already says in a comment: what a reader got
/// wrong is theirs.
pub fn record_attempt(store: &mut Store, exercise_id: &str, answers: &Value) -> Result<(String, i64, i64)> {
    let (items, board_id): (String, String) = store.conn().query_row(
        "SELECT items, board_id FROM exercise WHERE id = ?1",
        params![exercise_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let items: Value = serde_json::from_str(&items).unwrap_or_else(|_| json!([]));

    // Graded here rather than trusted from the caller, so a score is a fact
    // about the exercise rather than a number the shell sent.
    let total = items.as_array().map(Vec::len).unwrap_or(0) as i64;
    let mut correct = 0i64;
    for item in items.as_array().into_iter().flatten() {
        let Some(id) = item["id"].as_str() else { continue };
        if answers[id].as_str() == item["answer_id"].as_str() {
            correct += 1;
        }
    }

    let id = new_id();
    let score = json!({ "correct": correct, "total": total });
    let (row, ex, answers_json, score_json, now) = (
        id.clone(),
        exercise_id.to_string(),
        answers.to_string(),
        score.to_string(),
        now_iso8601(),
    );

    store.append_with(
        NewEvent::new(
            "attempt.recorded.v1",
            json!({
                "attempt_id": id,
                "exercise_id": exercise_id,
                "correct": correct,
                "total": total,
            }),
            Provenance::user(),
        )
        .on_board(&board_id),
        move |tx| {
            tx.execute(
                "INSERT INTO attempt (id, exercise_id, answers, score, taken_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![row, ex, answers_json, score_json, now],
            )?;
            Ok(())
        },
    )?;
    Ok((id, correct, total))
}

/// Doc 08 section 11: a wrong item is reported by the user from the card, and
/// the report feeds pack maintenance rather than changing the exercise.
pub fn report_exercise_item(
    store: &mut Store,
    exercise_id: &str,
    item_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    let board_id: String = store.conn().query_row(
        "SELECT board_id FROM exercise WHERE id = ?1",
        params![exercise_id],
        |r| r.get(0),
    )?;
    store.append(
        NewEvent::new(
            "exercise.item_reported.v1",
            json!({
                "exercise_id": exercise_id,
                "item_id": item_id,
                "reason": reason,
            }),
            Provenance::user(),
        )
        .on_board(&board_id),
    )?;
    Ok(())
}

/// The exercises a board holds, newest first, with the last attempt on each.
pub fn list_exercises(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT e.id, e.items, e.template_id, e.audience_id, e.created_at,
                (SELECT a.score FROM attempt a WHERE a.exercise_id = e.id
                 ORDER BY a.taken_at DESC LIMIT 1) AS last_score
         FROM exercise e WHERE e.board_id = ?1
         ORDER BY e.created_at DESC",
    )?;
    Ok(stmt
        .query_map(params![board_id], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "items": serde_json::from_str::<Value>(&r.get::<_, String>(1)?)
                    .unwrap_or_else(|_| json!([])),
                "template_id": r.get::<_, String>(2)?,
                "audience_id": r.get::<_, Option<String>>(3)?,
                "created_at": r.get::<_, String>(4)?,
                "last_score": r.get::<_, Option<String>>(5)?
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .unwrap_or(Value::Null),
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
