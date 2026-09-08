//! Cards: the question, the run that answers it, and what the answer carries.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::sources::normalise_locator;
use super::sql::{parse_json, read_citations, read_flags};
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

// ----------------------------------------------------------------- writes --

pub struct NewCard<'a> {
    pub board_id: &'a str,
    pub parent_card_id: Option<&'a str>,
    pub kind: &'a str,
    pub question: &'a str,
    pub depth: &'a str,
    pub anchor_text: Option<&'a str>,
    pub anchor_block_ref: Option<&'a str>,
    pub audience_id: Option<&'a str>,
}

/// Doc 03 section 3: `card.requested.v1` is what wakes the Router.
pub fn create_card(store: &mut Store, c: NewCard<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row_id, board, parent, kind, question, depth) = (
        id.clone(),
        c.board_id.to_string(),
        c.parent_card_id.map(str::to_string),
        c.kind.to_string(),
        c.question.to_string(),
        c.depth.to_string(),
    );
    let (anchor, block_ref, audience) = (
        c.anchor_text.map(str::to_string),
        c.anchor_block_ref.map(str::to_string),
        c.audience_id.map(str::to_string),
    );

    store.append_with(
        NewEvent::new(
            "card.requested.v1",
            json!({
                "card_id": id, "kind": c.kind, "question": c.question,
                "depth": c.depth, "parent_card_id": c.parent_card_id,
                "anchor_text": c.anchor_text
            }),
            Provenance::user(),
        )
        .on_board(c.board_id)
        .on_card(&id),
        move |tx| {
            tx.execute(
                "INSERT INTO card (id, board_id, parent_card_id, kind, anchor_text, anchor_block_ref,
                                   question, depth, audience_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued', ?10, ?10)",
                params![
                    row_id, board, parent, kind, anchor, block_ref, question, depth, audience, now
                ],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// Open a Run and snapshot the resolved policy onto it. Doc 01 section 6.1.
pub struct NewRun<'a> {
    pub board_id: &'a str,
    pub card_id: Option<&'a str>,
    pub kind: &'a str,
    pub depth: Option<&'a str>,
    pub policy_snapshot: &'a Value,
    pub pack_version: &'a str,
}

pub fn start_run(store: &Store, r: NewRun<'_>) -> Result<String> {
    let NewRun {
        board_id,
        card_id,
        kind,
        depth,
        policy_snapshot,
        pack_version,
    } = r;
    let id = new_id();
    store.conn().execute(
        "INSERT INTO run (id, board_id, card_id, kind, depth, model_policy_snapshot,
                          doctrine_pack_version, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8)",
        params![
            id,
            board_id,
            card_id,
            kind,
            depth,
            policy_snapshot.to_string(),
            pack_version,
            now_iso8601()
        ],
    )?;
    Ok(id)
}

/// The Synthesizer's result. Doc 06 section A7.
/// Which card, on which board, produced by which run.
#[derive(Clone, Copy)]
pub struct CardRef<'a> {
    pub card_id: &'a str,
    pub board_id: &'a str,
    pub run_id: &'a str,
}

