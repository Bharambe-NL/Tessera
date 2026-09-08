//! Flags: what the Verifier raised and what the person decided about it.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::cards::CardRef;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

pub struct NewFlag<'a> {
    pub rule_id: &'a str,
    pub severity: &'a str,
    pub target: Value,
    pub reason: &'a str,
    pub evidence: Option<Value>,
}

pub fn write_flag(store: &mut Store, at: CardRef<'_>, f: NewFlag<'_>) -> Result<String> {
    let (card_id, board_id, run_id) = (at.card_id, at.board_id, at.run_id);
    let id = new_id();
    let now = now_iso8601();
    let owned = (
        id.clone(),
        card_id.to_string(),
        f.rule_id.to_string(),
        f.severity.to_string(),
        f.target.to_string(),
        f.reason.to_string(),
        f.evidence.map(|e| e.to_string()),
    );

    store.append_with(
        NewEvent::new(
            "flag.raised.v1",
            json!({
                "card_id": card_id, "rule_id": f.rule_id,
                "severity": f.severity, "reason": f.reason
            }),
            Provenance::agent("verifier", run_id),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            let (fid, card, rule, severity, target, reason, evidence) = owned;
            tx.execute(
                "INSERT INTO flag (id, card_id, rule_id, severity, target, reason, evidence, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8)",
                params![fid, card, rule, severity, target, reason, evidence, now],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// The Flags queue. Doc 09 section 6: open flags across every board on the
/// profile, severity first and then age.
///
/// `read_flags` is per card and feeds the chip on a card. This is the other
/// shape the same table is read in, and the `flag_open` index in the migration
/// was written for it.
pub fn open_flags(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT f.id, f.rule_id, f.severity, f.reason, f.evidence, f.created_at,
                c.id, c.question, c.anchor_text, c.kind,
                b.id, b.title
         FROM flag f
         JOIN card c ON c.id = f.card_id
         JOIN board b ON b.id = c.board_id
         WHERE f.status = 'open' AND b.profile_id = ?1 AND b.status = 'active'
         ORDER BY CASE f.severity WHEN 'block' THEN 0 WHEN 'warn' THEN 1 ELSE 2 END,
                  f.created_at
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            let question: String = r.get(7)?;
            let anchor: Option<String> = r.get(8)?;
            let kind: String = r.get(9)?;
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "rule_id": r.get::<_, String>(1)?,
                "severity": r.get::<_, String>(2)?,
                "reason": r.get::<_, String>(3)?,
                // Doc 09 section 6 wants an evidence preview on every row: the
                // passage excerpt or the stale date, whichever the rule wrote.
                "evidence": r.get::<_, Option<String>>(4)?
                    .and_then(|e| serde_json::from_str::<Value>(&e).ok())
                    .unwrap_or(Value::Null),
                "created_at": r.get::<_, String>(5)?,
                "card_id": r.get::<_, String>(6)?,
                // The card title as the board shows it, so a row names what the
                // reader will recognise rather than repeating the question.
                "card_title": if kind == "root" { question } else { anchor.unwrap_or(question) },
                "board_id": r.get::<_, String>(10)?,
                "board_title": r.get::<_, String>(11)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Record one decision over one or more flags. Doc 01 section 4.12.
///
/// Reviews are immutable: changing your mind inserts another Review rather than
/// editing this one, which is why the flag carries the review id and the review
/// carries the flag ids.
///
/// `None` means no open flag matched. A decision recorded over nothing would
/// leave a Review in the table that decided nothing, and the caller can say so
/// instead.
pub fn decide_flags(
    store: &mut Store,
    flag_ids: &[String],
    decision: &str,
    note: Option<&str>,
) -> Result<Option<String>> {
    // Which card each flag belongs to, read before the write so the events can
    // be grouped by card and so a flag id naming nothing open is dropped rather
    // than silently decided.
    let mut cards: Vec<(String, String)> = Vec::new();
    let mut open: Vec<String> = Vec::new();
    {
        let conn = store.conn();
        for flag_id in flag_ids {
            let found: Option<(String, String)> = conn
                .query_row(
                    "SELECT f.card_id, c.board_id FROM flag f JOIN card c ON c.id = f.card_id
                     WHERE f.id = ?1 AND f.status = 'open'",
                    params![flag_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some(pair) = found {
                cards.push(pair);
                open.push(flag_id.clone());
            }
        }
    }
    let Some((first_card, first_board)) = cards.first().cloned() else {
        return Ok(None);
    };

    let status = match decision {
        "accept" => "accepted",
        "dismiss" => "dismissed",
        // Rerun and edit leave the flag open until the rerun writes a new card,
        // so the queue does not lose a row to a decision that has not landed.
        _ => "open",
    };

    let review_id = new_id();
    let (rid, dec, n, at) = (
        review_id.clone(),
        decision.to_string(),
        note.map(str::to_string),
        now_iso8601(),
    );
    let flag_json = serde_json::to_string(&open).unwrap_or_else(|_| "[]".into());
    let row_status = status.to_string();
    let ids = open.clone();

    store.append_with(
        NewEvent::new(
            "review.decided.v1",
            json!({
                "review_id": review_id,
                "flag_ids": open,
                "decision": decision,
                "note": note,
                "card_id": first_card,
            }),
            Provenance::user(),
        )
        .on_board(&first_board)
        .on_card(&first_card),
        move |tx| {
            tx.execute(
                "INSERT INTO review (id, flag_ids, decision, note, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![rid, flag_json, dec, n, at],
            )?;
            for flag_id in &ids {
                tx.execute(
                    "UPDATE flag SET status = ?1, review_id = ?2 WHERE id = ?3 AND status = 'open'",
                    params![row_status, rid, flag_id],
                )?;
            }
            Ok(())
        },
    )?;

    // One event per card the decision touched, because the projection that
    // reopens or closes a card reads the card from the event and a bulk
    // decision can span several. The first card had its event above.
    let mut seen = std::collections::BTreeSet::from([first_card]);
    for (card_id, board_id) in &cards {
        if !seen.insert(card_id.clone()) {
            continue;
        }
        store.append(
            NewEvent::new(
                "review.decided.v1",
                json!({
                    "review_id": review_id,
                    "flag_ids": open,
                    "decision": decision,
                    "card_id": card_id,
                }),
                Provenance::user(),
            )
            .on_board(board_id)
            .on_card(card_id),
        )?;
    }

    Ok(Some(review_id))
}