pub fn write_answer(
    store: &mut Store,
    at: CardRef<'_>,
    answer: &str,
    findings: &Value,
    produced_by: &Value,
    payload: Value,
) -> Result<()> {
    let (card_id, board_id, run_id) = (at.card_id, at.board_id, at.run_id);
    let (card, answer_text, findings_json, produced, run) = (
        card_id.to_string(),
        answer.to_string(),
        findings.to_string(),
        produced_by.to_string(),
        run_id.to_string(),
    );
    let now = now_iso8601();

    store.append_with(
        NewEvent::new(
            "card.synthesized.v1",
            payload,
            Provenance::agent("synthesizer", run_id),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            tx.execute(
                "UPDATE card SET answer = ?1, findings = ?2, produced_by = ?3, run_id = ?4, updated_at = ?5
                 WHERE id = ?6",
                params![answer_text, findings_json, produced, run, now, card],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// The Visualizer's result. Doc 06 section B7.
pub fn write_visual(
    store: &mut Store,
    at: CardRef<'_>,
    visual_type: &str,
    title: &str,
    payload: &Value,
    block_index: &Value,
    produced_by: &Value,
) -> Result<String> {
    let (card_id, board_id, run_id) = (at.card_id, at.board_id, at.run_id);
    let id = new_id();
    let now = now_iso8601();
    let (vid, card, vtype, vtitle) = (
        id.clone(),
        card_id.to_string(),
        visual_type.to_string(),
        title.to_string(),
    );
    let (payload_json, blocks_json, produced) = (
        payload.to_string(),
        block_index.to_string(),
        produced_by.to_string(),
    );

    let block_count = block_index.as_array().map_or(0, Vec::len);
    let cited = block_index.as_array().map_or(0, |b| {
        b.iter()
            .filter(|e| {
                e.get("citation_ordinals")
                    .and_then(Value::as_array)
                    .is_some_and(|c| !c.is_empty())
            })
            .count()
    });

    store.append_with(
        NewEvent::new(
            "visual.produced.v1",
            json!({
                "card_id": card_id, "type": visual_type, "block_count": block_count,
                "cited_blocks": cited, "no_claim_blocks": block_count - cited
            }),
            Provenance::agent("visualizer", run_id),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            tx.execute(
                "INSERT INTO visual (id, card_id, type, title, payload, block_index, produced_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![vid, card, vtype, vtitle, payload_json, blocks_json, produced, now],
            )?;
            tx.execute("UPDATE card SET visual_id = ?1 WHERE id = ?2", params![vid, card])?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// A source, a passage and the citation binding them to a claim. In one
/// transaction because a citation without its passage is not an audit trail.
pub struct NewCitation<'a> {
    pub ordinal: i64,
    pub source_title: &'a str,
    pub source_class: &'a str,
    pub locator: &'a str,
    pub issuer: Option<&'a str>,
    pub freshness_class: &'a str,
    pub trust_rank: i64,
    pub passage_text: &'a str,
    pub claim_span: Value,
    pub binding: &'a str,
}

pub fn write_citation(
    store: &mut Store,
    profile_id: &str,
    at: CardRef<'_>,
    c: NewCitation<'_>,
) -> Result<String> {
    let (card_id, board_id, run_id) = (at.card_id, at.board_id, at.run_id);
    let (source_id, passage_id, citation_id) = (new_id(), new_id(), new_id());
    let now = now_iso8601();
    let dedupe = normalise_locator(c.locator);

    let owned = (
        source_id.clone(),
        passage_id.clone(),
        citation_id.clone(),
        profile_id.to_string(),
        card_id.to_string(),
        run_id.to_string(),
        c.source_title.to_string(),
        c.source_class.to_string(),
        c.locator.to_string(),
        c.issuer.map(str::to_string),
        c.freshness_class.to_string(),
        c.passage_text.to_string(),
        c.claim_span.to_string(),
        c.binding.to_string(),
        dedupe,
    );
    let (ordinal, trust_rank) = (c.ordinal, c.trust_rank);

    store.append_with(
        NewEvent::new(
            "citation.bound.v1",
            json!({ "card_id": card_id, "ordinal": ordinal, "source_class": c.source_class }),
            Provenance::agent("synthesizer", run_id),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            let (sid, pid, cid, profile, card, run, title, class, locator, issuer, freshness, text, span, binding, dedupe) = owned;

            // Doc 01 section 4.7: two retrievals of the same page yield one Source.
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM source WHERE profile_id = ?1 AND dedupe_key = ?2",
                    params![profile, dedupe],
                    |r| r.get(0),
                )
                .optional()?;

            let source_id = match existing {
                Some(id) => id,
                None => {
                    tx.execute(
                        "INSERT INTO source (id, profile_id, class, title, locator, site_or_issuer,
                                             retrieved_at, freshness_class, trust_rank, dedupe_key, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?7)",
                        params![sid, profile, class, title, locator, issuer, now, freshness, trust_rank, dedupe],
                    )?;
                    sid
                }
            };

            tx.execute(
                "INSERT INTO passage (id, source_id, text, retrieved_in_run, retrieved_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'synthesizer', ?5)",
                params![pid, source_id, text, run, now],
            )?;
            tx.execute(
                "INSERT INTO citation (id, card_id, ordinal, passage_id, claim_span, binding, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![cid, card, ordinal, pid, span, binding, now],
            )?;
            Ok(())
        },
    )?;
    Ok(citation_id)
}

/// Record the Verifier's verdicts and confidence, then answer the card.
/// Doc 07 section B7: `card.answered.v1` is emitted by the harness after the
/// Verifier returns.
///
/// `builds_on` is the prior cards this card was built from, doc 05 section 8.5.
/// It is a parameter rather than something read back out of the store because
/// only the pipeline knows which recalled passages the Synthesizer actually
/// used, and doc 15 section 2 makes that distinction load bearing.
pub fn finish_card(
    store: &mut Store,
    at: CardRef<'_>,
    confidence: f64,
    verdicts: &[(i64, String)],
    checks_run: &Value,
    builds_on: &[Value],
) -> Result<()> {
    let (card_id, board_id, run_id) = (at.card_id, at.board_id, at.run_id);
    let card = card_id.to_string();
    let verdict_rows: Vec<(i64, String)> = verdicts.to_vec();

    store.append_with(
        NewEvent::new(
            "verify.completed.v1",
            json!({
                "card_id": card_id,
                "card_confidence": confidence,
                "checks_run": checks_run,
                "verdict_counts": count_verdicts(verdicts)
            }),
            Provenance::agent("verifier", run_id).with_trust(crate::event::TrustLevel::Verified),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            for (ordinal, verdict) in &verdict_rows {
                tx.execute(
                    "UPDATE citation SET verifier_verdict = ?1 WHERE card_id = ?2 AND ordinal = ?3",
                    params![verdict, card, ordinal],
                )?;
            }
            Ok(())
        },
    )?;

    // The status comes from the flag table rather than the caller, so the two
    // cannot disagree. The projection does the same on replay.
    let open: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM flag WHERE card_id = ?1 AND status = 'open' AND severity != 'info'",
        params![card_id],
        |r| r.get(0),
    )?;
    let status = if open > 0 { "flagged" } else { "done" };

    store.append(
        NewEvent::new(
            "card.answered.v1",
            json!({
                "card_id": card_id,
                "status": status,
                "card_confidence": confidence,
                "builds_on": builds_on
            }),
            Provenance::harness("harness", Some(run_id.to_string())),
        )
        .on_board(board_id)
        .on_card(card_id),
    )?;
    Ok(())
}

fn count_verdicts(verdicts: &[(i64, String)]) -> Value {
    let mut counts = serde_json::Map::new();
    for (_, v) in verdicts {
        let entry = counts.entry(v.clone()).or_insert_with(|| json!(0));
        *entry = json!(entry.as_i64().unwrap_or(0) + 1);
    }
    Value::Object(counts)
}

pub fn fail_card(store: &mut Store, card_id: &str, board_id: &str, failure: &Value) -> Result<()> {
    store.append(
        NewEvent::new(
            "card.failed.v1",
            json!({ "card_id": card_id, "failure": failure }),
            Provenance::harness("harness", None),
        )
        .on_board(board_id)
        .on_card(card_id),
    )?;
    Ok(())
}

/// Note a run's outcome on its row. The event log already carries the detail.
pub fn end_run(store: &Store, run_id: &str, status: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE run SET status = ?1, ended_at = ?2 WHERE id = ?3",
        params![status, now_iso8601(), run_id],
    )?;
    Ok(())
}

// ------------------------------------------------------------------ reads --

/// What the canvas renders. Mirrors `app/ui/src/canvas/types.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardView {
    pub id: String,
    pub parent_card_id: Option<String>,
    pub kind: String,
    pub anchor_text: Option<String>,
    pub anchor_block_ref: Option<String>,
    pub question: String,
    pub depth: String,
    pub audience_id: Option<String>,
    pub answer: Option<String>,
    pub findings: Vec<Value>,
    pub visual: Option<Value>,
    pub citations: Vec<Value>,
    pub flags: Vec<Value>,
    pub status: String,
    pub confidence: Option<f64>,
    /// Doc 01 section 4.4. Prior verified cards this one was built from, as
    /// {board_id, card_id, verified_at}. Context, never evidence: doc 15
    /// section 2. Empty on every card that used no prior work.
    pub builds_on: Vec<Value>,
    pub model_alias: Option<String>,
    pub stages: Vec<Value>,
    pub position: Value,
    /// Doc 16 section 4: the page this card was saved as, which the card header
    /// shows as a chip. `None` on every card nobody has saved.
    pub page_id: Option<String>,
}

pub fn read_cards(store: &Store, board_id: &str) -> Result<Vec<CardView>> {
    let conn = store.conn();
    // A rerun inserts a new row pointing at the old one, so the board shows the
    // head of each chain: a card nothing supersedes.
    let mut stmt = conn.prepare(
        "SELECT c.id, c.parent_card_id, c.kind, c.anchor_text, c.anchor_block_ref, c.question,
                c.depth, c.audience_id, c.answer, c.findings, c.status, c.confidence,
                c.produced_by, c.position, c.visual_id, c.builds_on, c.page_id
         FROM card c
         WHERE c.board_id = ?1
           AND NOT EXISTS (SELECT 1 FROM card newer WHERE newer.supersedes = c.id)
         ORDER BY c.created_at ASC",
    )?;

    let rows: Vec<(CardView, Option<String>)> = stmt
        .query_map(params![board_id], |r| {
            let produced_by: Option<String> = r.get(12)?;
            Ok((
                CardView {
                    id: r.get(0)?,
                    parent_card_id: r.get(1)?,
                    kind: r.get(2)?,
                    anchor_text: r.get(3)?,
                    anchor_block_ref: r.get(4)?,
                    question: r.get(5)?,
                    depth: r.get(6)?,
                    audience_id: r.get(7)?,
                    answer: r.get(8)?,
                    findings: r
                        .get::<_, Option<String>>(9)?
                        .map(|f| parse_json(&f))
                        .and_then(|v| v.as_array().cloned())
                        .unwrap_or_default(),
                    visual: None,
                    citations: Vec::new(),
                    flags: Vec::new(),
                    status: r.get(10)?,
                    confidence: r.get(11)?,
                    builds_on: parse_json(&r.get::<_, String>(15)?)
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                    model_alias: produced_by
                        .as_deref()
                        .map(parse_json)
                        .and_then(|v| v.get("model_alias").and_then(Value::as_str).map(str::to_string)),
                    stages: Vec::new(),
                    position: parse_json(&r.get::<_, String>(13)?),
                    page_id: r.get(16)?,
                },
                r.get(14)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::with_capacity(rows.len());
    for (mut card, visual_id) in rows {
        if let Some(vid) = visual_id {
            card.visual = read_visual(store, &vid)?;
        }
        card.citations = read_citations(store, &card.id)?;
        card.flags = read_flags(store, &card.id)?;
        out.push(card);
    }
    Ok(out)
}

/// Put a card where the person dropped it.
///
/// Doc 01 section 4.2's `position` is a layout slot plus a user offset, and the
/// offset is the half a person writes. `pinned` is what tells the layout to
/// leave the card alone. The whole object goes in one write because `x` and
/// `dx` have to agree: a partial write would leave the card somewhere neither
/// the person nor the layout chose.
///
/// The row is checked inside the transaction rather than before it. A move that
/// names no card is a caller's bug, and writing the event without the row would
/// leave an audit trail for something that never happened.
pub fn move_card(store: &mut Store, board_id: &str, card_id: &str, position: &Value) -> Result<()> {
    let (id, place, now) = (card_id.to_string(), position.to_string(), now_iso8601());
    let pinned = position.get("pinned").and_then(Value::as_bool).unwrap_or(false);
    store.append_with(
        NewEvent::new(
            "card.moved.v1",
            json!({ "card_id": card_id, "pinned": pinned }),
            Provenance::user(),
        )
        .on_board(board_id)
        .on_card(card_id),
        move |tx| {
            let rows = tx.execute(
                "UPDATE card SET position = ?1, updated_at = ?2 WHERE id = ?3",
                params![place, now, id],
            )?;
            if rows == 0 {
                return Err(crate::error::StoreError::CardMissing(id));
            }
            Ok(())
        },
    )?;
    Ok(())
}

fn read_visual(store: &Store, visual_id: &str) -> Result<Option<Value>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT id, type, title, payload, block_index FROM visual WHERE id = ?1",
            params![visual_id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "type": r.get::<_, String>(1)?,
                    "title": r.get::<_, String>(2)?,
                    "payload": parse_json(&r.get::<_, String>(3)?),
                    "block_index": parse_json(&r.get::<_, String>(4)?),
                }))
            },
        )
        .optional()?)
}
