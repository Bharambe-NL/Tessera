//! Entity writes, each carrying the event that announces it.
//!
//! Every function here goes through [`Store::append_with`], so the row and its
//! event land in one transaction (doc 10 section 4). There is no path that
//! writes an entity without an event, which is what makes board history complete
//! rather than best effort.
//!
//! The read side returns the shapes the canvas renders. They mirror doc 01's
//! field names exactly, so a name never means two things across the RPC
//! boundary.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::Result;
use crate::event::{NewEvent, Provenance, Source};
use crate::{Store, new_id, now_iso8601};

// ----------------------------------------------------------------- writes --

pub struct NewBoard<'a> {
    pub profile_id: &'a str,
    pub title: &'a str,
    pub doctrine_pack_id: &'a str,
    pub default_depth: &'a str,
    pub named_by_user: bool,
    pub parent_board_id: Option<&'a str>,
    pub seed_label: Option<&'a str>,
    pub context: Option<&'a str>,
}

pub fn create_board(store: &mut Store, b: NewBoard<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row_id, title, pack, depth) = (
        id.clone(),
        b.title.to_string(),
        b.doctrine_pack_id.to_string(),
        b.default_depth.to_string(),
    );
    let (profile, named, parent, seed, context) = (
        b.profile_id.to_string(),
        b.named_by_user,
        b.parent_board_id.map(str::to_string),
        b.seed_label.map(str::to_string),
        b.context.map(str::to_string),
    );

    store.append_with(
        NewEvent::new(
            "board.created.v1",
            json!({ "board_id": id, "title": b.title, "doctrine_pack_id": b.doctrine_pack_id }),
            Provenance::user(),
        )
        .on_board(&id),
        move |tx| {
            tx.execute(
                "INSERT INTO board (id, profile_id, title, named_by_user, doctrine_pack_id, context,
                                    seed_label, parent_board_id, default_depth, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    row_id,
                    profile,
                    title,
                    named as i64,
                    pack,
                    context,
                    seed,
                    parent,
                    depth,
                    now
                ],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

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

/// Doc 01 section 4.7: the dedupe key is a normalised locator.
/// The key that makes two retrievals of one thing a single Source.
///
/// Doc 01 section 4.8 keys `source` uniqueness on this and doc 05 section 12
/// wants zero duplicate sources for mirrored pages. Public and living here on
/// purpose: this crate owns the uniqueness constraint, and a second copy of the
/// rule in the retrievers would drift from it the first time either changed.
///
/// Scheme, case, a leading `www.`, a trailing slash, a query string and a
/// fragment are all noise. A tracking parameter is the common way one page
/// arrives four times.
pub fn normalise_locator(locator: &str) -> String {
    let lower = locator.trim().to_lowercase().replace('\\', "/");
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let without_www = without_scheme.strip_prefix("www.").unwrap_or(without_scheme);
    let without_query = without_www.split(['?', '#']).next().unwrap_or(without_www);
    without_query.trim_end_matches('/').to_string()
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardView {
    pub id: String,
    pub title: String,
    pub named_by_user: bool,
    pub doctrine_pack: Value,
    pub default_depth: String,
    pub mode: String,
    pub parent_board_id: Option<String>,
    pub seed_label: Option<String>,
    pub viewport: Value,
    pub cards: Vec<CardView>,
    /// Doc 01 section 4.5's stickies, which hang off the board rather than off
    /// a card even when they quote one.
    pub notes: Vec<NoteView>,
}

pub fn read_board(store: &Store, board_id: &str) -> Result<Option<BoardView>> {
    let conn = store.conn();
    let board = conn
        .query_row(
            "SELECT b.id, b.title, b.named_by_user, p.code, p.version, b.default_depth, b.mode,
                    b.parent_board_id, b.seed_label, b.viewport
             FROM board b JOIN doctrine_pack p ON p.id = b.doctrine_pack_id
             WHERE b.id = ?1",
            params![board_id],
            |r| {
                Ok(BoardView {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    named_by_user: r.get::<_, i64>(2)? != 0,
                    doctrine_pack: json!({ "code": r.get::<_, String>(3)?, "version": r.get::<_, String>(4)? }),
                    default_depth: r.get(5)?,
                    mode: r.get(6)?,
                    parent_board_id: r.get(7)?,
                    seed_label: r.get(8)?,
                    viewport: parse_json(&r.get::<_, String>(9)?),
                    cards: Vec::new(),
                    notes: Vec::new(),
                })
            },
        )
        .optional()?;

    let Some(mut board) = board else { return Ok(None) };
    board.cards = read_cards(store, board_id)?;
    board.notes = read_notes(store, board_id)?;
    Ok(Some(board))
}

fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
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

/// One card in a follow-up's ancestry, in the shape both packets want.
///
/// Doc 03 section 4 gives the Router the immediate parent; doc 04 section 4
/// gives the Planner up to three ancestors. Both read the same rows, so they
/// read them through one query.
#[derive(Debug, Clone)]
pub struct Ancestor {
    pub card_id: String,
    pub question: String,
    pub answer: Option<String>,
    pub depth: String,
    pub confidence: Option<f64>,
    pub answered_at: Option<String>,
    pub citations: Vec<Value>,
}

impl Ancestor {
    pub fn stale_citations(&self) -> usize {
        self.citations
            .iter()
            .filter(|c| c["stale"] == json!(true))
            .count()
    }
}

/// Walk up from a card through `parent_card_id`, nearest first.
///
/// Doc 04 section 4 caps the chain at three, and the cap is enforced here
/// rather than by the caller because an unbounded walk on a deep board would
/// put an entire thread into a prompt.
///
/// A cycle would hang this, and `parent_card_id` is a plain foreign key that
/// nothing stops from pointing at a descendant, so the walk also refuses to
/// visit a card twice.
pub fn ancestor_chain(store: &Store, card_id: &str, limit: usize) -> Result<Vec<Ancestor>> {
    let conn = store.conn();
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(card_id.to_string());

    let mut next: Option<String> = conn
        .query_row(
            "SELECT parent_card_id FROM card WHERE id = ?1",
            params![card_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();

    while let Some(id) = next {
        if chain.len() >= limit || !seen.insert(id.clone()) {
            break;
        }
        let row = conn
            .query_row(
                "SELECT id, question, answer, depth, confidence, updated_at, parent_card_id, status
                 FROM card WHERE id = ?1",
                params![&id],
                |r| {
                    Ok((
                        Ancestor {
                            card_id: r.get(0)?,
                            question: r.get(1)?,
                            answer: r.get(2)?,
                            depth: r.get(3)?,
                            confidence: r.get(4)?,
                            // An unanswered card has no answered_at to give, and
                            // a timestamp on a card that never answered would
                            // read as freshness it does not have.
                            answered_at: match r.get::<_, String>(7)?.as_str() {
                                "done" | "flagged" => Some(r.get(5)?),
                                _ => None,
                            },
                            citations: Vec::new(),
                        },
                        r.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?;

        let Some((mut ancestor, parent)) = row else { break };
        ancestor.citations = read_citations(store, &ancestor.card_id)?;
        chain.push(ancestor);
        next = parent;
    }

    Ok(chain)
}

fn read_citations(store: &Store, card_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.ordinal, s.title, s.class, s.locator, c.verifier_verdict, s.stale, p.text,
                c.claim_span, c.binding
         FROM citation c JOIN passage p ON p.id = c.passage_id JOIN source s ON s.id = p.source_id
         WHERE c.card_id = ?1 ORDER BY c.ordinal ASC",
    )?;
    Ok(stmt
        .query_map(params![card_id], |r| {
            Ok(json!({
                "ordinal": r.get::<_, i64>(0)?,
                "source_title": r.get::<_, String>(1)?,
                "source_class": r.get::<_, String>(2)?,
                "locator": r.get::<_, String>(3)?,
                "verdict": r.get::<_, String>(4)?,
                "stale": r.get::<_, i64>(5)? != 0,
                // Doc 02 section 10.2 reports citation accuracy per Verifier
                // verdict *and* per ledger check, and the ledger check has to
                // ask the same question the Verifier did. Without the passage
                // the scorer could only ask a different one and call the gap
                // disagreement.
                "passage_text": r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                // And the other half of that question. The Verifier judges a
                // passage against the claim span it was bound to; a ledger check
                // over every citation instead asks whether each one states the
                // answer to the question, which most correct citations on a deep
                // card do not. BN-110: 0.365 was that difference, not the
                // product's citation accuracy.
                "claim_span": parse_json(&r.get::<_, String>(7)?),
                "binding": r.get::<_, String>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// A card read back for re-verification, with the current state of the sources
/// it cited. Doc 07 section B8.4's freshness check runs against this.
#[derive(Debug, Clone)]
pub struct CardForVerify {
    pub depth: String,
    pub answer: Option<String>,
    pub findings: Value,
    /// In the shape an agent packet uses, keyed `n` rather than `ordinal`, so
    /// the Verifier reads a re-verified card's citations exactly as it reads a
    /// freshly synthesised one.
    pub citations: Vec<Value>,
    /// One per citation, in the shape the Verifier's packet expects, carrying
    /// what the source looks like now rather than when the card was written.
    pub passages: Vec<Value>,
}

/// Read a card and its citations back, for a run that re-verifies rather than
/// answers. Doc 07 section B3.
///
/// The passages carry `stale` and `stale_reason` from the source rows as they
/// stand now, which is the whole point: a card written months ago is judged
/// against what its sources have since become.
pub fn read_card_for_verify(store: &Store, card_id: &str) -> Result<Option<CardForVerify>> {
    let conn = store.conn();
    let row: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT depth, answer, findings FROM card WHERE id = ?1",
            params![card_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;
    let Some((depth, answer, findings)) = row else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT c.ordinal, c.passage_id, p.text, s.title, s.class, s.locator, s.site_or_issuer,
                s.trust_rank, s.published_at, s.version_ref, s.stale, s.stale_reason, c.claim_span,
                c.binding
           FROM citation c
           JOIN passage p ON p.id = c.passage_id
           JOIN source s ON s.id = p.source_id
          WHERE c.card_id = ?1 ORDER BY c.ordinal ASC",
    )?;
    let rows: Vec<(Value, Value)> = stmt
        .query_map(params![card_id], |r| {
            let stale: i64 = r.get(10)?;
            let passage_id: String = r.get(1)?;
            let citation = json!({
                "n": r.get::<_, i64>(0)?,
                "passage_id": passage_id,
                "claim_span": parse_json(&r.get::<_, String>(12)?),
                "binding": r.get::<_, String>(13)?,
            });
            let passage = json!({
                "passage_id": r.get::<_, String>(1)?,
                "text": r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                "source": {
                    "title": r.get::<_, String>(3)?,
                    "class": r.get::<_, String>(4)?,
                    "locator": r.get::<_, String>(5)?,
                    "issuer": r.get::<_, Option<String>>(6)?,
                    "trust_rank": r.get::<_, i64>(7)?,
                    "published_at": r.get::<_, Option<String>>(8)?,
                    "version_ref": r.get::<_, Option<String>>(9)?,
                    "stale": stale != 0,
                    "stale_reason": r.get::<_, Option<String>>(11)?,
                },
            });
            Ok((citation, passage))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let (citations, passages) = rows.into_iter().unzip();
    Ok(Some(CardForVerify {
        depth,
        answer,
        findings: parse_json(&findings),
        citations,
        passages,
    }))
}

fn read_flags(store: &Store, card_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, rule_id, severity, reason FROM flag
         WHERE card_id = ?1 AND status = 'open' ORDER BY created_at ASC",
    )?;
    Ok(stmt
        .query_map(params![card_id], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "rule_id": r.get::<_, String>(1)?,
                "severity": r.get::<_, String>(2)?,
                "reason": r.get::<_, String>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The Home grid. Doc 09 section 3: boards with their open flag count and last
/// activity.
/// The boards a listing shows. Doc 16 section 3.4 and BN-106.
///
/// Home is the boards a person explores and learns on; the Notebook lists its
/// own sessions; and the Map, when doc 17 builds it, is a board nothing lists
/// at all. A filter rather than three queries, because the difference between
/// them is one column.
pub fn list_boards_in(store: &Store, profile_id: &str, status: &str, modes: &[&str]) -> Result<Vec<Value>> {
    let all = list_boards(store, profile_id, status)?;
    if modes.is_empty() {
        return Ok(all);
    }
    Ok(all
        .into_iter()
        .filter(|b| {
            b.get("mode")
                .and_then(Value::as_str)
                .is_some_and(|m| modes.contains(&m))
        })
        .collect())
}

pub fn list_boards(store: &Store, profile_id: &str, status: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT b.id, b.title, b.updated_at, b.mode,
                (SELECT COUNT(*) FROM card c WHERE c.board_id = b.id) AS cards,
                (SELECT COUNT(*) FROM flag f JOIN card c2 ON c2.id = f.card_id
                 WHERE c2.board_id = b.id AND f.status = 'open') AS open_flags
         FROM board b
         WHERE b.profile_id = ?1 AND b.status = ?2
         ORDER BY b.updated_at DESC",
    )?;
    Ok(stmt
        .query_map(params![profile_id, status], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "updated_at": r.get::<_, String>(2)?,
                "mode": r.get::<_, String>(3)?,
                "cards": r.get::<_, i64>(4)?,
                "open_flags": r.get::<_, i64>(5)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Ensure a profile exists and return its id. First run, doc 11 section 6.
pub fn ensure_profile(store: &Store, pack_id: &str, default_depth: &str, policy: &Value) -> Result<String> {
    if let Some(id) = store
        .conn()
        .query_row("SELECT id FROM profile LIMIT 1", [], |r| r.get::<_, String>(0))
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    let now = now_iso8601();
    store.conn().execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                              retriever_config, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '{}', ?5, ?5)",
        params![id, default_depth, pack_id, policy.to_string(), now],
    )?;
    Ok(id)
}

/// The code of the pack this profile last chose, if the row still names one.
///
/// The profile's pack is a choice that has to outlive the process. Before this
/// the core read `general` at every start, so a person who chose finance came
/// back the next morning judged by rules they had switched away from, and
/// nothing on the screen said so.
pub fn active_pack_code(store: &Store) -> Result<Option<String>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT p.code FROM profile pr
               JOIN doctrine_pack p ON p.id = pr.default_doctrine_pack_id
              LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Point the profile at a pack version. Boards keep the version they pinned.
pub fn set_active_pack(store: &Store, profile_id: &str, pack_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE profile SET default_doctrine_pack_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![pack_id, now_iso8601(), profile_id],
    )?;
    Ok(())
}

/// Open a notebook session on a board. Doc 16 section 3.4.
///
/// "Sessions are boards of `mode: notebook` so history, events, memory, and
/// export come free." The mode is what narrows retrieval, so it moves with the
/// session rather than being inferred from what the board looks like.
pub fn start_notebook(store: &mut Store, board_id: &str) -> Result<()> {
    let (board, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "notebook.asked.v1",
            json!({ "board_id": board_id, "opened": true }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET mode = 'notebook', updated_at = ?1 WHERE id = ?2",
                params![now, board],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Register a doctrine pack version, returning its row id. A pack version is
/// inserted once; boards pin it (doc 01 section 4.17).
pub fn ensure_pack(store: &Store, pack: &Value) -> Result<String> {
    let code = pack.get("code").and_then(Value::as_str).unwrap_or("general");
    let version = pack.get("version").and_then(Value::as_str).unwrap_or("1.0.0");

    if let Some(id) = store
        .conn()
        .query_row(
            "SELECT id FROM doctrine_pack WHERE code = ?1 AND version = ?2",
            params![code, version],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }

    let id = new_id();
    let field = |name: &str| pack.get(name).cloned().unwrap_or(json!([])).to_string();
    store.conn().execute(
        "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                    flag_rules, retrievers, exercise_templates, rulings, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            code,
            version,
            field("audiences"),
            field("source_hierarchy"),
            pack.get("freshness_classes")
                .cloned()
                .unwrap_or(json!({}))
                .to_string(),
            field("flag_rules"),
            field("retrievers"),
            field("exercise_templates"),
            field("rulings"),
            now_iso8601(),
        ],
    )?;
    Ok(id)
}

/// Give a board a name the user chose.
///
/// `named_by_user` is what stops the next question overwriting it: doc 01
/// section 4.1 lets the first question title an unnamed board, and once a person
/// has typed a title that inference has to stop.
pub fn rename_board(store: &mut Store, board_id: &str, title: &str) -> Result<()> {
    let (id, name, now) = (board_id.to_string(), title.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.renamed.v1",
            json!({ "board_id": board_id, "title": title }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET title = ?1, named_by_user = 1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// The doctrine pack a board pinned. Doc 01 section 4.17.
#[derive(Debug, Clone)]
pub struct PinnedPack {
    pub pack_id: String,
    pub code: String,
    pub version: String,
}

pub fn board_pack(store: &Store, board_id: &str) -> Result<Option<PinnedPack>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT p.id, p.code, p.version FROM board b
               JOIN doctrine_pack p ON p.id = b.doctrine_pack_id
              WHERE b.id = ?1",
            params![board_id],
            |r| {
                Ok(PinnedPack {
                    pack_id: r.get(0)?,
                    code: r.get(1)?,
                    version: r.get(2)?,
                })
            },
        )
        .optional()?)
}

/// The cards on a board that a re-verification has something to judge.
///
/// A card with no answer has nothing for the Verifier to read, and a blocked
/// one was never admitted, so neither is re-judged by a pack update. Ordered by
/// creation so a batch runs the board top to bottom and its events read in the
/// order a person would expect.
pub fn cards_to_reverify(store: &Store, board_id: &str) -> Result<Vec<String>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id FROM card
          WHERE board_id = ?1 AND status IN ('done', 'flagged')
          ORDER BY created_at, id",
    )?;
    Ok(stmt
        .query_map(params![board_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Point a board at a newer version of the pack it pinned. Doc 10 section 9.
///
/// The pin moves only here. A pack update never rewrites it on its own, because
/// a board's answers were judged by the rules of the version it names and
/// changing that silently would make the claim untrue after the fact.
pub fn repin_board(
    store: &mut Store,
    board_id: &str,
    to: &PinnedPack,
    from_version: &str,
    cards: usize,
) -> Result<()> {
    let (id, pack_id, now) = (board_id.to_string(), to.pack_id.clone(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.pack_updated.v1",
            json!({
                "board_id": board_id,
                "pack_code": to.code,
                "from_version": from_version,
                "to_version": to.version,
                "cards_to_reverify": cards,
            }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET doctrine_pack_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![pack_id, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Move a board to Trash. Doc 09 open question 1, adopted by doc 11: Trash is a
/// filter on Home rather than a rail item of its own.
///
/// Doc 09 section 5: every verb is undoable within the session except Remove on
/// a board, which goes here instead of vanishing.
pub fn trash_board(store: &mut Store, board_id: &str) -> Result<()> {
    let (id, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.trashed.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET status = 'trashed', trashed_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

pub fn restore_board(store: &mut Store, board_id: &str) -> Result<()> {
    let (id, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.restored.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET status = 'active', trashed_at = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Delete a board and everything hanging from it.
///
/// The events stay. The log is append only and the database enforces it with a
/// trigger, so a purge removes the entities and leaves the trail that says they
/// existed, which is what makes `board.purged.v1` readable afterwards rather
/// than a claim about rows nobody can check.
pub fn purge_board(store: &mut Store, board_id: &str) -> Result<()> {
    let id = board_id.to_string();
    store.append_with(
        NewEvent::new(
            "board.purged.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            // Cards cascade from the board, and citations and flags cascade from
            // the cards. Visuals and notes are keyed on the board the same way.
            tx.execute("DELETE FROM board WHERE id = ?1", params![id])?;
            Ok(())
        },
    )?;
    Ok(())
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

/// Doc 14 section 3.6's mastery: plus one on a correct check, minus one on a
/// wrong one, floored at zero.
///
/// Floored rather than allowed negative, because a score below zero would say
/// something about a learner that the counting cannot support: three wrong
/// answers in a row on one concept is the same signal as thirty.
pub fn score_mastery(mastery: &Value, concept_ids: &[String], correct: bool) -> Value {
    let mut map = mastery.as_object().cloned().unwrap_or_default();
    for id in concept_ids {
        let now = map.get(id).and_then(Value::as_i64).unwrap_or(0);
        let next = if correct { now + 1 } else { (now - 1).max(0) };
        map.insert(id.clone(), json!(next));
    }
    Value::Object(map)
}

/// The cards a Tutor packet carries. Doc 14 section 3.2.
///
/// Question, answer, visual labels and citations. Never the passages behind
/// them: doc 14 section 3.1 keeps retrieval out of the Tutor's scope, and a
/// packet carrying a passage would be the first step towards it writing content.
pub fn cards_for_tutor(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    let cards = cards_for_exercise(store, board_id, 12)?;
    Ok(cards
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
        .collect())
}

/// Store an image and the row that points at it. Doc 01 section 4.6.
///
/// The bytes go to the blob store by hash, so a board forked from a bundle never
/// duplicates a picture, and the row carries the size the Reader's packet needs.
pub struct NewImage<'a> {
    pub board_id: &'a str,
    pub origin: &'a str,
    pub bytes: &'a [u8],
    pub mime: &'a str,
    pub width: u32,
    pub height: u32,
    pub source_ink_ids: Option<&'a str>,
}

pub fn write_image(store: &mut Store, image: NewImage<'_>) -> Result<String> {
    let blob_ref = store.blobs().put(image.bytes)?;
    let id = new_id();
    let now = now_iso8601();
    let (row, board, origin, blob, mime, ink) = (
        id.clone(),
        image.board_id.to_string(),
        image.origin.to_string(),
        blob_ref.clone(),
        image.mime.to_string(),
        image.source_ink_ids.map(str::to_string),
    );
    let (w, h) = (image.width as i64, image.height as i64);

    let event = match image.origin {
        // Doc 01 section 6.3 has two names here and they mean different things:
        // one is a person putting a picture on a board, the other is the product
        // making one.
        "generated" => "image.generated.v1",
        "sketch_raster" => "sketch.rasterised.v1",
        _ => "image.pasted.v1",
    };

    store.append_with(
        NewEvent::new(
            event,
            json!({
                "image_id": id, "origin": image.origin, "blob_ref": blob_ref,
                "mime": image.mime, "width": image.width, "height": image.height
            }),
            Provenance::user(),
        )
        .on_board(image.board_id),
        move |tx| {
            tx.execute(
                "INSERT INTO image (id, board_id, origin, blob_ref, mime, width, height,
                                    position, source_ink_ids, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{\"x\":0,\"y\":0}', ?8, ?9)",
                params![row, board, origin, blob, mime, w, h, ink, now],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// One image row, with its bytes. What doc 07 section A4's packet is built from.
pub fn read_image(store: &Store, image_id: &str) -> Result<Option<(Value, Vec<u8>)>> {
    let row: Option<(String, String, String, i64, i64, String)> = store
        .conn()
        .query_row(
            "SELECT id, origin, blob_ref, width, height, mime FROM image WHERE id = ?1",
            params![image_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;

    let Some((id, origin, blob_ref, width, height, mime)) = row else {
        return Ok(None);
    };
    let bytes = store.blobs().get(&blob_ref)?;
    Ok(Some((
        json!({
            "image_id": id,
            "origin": origin,
            "blob_ref": blob_ref,
            "mime": mime,
            "width": width,
            "height": height,
        }),
        bytes,
    )))
}

/// The ink a board holds, for the sketch raster path.
pub fn read_ink(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt =
        conn.prepare("SELECT points, colour, width FROM ink WHERE board_id = ?1 ORDER BY created_at")?;
    Ok(stmt
        .query_map(params![board_id], |r| {
            let points: String = r.get(0)?;
            Ok(json!({
                "points": serde_json::from_str::<Value>(&points).unwrap_or_else(|_| json!([])),
                "colour": r.get::<_, String>(1)?,
                "width": r.get::<_, f64>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Finish a Reader card. Doc 07 section A7's `read.completed.v1`.
pub struct ReadResult<'a> {
    pub image_id: &'a str,
    pub kind: &'a str,
    pub legibility: f64,
    pub injection_suspected: bool,
    pub notable: &'a Value,
}

pub fn finish_read(store: &mut Store, at: CardRef<'_>, result: ReadResult<'_>) -> Result<()> {
    store.append(
        NewEvent::new(
            "read.completed.v1",
            json!({
                "card_id": at.card_id,
                "image_id": result.image_id,
                "kind": result.kind,
                "legibility": result.legibility,
                "injection_suspected": result.injection_suspected,
                "notable_count": result.notable.as_array().map(Vec::len).unwrap_or(0),
            }),
            Provenance::agent("reader", at.run_id.to_string()),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}

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

/// Library, Sources tab. Doc 09 section 9.
///
/// "title, issuer, class, trust rank, cited on n cards, last verified, stale
/// state". The card count is the column that decides whether a source can be
/// removed: doc 09 section 5 allows Remove on a source only if it is uncited.
pub fn list_sources(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.class, s.site_or_issuer, s.locator, s.trust_rank,
                s.last_verified_at, s.stale, s.stale_reason, s.freshness_class, s.version_ref,
                -- A citation names a passage, and a passage names its source,
                -- so the count of cards citing a source is two joins rather
                -- than a column the citation does not carry.
                (SELECT COUNT(DISTINCT c.card_id) FROM citation c
                 JOIN passage pg ON pg.id = c.passage_id
                 WHERE pg.source_id = s.id) AS cards
         FROM source s
         WHERE s.profile_id = ?1
         ORDER BY s.stale DESC, s.trust_rank, s.title
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "class": r.get::<_, String>(2)?,
                "issuer": r.get::<_, Option<String>>(3)?,
                "locator": r.get::<_, String>(4)?,
                "trust_rank": r.get::<_, i64>(5)?,
                "last_verified_at": r.get::<_, Option<String>>(6)?,
                "stale": r.get::<_, i64>(7)? == 1,
                "stale_reason": r.get::<_, Option<String>>(8)?,
                "freshness_class": r.get::<_, String>(9)?,
                "version_ref": r.get::<_, Option<String>>(10)?,
                "cards": r.get::<_, i64>(11)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Propose the concepts a card named, and link them to it.
///
/// Doc 01 section 4.10: "Agents propose; the user confirms." The Router already
/// returns the entities a question names, and until M9 they went into the log
/// and nowhere else, so the Planner packet's `concepts` was an empty array and
/// entity resolution degraded to literals marked `unknown` exactly as doc 04
/// says it should when the graph is empty.
///
/// A term the profile already knows is reused rather than duplicated, which is
/// what doc 01 section 4.11 means by "two boards that both cite the same Concept
/// share it". Matching is case insensitive on the canonical spelling; an alias
/// pass belongs with the Concept editor, not here.
///
/// Returns how many concepts were newly proposed.
pub fn propose_concepts(
    store: &mut Store,
    at: CardRef<'_>,
    profile_id: &str,
    doctrine_pack_id: &str,
    terms: &[String],
    proposed_by: &str,
) -> Result<usize> {
    let mut proposed = 0usize;

    for term in terms {
        let term = term.trim();
        // A one character entity is noise, and an empty one is a model slip.
        if term.chars().count() < 2 || term.chars().count() > 120 {
            continue;
        }

        let existing: Option<String> = store
            .conn()
            .query_row(
                "SELECT id FROM concept WHERE profile_id = ?1 AND lower(term) = lower(?2)",
                params![profile_id, term],
                |r| r.get(0),
            )
            .optional()?;

        let concept_id = match existing {
            Some(id) => {
                // Doc 01 section 6.3's `entity.resolved.v1`: the entity named a
                // node the profile already has, which is the whole point of the
                // graph being shared across boards.
                store.append(
                    NewEvent::new(
                        "entity.resolved.v1",
                        json!({ "concept_id": id, "term": term, "card_id": at.card_id }),
                        Provenance::agent(proposed_by, at.run_id.to_string()),
                    )
                    .on_board(at.board_id)
                    .on_card(at.card_id),
                )?;
                id
            }
            None => {
                let id = new_id();
                let now = now_iso8601();
                let (row, profile, pack, name, at_time) = (
                    id.clone(),
                    profile_id.to_string(),
                    doctrine_pack_id.to_string(),
                    term.to_string(),
                    now,
                );
                store.append_with(
                    NewEvent::new(
                        "concept.proposed.v1",
                        json!({ "concept_id": id, "term": term, "card_id": at.card_id }),
                        Provenance::agent(proposed_by, at.run_id.to_string()),
                    )
                    .on_board(at.board_id)
                    .on_card(at.card_id),
                    move |tx| {
                        tx.execute(
                            "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status,
                                                  created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, 'proposed', ?5, ?5)",
                            params![row, profile, name, pack, at_time],
                        )?;
                        Ok(())
                    },
                )?;
                proposed += 1;
                id
            }
        };

        // One link per concept per card. A card asked twice about the same term
        // should touch the node once.
        let linked: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM concept_link
             WHERE concept_id = ?1 AND target_type = 'card' AND target_ref = ?2",
            params![concept_id, at.card_id],
            |r| r.get(0),
        )?;
        if linked > 0 {
            continue;
        }

        let link_id = new_id();
        let (row, cid, card, by, now) = (
            link_id.clone(),
            concept_id.clone(),
            at.card_id.to_string(),
            json!({ "agent_id": proposed_by }).to_string(),
            now_iso8601(),
        );
        store.append_with(
            NewEvent::new(
                "concept.linked.v1",
                json!({
                    "link_id": link_id,
                    "concept_id": concept_id,
                    "target_type": "card",
                    "target_ref": at.card_id,
                    "relation": "mentions",
                }),
                Provenance::agent(proposed_by, at.run_id.to_string()),
            )
            .on_board(at.board_id)
            .on_card(at.card_id),
            move |tx| {
                tx.execute(
                    "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation,
                                               proposed_by, status, created_at)
                     VALUES (?1, ?2, 'card', ?3, 'mentions', ?4, 'proposed', ?5)",
                    params![row, cid, card, by, now],
                )?;
                Ok(())
            },
        )?;
    }

    Ok(proposed)
}

/// Confirm or reject a proposed concept. Doc 09 section 9's row actions.
///
/// `None` means no proposed concept had that id, so nothing was decided and the
/// caller can say so rather than reporting a decision that never happened.
pub fn decide_concept(store: &mut Store, concept_id: &str, accept: bool) -> Result<Option<String>> {
    let term: Option<String> = store
        .conn()
        .query_row(
            "SELECT term FROM concept WHERE id = ?1 AND status = 'proposed'",
            params![concept_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(term) = term else { return Ok(None) };

    let (id, now) = (concept_id.to_string(), now_iso8601());
    if accept {
        store.append_with(
            NewEvent::new(
                "concept.confirmed.v1",
                json!({ "concept_id": concept_id, "term": term }),
                Provenance::user(),
            ),
            move |tx| {
                tx.execute(
                    "UPDATE concept SET status = 'confirmed', updated_at = ?1 WHERE id = ?2",
                    params![now, id],
                )?;
                Ok(())
            },
        )?;
    } else {
        // A rejected concept leaves, and its links leave with it. Doc 01 section
        // 4.11: links are how boards touch a node, so a node nobody kept has
        // nothing left to touch. There is no `concept.rejected.v1` in the
        // vocabulary and this does not invent one: the link rows carry the
        // status the model has for a rejection, and `concept.linked.v1` already
        // said they existed.
        store.append_with(
            NewEvent::new(
                "concept.linked.v1",
                json!({ "concept_id": concept_id, "term": term, "status": "rejected" }),
                Provenance::user(),
            ),
            move |tx| {
                tx.execute(
                    "UPDATE concept_link SET status = 'rejected' WHERE concept_id = ?1",
                    params![id],
                )?;
                Ok(())
            },
        )?;
    }
    Ok(Some(term))
}

/// The concepts the Planner packet carries. Doc 04 section 4.
///
/// Confirmed terms first, because those are the ones a person has stood behind,
/// then proposed ones, so a fresh profile still gets the graph it has.
pub fn concepts_for_packet(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, term, definition, status FROM concept
         WHERE profile_id = ?1
         ORDER BY CASE status WHEN 'confirmed' THEN 0 ELSE 1 END, term
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            Ok(json!({
                "concept_id": r.get::<_, String>(0)?,
                "term": r.get::<_, String>(1)?,
                "definition": r.get::<_, Option<String>>(2)?,
                "status": r.get::<_, String>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Library, Concepts tab. Doc 09 section 9.
///
/// "term, status (proposed or confirmed), definition, audience definitions,
/// linked cards". The link count is what doc 09 section 5's Remove on a concept
/// checks: only if unlinked.
pub fn list_concepts(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.term, c.status, c.definition, c.aliases, c.audience_definitions,
                c.definition_card_id, c.updated_at,
                (SELECT COUNT(*) FROM concept_link l
                 WHERE l.concept_id = c.id AND l.status != 'rejected') AS links
         FROM concept c
         WHERE c.profile_id = ?1
         ORDER BY CASE c.status WHEN 'proposed' THEN 0 ELSE 1 END, c.term
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            let json_col = |v: Option<String>| {
                v.and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .unwrap_or(Value::Null)
            };
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "term": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "definition": r.get::<_, Option<String>>(3)?,
                "aliases": json_col(r.get::<_, Option<String>>(4)?),
                "audience_definitions": json_col(r.get::<_, Option<String>>(5)?),
                "definition_card_id": r.get::<_, Option<String>>(6)?,
                "updated_at": r.get::<_, String>(7)?,
                "links": r.get::<_, i64>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Profile, Diagnostics page. Doc 11 section 6.
///
/// Counts rather than a health verdict: what the page is for is telling a user
/// whether the thing they think happened happened, and a green tick that
/// summarises six numbers hides the one that is wrong.
pub fn profile_counts(store: &Store, profile_id: &str) -> Result<Value> {
    let conn = store.conn();
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, params![profile_id], |r| r.get(0))?) };

    Ok(json!({
        "boards": one("SELECT COUNT(*) FROM board WHERE profile_id = ?1 AND status = 'active'")?,
        "boards_trashed": one("SELECT COUNT(*) FROM board WHERE profile_id = ?1 AND status = 'trashed'")?,
        "cards": one(
            "SELECT COUNT(*) FROM card c JOIN board b ON b.id = c.board_id WHERE b.profile_id = ?1",
        )?,
        "open_flags": one(
            "SELECT COUNT(*) FROM flag f JOIN card c ON c.id = f.card_id
             JOIN board b ON b.id = c.board_id WHERE b.profile_id = ?1 AND f.status = 'open'",
        )?,
        "sources": one("SELECT COUNT(*) FROM source WHERE profile_id = ?1")?,
        "sources_stale": one("SELECT COUNT(*) FROM source WHERE profile_id = ?1 AND stale = 1")?,
        "concepts": one("SELECT COUNT(*) FROM concept WHERE profile_id = ?1")?,
        "events": conn.query_row("SELECT COUNT(*) FROM event", [], |r| r.get::<_, i64>(0))?,
    }))
}

/// Bump a board's last activity, which is what the Home grid sorts on.
pub fn touch_board(store: &Store, board_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE board SET updated_at = ?1 WHERE id = ?2",
        params![now_iso8601(), board_id],
    )?;
    Ok(())
}

/// Doc 09 section 12: board history, rendered from events with filters by agent
/// and by user action.
pub fn board_history(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    Ok(store
        .events(Some(board_id))?
        .into_iter()
        .map(|e| {
            json!({
                "event_id": e.event_id,
                "index": e.monotonic_index,
                "type": e.event_type,
                "payload": e.payload,
                "actor": e.provenance.emitter_id,
                "actor_type": e.provenance.emitter_type,
                "source": e.provenance.source,
                "card_id": e.card_id,
                "at": e.timestamp,
            })
        })
        .collect())
}

/// Test provenance never reaches a user facing history view.
pub fn is_user_visible(source: Source) -> bool {
    matches!(source, Source::Live | Source::Harness)
}

// --------------------------------------------------------------- stickies ---
//
// Doc 01 section 4.5's Note, which doc 16 section 7 point 1 calls a sticky
// everywhere a person can read it, to keep it apart from a vault page. The
// table has existed since 0001 with nothing writing to it, so `note.added.v1`
// was in the vocabulary and had never once been emitted.

/// A sticky as it is put on the board.
pub struct NewNote<'a> {
    pub board_id: &'a str,
    pub text: &'a str,
    /// A palette token name, doc 01 section 4.5.
    pub colour: &'a str,
    /// `{x, y, w, h}` in board coordinates.
    pub position: Value,
    /// The card this sticky is about, when it came from a quote. Doc 16 section
    /// 3.6 draws the dashed edge from this.
    pub card_id: Option<&'a str>,
}

/// One sticky as it is read back.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteView {
    pub id: String,
    pub text: String,
    pub colour: String,
    pub position: Value,
    pub card_id: Option<String>,
}

pub fn write_note(store: &mut Store, n: NewNote<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row, board, text, colour, position, card, at) = (
        id.clone(),
        n.board_id.to_string(),
        n.text.to_string(),
        n.colour.to_string(),
        n.position.to_string(),
        n.card_id.map(str::to_string),
        now,
    );

    let mut event = NewEvent::new(
        "note.added.v1",
        json!({
            "note_id": id,
            "board_id": n.board_id,
            "card_id": n.card_id,
            // The words are the person's own and the log is not where they are
            // kept: the row is. What the event records is that a sticky exists
            // and what it is about.
            "characters": n.text.chars().count(),
        }),
        Provenance::user(),
    )
    .on_board(n.board_id);
    if let Some(card_id) = n.card_id {
        event = event.on_card(card_id);
    }

    store.append_with(event, move |tx| {
        tx.execute(
            "INSERT INTO note (id, board_id, text, colour, position, card_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![row, board, text, colour, position, card, at],
        )?;
        Ok(())
    })?;
    Ok(id)
}

/// Replace a sticky's text, or move it, or both.
pub fn edit_note(
    store: &mut Store,
    note_id: &str,
    text: Option<&str>,
    position: Option<Value>,
) -> Result<()> {
    let (id, body, place, now) = (
        note_id.to_string(),
        text.map(str::to_string),
        position.map(|p| p.to_string()),
        now_iso8601(),
    );
    store.append_with(
        NewEvent::new(
            "note.edited.v1",
            json!({
                "note_id": note_id,
                "text_changed": body.is_some(),
                "moved": place.is_some(),
            }),
            Provenance::user(),
        ),
        move |tx| {
            // Two columns, either of which may be left alone. COALESCE rather
            // than two statements, so a move and a rewrite in one call are one
            // write and one row version.
            tx.execute(
                "UPDATE note SET text = COALESCE(?1, text), position = COALESCE(?2, position),
                     updated_at = ?3 WHERE id = ?4",
                params![body, place, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Take a sticky off the board. Doc 09 section 5: every verb has an undo, and
/// this is what Add note's is.
pub fn remove_note(store: &mut Store, note_id: &str) -> Result<()> {
    let id = note_id.to_string();
    // Read the board before the row is gone. An event with no board is an event
    // the board's own history never shows, which for a verb whose whole point
    // is being undoable is the one place it has to appear.
    let board: Option<String> = store
        .conn()
        .query_row("SELECT board_id FROM note WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()?;

    let mut event = NewEvent::new(
        "note.removed.v1",
        json!({ "note_id": note_id }),
        Provenance::user(),
    );
    if let Some(board_id) = &board {
        event = event.on_board(board_id);
    }

    store.append_with(event, move |tx| {
        tx.execute("DELETE FROM note WHERE id = ?1", params![id])?;
        Ok(())
    })?;
    Ok(())
}

/// The stickies on a board, oldest first.
pub fn read_notes(store: &Store, board_id: &str) -> Result<Vec<NoteView>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, text, colour, position, card_id FROM note
         WHERE board_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![board_id], |r| {
        let position: String = r.get(3)?;
        Ok(NoteView {
            id: r.get(0)?,
            text: r.get(1)?,
            colour: r.get(2)?,
            position: serde_json::from_str(&position).unwrap_or_else(|_| json!({})),
            card_id: r.get(4)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

// ------------------------------------------------------------------ vault ---

/// A page as it is written. Doc 16 section 3.1.
pub struct NewPage<'a> {
    pub profile_id: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    /// `vault/<slug>.md`, relative to the profile folder. The caller computes
    /// it, because the slug rule belongs with the mirror that writes the file
    /// and not with the table that indexes it.
    pub file_path: &'a str,
    /// Set by Save as page. Doc 16 section 3.2.
    pub source_card_id: Option<&'a str>,
    /// `[{ordinal, passage_id}]`, copied from the card. Doc 16 section 2.2: a
    /// page is context and the passages it carries are the evidence, so they
    /// are copied once and never re-derived from the page's own text.
    pub citations_carried: Value,
    pub doctrine_pack_id: Option<&'a str>,
}

/// A page as it is read.
#[derive(Debug, Clone)]
pub struct PageRow {
    pub id: String,
    pub title: String,
    pub body: String,
    pub file_path: String,
    pub source_card_id: Option<String>,
    pub citations_carried: Value,
    /// The hash of the text this row and its file last agreed on.
    ///
    /// Not the hash of `body`, which is derivable from `body`. Deciding which
    /// of the two copies moved needs a third value, and this is it: an edit in
    /// the app leaves it alone, and the mirror writes it when it reconciles.
    pub synced_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

/// What a title collision looks like to a caller.
///
/// Doc 16 section 3.1 makes the title unique per profile and case insensitive,
/// so this is a rule the person meets rather than a constraint error they
/// should have to read.
pub const PAGE_TITLE_TAKEN: &str = "page_title_taken";

fn page_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PageRow> {
    Ok(PageRow {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        file_path: r.get(3)?,
        source_card_id: r.get(4)?,
        citations_carried: parse_json(&r.get::<_, String>(5)?),
        synced_hash: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

const PAGE_COLUMNS: &str = "id, title, body, file_path, source_card_id, citations_carried,
     synced_hash, created_at, updated_at";

/// Write a page. Doc 16 section 3.1.
///
/// The event depends on where the page came from: a page saved from a card is
/// `page.created_from_card.v1`, which doc 16 section 3.2 names, and one written
/// by hand is `page.created.v1`. One writer, two events, because the two are
/// different claims about where the text came from and the log is what a person
/// reads to find out.
pub fn create_page(store: &mut Store, p: NewPage<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    // The file does not exist yet and the mirror writes it from this body, so
    // the two agree on it from the start. A sync that crashes before the write
    // finds the file missing next time and writes it, which lands in the same
    // place.
    let hash = crate::blob::BlobStore::hash(p.body.as_bytes());
    let event_type = if p.source_card_id.is_some() {
        "page.created_from_card.v1"
    } else {
        "page.created.v1"
    };

    let (row_id, profile_id, title, body, file_path, card, carried, pack, hash_for_row, now_for_row) = (
        id.clone(),
        p.profile_id.to_string(),
        p.title.trim().to_string(),
        p.body.to_string(),
        p.file_path.to_string(),
        p.source_card_id.map(str::to_string),
        p.citations_carried.to_string(),
        p.doctrine_pack_id.map(str::to_string),
        hash.clone(),
        now.clone(),
    );

    let mut event = NewEvent::new(
        event_type,
        json!({
            "page_id": id,
            "profile_id": p.profile_id,
            "title": p.title.trim(),
            "file_path": p.file_path,
            "source_card_id": p.source_card_id,
            "citations_carried": p.citations_carried.as_array().map(Vec::len).unwrap_or(0),
        }),
        Provenance::user(),
    );
    if let Some(card_id) = p.source_card_id {
        event = event.on_card(card_id);
    }

    store.append_with(event, move |tx| {
        tx.execute(
            "INSERT INTO page (id, profile_id, title, body, file_path, source_card_id,
                 citations_carried, doctrine_pack_id, synced_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                row_id,
                profile_id,
                title,
                body,
                file_path,
                card,
                carried,
                pack,
                hash_for_row,
                now_for_row
            ],
        )?;
        Ok(())
    })?;
    Ok(id)
}

/// Replace a page's body. Doc 16 section 4's `page.edited.v1`.
///
/// `synced_hash` is deliberately left where it is. It records what the row and
/// the file last agreed on, so an edit here is exactly the event that makes
/// them disagree, and moving it would erase the evidence the mirror reads.
pub fn edit_page(store: &mut Store, page_id: &str, body: &str) -> Result<()> {
    let (id, text, now) = (page_id.to_string(), body.to_string(), now_iso8601());
    let hash = crate::blob::BlobStore::hash(body.as_bytes());
    store.append_with(
        NewEvent::new(
            "page.edited.v1",
            json!({ "page_id": page_id, "body_hash": hash, "length": body.len() }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET body = ?1, updated_at = ?2 WHERE id = ?3",
                params![text, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Record that the row and its file now hold the same text.
///
/// Written by the mirror after it reconciles them, and by nothing else. Doc 16
/// section 7 point 2's conflict rule is decidable only while this is true of
/// the last agreement rather than of the newest write.
pub fn mark_page_synced(store: &Store, page_id: &str, body: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE page SET synced_hash = ?1 WHERE id = ?2",
        params![crate::blob::BlobStore::hash(body.as_bytes()), page_id],
    )?;
    Ok(())
}

/// Take the file's text as the page's, when the mirror finds the file moved and
/// the row did not.
///
/// One statement, because the body and the agreement move together here: the
/// row now says what the file says.
pub fn adopt_page_body(store: &mut Store, page_id: &str, body: &str) -> Result<()> {
    let (id, text, now) = (page_id.to_string(), body.to_string(), now_iso8601());
    let hash = crate::blob::BlobStore::hash(body.as_bytes());
    let hash_for_row = hash.clone();
    store.append_with(
        NewEvent::new(
            "page.edited.v1",
            json!({
                "page_id": page_id,
                "body_hash": hash,
                "length": body.len(),
                // The one edit nobody made in the app. Doc 16 section 3.1: the
                // vault is the person's even without Tessera running.
                "edited_in": "vault",
            }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET body = ?1, synced_hash = ?2, updated_at = ?3 WHERE id = ?4",
                params![text, hash_for_row, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Rename a page, keeping its id. Doc 16 section 3.1.
///
/// The id is what a wikilink resolves to, which is the whole reason a rename is
/// not a delete and a create: doc 16 section 2.2 lists resolution by title
/// string as one of the package's mistakes, because renames silently break the
/// links into it.
pub fn rename_page(store: &mut Store, page_id: &str, title: &str, file_path: &str) -> Result<()> {
    let (id, name, path, now) = (
        page_id.to_string(),
        title.trim().to_string(),
        file_path.to_string(),
        now_iso8601(),
    );
    store.append_with(
        NewEvent::new(
            "page.renamed.v1",
            json!({ "page_id": page_id, "title": title.trim(), "file_path": file_path }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET title = ?1, file_path = ?2, updated_at = ?3 WHERE id = ?4",
                params![name, path, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Delete a page. Doc 16 section 2.1: a deleted page must not corrupt an answer
/// that cited it, and it cannot, because a citation names a Passage and the
/// passage carries its own verbatim text.
pub fn delete_page(store: &mut Store, page_id: &str) -> Result<()> {
    let id = page_id.to_string();
    store.append_with(
        NewEvent::new(
            "page.deleted.v1",
            json!({ "page_id": page_id }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute("DELETE FROM page WHERE id = ?1", params![id])?;
            Ok(())
        },
    )?;
    Ok(())
}

pub fn read_page(store: &Store, page_id: &str) -> Result<Option<PageRow>> {
    let conn = store.conn();
    Ok(conn
        .query_row(
            &format!("SELECT {PAGE_COLUMNS} FROM page WHERE id = ?1"),
            params![page_id],
            page_row,
        )
        .optional()?)
}

/// The page with this title, case insensitively. Doc 16 section 3.1's
/// uniqueness rule read from the other side: this is what a wikilink resolves
/// through and what a create checks before it collides.
pub fn page_by_title(store: &Store, profile_id: &str, title: &str) -> Result<Option<PageRow>> {
    let conn = store.conn();
    Ok(conn
        .query_row(
            &format!(
                "SELECT {PAGE_COLUMNS} FROM page
                  WHERE profile_id = ?1 AND title = ?2 COLLATE NOCASE"
            ),
            params![profile_id, title.trim()],
            page_row,
        )
        .optional()?)
}

/// Every page in the profile, most recently edited first.
pub fn list_pages(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<PageRow>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(&format!(
        "SELECT {PAGE_COLUMNS} FROM page WHERE profile_id = ?1
          ORDER BY updated_at DESC, title LIMIT ?2"
    ))?;
    Ok(stmt
        .query_map(params![profile_id, limit], page_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One link out of a page, resolved. Doc 16 section 3.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPageLink {
    /// `page`, `concept` or `unresolved`.
    pub target_kind: String,
    /// The row the link points at, or `None` when nothing carries that title
    /// yet. Doc 16 section 3.1: clicking an unresolved link creates the page.
    pub target_id: Option<String>,
    /// The title the link names, which is what an unresolved one is waiting for
    /// and what clicking it would create. `[[Liquidity risk|the rule]]` names
    /// the first and shows the second.
    pub target_title: String,
    pub display_text: String,
    pub position: i64,
}

/// One link into something, with the page it came from. Doc 16 section 2.1's
/// backlinks panel.
#[derive(Debug, Clone)]
pub struct Backlink {
    pub page_id: String,
    pub page_title: String,
    pub display_text: String,
    pub position: i64,
}

/// Replace a page's outbound links with the ones its body now carries.
///
/// Replace rather than merge: the body is the truth about what it links to, and
/// a link the person deleted has to stop appearing in the target's backlinks.
///
/// One event per kind per save rather than one per link. A page with twenty
/// links would otherwise write twenty events on every keystroke-sized edit, and
/// what a person reads the log for is that a save resolved links or left some
/// hanging, not the twenty.
pub fn replace_page_links(store: &mut Store, page_id: &str, links: &[NewPageLink]) -> Result<()> {
    let now = now_iso8601();
    let resolved: Vec<&NewPageLink> = links.iter().filter(|l| l.target_id.is_some()).collect();
    let unresolved: Vec<&str> = links
        .iter()
        .filter(|l| l.target_id.is_none())
        .map(|l| l.target_title.as_str())
        .collect();

    let rows: Vec<(String, String, Option<String>, String, String, i64)> = links
        .iter()
        .map(|l| {
            (
                new_id(),
                l.target_kind.clone(),
                l.target_id.clone(),
                l.target_title.clone(),
                l.display_text.clone(),
                l.position,
            )
        })
        .collect();
    let id = page_id.to_string();
    let when = now.clone();

    let payload = json!({
        "page_id": page_id,
        "resolved": resolved.len(),
        "unresolved": unresolved.len(),
        "titles": unresolved,
    });
    let event_type = if unresolved.is_empty() {
        "page.link_resolved.v1"
    } else {
        // Doc 16 section 3.1: an unresolved link is kept and created on click,
        // so the save that left one is the thing worth recording.
        "page.link_unresolved.v1"
    };

    store.append_with(
        NewEvent::new(event_type, payload, Provenance::user()),
        move |tx| {
            tx.execute("DELETE FROM page_link WHERE from_page_id = ?1", params![id])?;
            for (link_id, kind, target, title, text, position) in rows {
                tx.execute(
                    "INSERT INTO page_link (id, from_page_id, target_kind, target_id,
                         target_title, display_text, position, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![link_id, id, kind, target, title, text, position, when],
                )?;
            }
            Ok(())
        },
    )?;
    Ok(())
}

/// The links out of one page, in body order.
pub fn page_links(store: &Store, page_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT target_kind, target_id, target_title, display_text, position FROM page_link
          WHERE from_page_id = ?1 ORDER BY position",
    )?;
    Ok(stmt
        .query_map(params![page_id], |r| {
            Ok(json!({
                "target_kind": r.get::<_, String>(0)?,
                "target_id": r.get::<_, Option<String>>(1)?,
                "target_title": r.get::<_, String>(2)?,
                "display_text": r.get::<_, String>(3)?,
                "position": r.get::<_, i64>(4)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Every link into a page or a concept. Doc 16 section 2.1.
///
/// A query over `page_link`, never a scan over bodies, which is what the
/// `page_link_target` index is for and what doc 16 phase 12c accepts on.
pub fn backlinks(store: &Store, target_kind: &str, target_id: &str) -> Result<Vec<Backlink>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT l.from_page_id, p.title, l.display_text, l.position
           FROM page_link l JOIN page p ON p.id = l.from_page_id
          WHERE l.target_kind = ?1 AND l.target_id = ?2
          ORDER BY p.title, l.position",
    )?;
    Ok(stmt
        .query_map(params![target_kind, target_id], |r| {
            Ok(Backlink {
                page_id: r.get(0)?,
                page_title: r.get(1)?,
                display_text: r.get(2)?,
                position: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Light up the links that were waiting for this title.
///
/// A person writes `[[Basel III]]` before the page exists, and doc 16 section
/// 3.1 keeps the link rather than dropping it. When the page arrives, by hand
/// or from the vault, the links into it stop being unresolved. Matching is on
/// the title the link named, not on what it displayed, which is why 0007 stores
/// it.
///
/// Returns how many links resolved.
pub fn resolve_pending_links(store: &Store, kind: &str, target_id: &str, title: &str) -> Result<usize> {
    Ok(store.conn().execute(
        "UPDATE page_link SET target_kind = ?1, target_id = ?2
          WHERE target_kind = 'unresolved' AND target_title = ?3 COLLATE NOCASE",
        params![kind, target_id, title.trim()],
    )?)
}

/// The concept this title names, by term or by alias, case insensitively.
///
/// Doc 16 section 3.1: "a wikilink whose title matches a Concept term or alias
/// links to the concept". The alias match is a scan over the profile's
/// concepts, which is bounded by the glossary rather than by the vault: a
/// profile has tens of concepts and thousands of links into them.
pub fn concept_by_term_or_alias(store: &Store, profile_id: &str, title: &str) -> Result<Option<String>> {
    let conn = store.conn();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM concept WHERE profile_id = ?1 AND term = ?2 COLLATE NOCASE LIMIT 1",
            params![profile_id, title.trim()],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(Some(id));
    }

    let wanted = title.trim().to_lowercase();
    let mut stmt = conn.prepare("SELECT id, aliases FROM concept WHERE profile_id = ?1")?;
    let rows = stmt.query_map(params![profile_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    for row in rows {
        let (id, aliases) = row?;
        let Some(aliases) = aliases else { continue };
        let Ok(list) = serde_json::from_str::<Value>(&aliases) else {
            continue;
        };
        let matched = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|a| a.trim().to_lowercase() == wanted);
        if matched {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Point a card at the page it was saved as. Doc 16 section 4.
pub fn set_card_page(store: &Store, card_id: &str, page_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE card SET page_id = ?1 WHERE id = ?2",
        params![page_id, card_id],
    )?;
    Ok(())
}

// ------------------------------------------------------------- retrieval ---

/// A folder this profile has pointed a retriever at. Doc 05 section 8.2.
///
/// The row is what the local retriever reads from, so the set of retrievers a
/// profile actually has is this list plus the pack's enabled ones. Reading it
/// belongs here rather than in the core, because the core would otherwise hold
/// the only SQL outside this module and the columns would drift.
#[derive(Debug, Clone)]
pub struct WatchedFolder {
    pub id: String,
    pub root: String,
    pub label: String,
    /// Doc 10 section 16: a sensitive folder keeps its text on this machine.
    pub sensitive: bool,
    /// `local` or `provider`. Doc 10 section 3 makes provider embeddings opt in
    /// per folder.
    pub embeddings: String,
    pub last_indexed_at: Option<String>,
}

/// Every folder this profile watches, oldest first.
///
/// The boards index lives in the same table because it is an index like any
/// other (doc 05 section 8.5), and it is returned here with the rest; callers
/// that mean folders on disk filter it out by id.
pub fn watched_folders(store: &Store, profile_id: &str) -> Result<Vec<WatchedFolder>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, root, label, sensitive, embeddings, last_indexed_at
           FROM watched_folder
          WHERE profile_id = ?1
          ORDER BY created_at, id",
    )?;
    Ok(stmt
        .query_map(params![profile_id], |r| {
            Ok(WatchedFolder {
                id: r.get(0)?,
                root: r.get(1)?,
                label: r.get(2)?,
                sensitive: r.get::<_, i64>(3)? == 1,
                embeddings: r.get(4)?,
                last_indexed_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Where a retrieval assignment sits. Doc 05 sections 4 and 7.
#[derive(Clone, Copy)]
pub struct RetrievalRef<'a> {
    pub run_id: &'a str,
    pub board_id: &'a str,
    pub card_id: &'a str,
    pub retriever_id: &'a str,
    pub sq_id: Option<&'a str>,
}

/// One passage a retriever found, with the source it came from.
pub struct NewPassage<'a> {
    pub class: &'a str,
    pub title: &'a str,
    pub locator: &'a str,
    pub issuer: Option<&'a str>,
    pub published_at: Option<&'a str>,
    pub freshness_class: &'a str,
    pub trust_rank: i64,
    pub version_ref: Option<&'a str>,
    pub content_hash: &'a str,
    pub text: &'a str,
    pub location: Value,
    /// Doc 01 open question 2, resolved: a folder marked sensitive stores
    /// offsets rather than verbatim text, and its passages are blocked from
    /// export. The text never reaches the row.
    pub text_withheld: bool,
}

/// What persisting a retrieval produced.
#[derive(Debug, Default, Clone)]
pub struct Retained {
    pub passage_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub sources_created: usize,
    pub sources_deduplicated: usize,
    /// Why each passage's source is stale, in passage order, or `None` where it
    /// is not. A source reached again after a re-verification marked it stale is
    /// still stale, and the Verifier's freshness check reads this rather than
    /// assuming that anything just retrieved is current.
    pub stale: Vec<Option<String>>,
}

/// Doc 05 section 7's `retrieval.started.v1`, emitted before anything is
/// fetched so the audit trail shows an assignment that hung as well as one that
/// returned.
pub fn start_retrieval(store: &mut Store, at: RetrievalRef<'_>, query: &str) -> Result<()> {
    store.append(
        NewEvent::new(
            "retrieval.started.v1",
            json!({
                "retriever_id": at.retriever_id,
                "sq_id": at.sq_id,
                "query": query,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}

/// Mark a cited source stale and say why. Doc 05 section 7's `source.stale.v1`,
/// carrying one of the three reasons that section names: `content_changed`,
/// `locator_gone`, `superseded_version`.
///
/// The number of cards citing the source rides on the event, because doc 07
/// section B14 open question 2 makes a batch of stale citations one notice
/// rather than one per card. Returns that count, so a caller re-verifying a
/// whole corpus can report what it touched.
///
/// Marking is idempotent: re-verifying a source already stale for the same
/// reason writes the same row and appends no second event, so a run repeated
/// against an unchanged corpus does not fill the log with duplicates.
pub fn mark_source_stale(store: &mut Store, source_id: &str, reason: &str, run_id: &str) -> Result<usize> {
    let already: Option<String> = store
        .conn()
        .query_row(
            "SELECT stale_reason FROM source WHERE id = ?1 AND stale = 1",
            params![source_id],
            |r| r.get(0),
        )
        .optional()?;
    let affected: i64 = store.conn().query_row(
        "SELECT COUNT(DISTINCT c.card_id)
           FROM citation c JOIN passage p ON p.id = c.passage_id
          WHERE p.source_id = ?1",
        params![source_id],
        |r| r.get(0),
    )?;
    let affected = affected.max(0) as usize;

    if already.as_deref() == Some(reason) {
        return Ok(affected);
    }

    let owned = (source_id.to_string(), reason.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "source.stale.v1",
            json!({
                "source_id": source_id,
                "reason": reason,
                "affected_cards": affected,
            }),
            Provenance::retriever("reverify", run_id),
        ),
        move |tx| {
            let (sid, reason, now) = owned;
            tx.execute(
                "UPDATE source SET stale = 1, stale_reason = ?2, last_verified_at = ?3
                  WHERE id = ?1",
                params![sid, reason, now],
            )?;
            Ok(())
        },
    )?;
    Ok(affected)
}

/// Persist what a retriever found, and say so. Doc 05 sections 5 and 7.
///
/// Sources are deduplicated on the normalised locator, so a page reached twice
/// through two spellings is one row and one `source.deduplicated.v1`. That is
/// what doc 05 section 12's zero-duplicates gate measures, and doing it here
/// rather than in each connector means a connector cannot forget.
pub fn record_retrieval(
    store: &mut Store,
    profile_id: &str,
    at: RetrievalRef<'_>,
    passages: &[NewPassage<'_>],
    coverage: &str,
    latency_ms: u128,
) -> Result<Retained> {
    let now = now_iso8601();
    let mut retained = Retained::default();

    for passage in passages {
        let dedupe = normalise_locator(passage.locator);

        let existing: Option<(String, Option<String>)> = store
            .conn()
            .query_row(
                "SELECT id, CASE WHEN stale = 1 THEN stale_reason END
                   FROM source WHERE profile_id = ?1 AND dedupe_key = ?2",
                params![profile_id, dedupe],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let (id, created, stale_reason) = match existing {
            Some((id, reason)) => (id, false, reason),
            None => (new_id(), true, None),
        };
        retained.stale.push(stale_reason);

        if created {
            let owned = (
                id.clone(),
                profile_id.to_string(),
                passage.class.to_string(),
                passage.title.to_string(),
                passage.locator.to_string(),
                passage.issuer.map(str::to_string),
                passage.published_at.map(str::to_string),
                passage.freshness_class.to_string(),
                passage.version_ref.map(str::to_string),
                passage.content_hash.to_string(),
                dedupe.clone(),
                now.clone(),
            );
            let rank = passage.trust_rank;
            store.append_with(
                NewEvent::new(
                    "source.created.v1",
                    json!({
                        "source_id": id,
                        "class": passage.class,
                        "locator": passage.locator
                    }),
                    Provenance::retriever(at.retriever_id, at.run_id),
                )
                .on_board(at.board_id)
                .on_card(at.card_id),
                move |tx| {
                    let (
                        sid,
                        profile,
                        class,
                        title,
                        locator,
                        issuer,
                        published,
                        freshness,
                        version,
                        hash,
                        dedupe,
                        now,
                    ) = owned;
                    tx.execute(
                        "INSERT INTO source (id, profile_id, class, title, locator, site_or_issuer,
                             published_at, retrieved_at, last_verified_at, content_hash,
                             freshness_class, trust_rank, dedupe_key, version_ref, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, ?12, ?13, ?8)",
                        params![
                            sid, profile, class, title, locator, issuer, published, now, hash, freshness,
                            rank, dedupe, version
                        ],
                    )?;
                    Ok(())
                },
            )?;
            retained.sources_created += 1;
        } else {
            store.append(
                NewEvent::new(
                    "source.deduplicated.v1",
                    json!({ "source_id": id, "locator": passage.locator }),
                    Provenance::retriever(at.retriever_id, at.run_id),
                )
                .on_board(at.board_id)
                .on_card(at.card_id),
            )?;
            retained.sources_deduplicated += 1;
        }

        // A withheld passage keeps its location and loses its text, which is
        // what makes a citation into a sensitive folder checkable by the person
        // who owns the folder and useless to anyone a bundle reaches.
        let text: Option<String> = (!passage.text_withheld).then(|| passage.text.to_string());
        let passage_id = new_id();
        store.conn().execute(
            "INSERT INTO passage (id, source_id, text, location, retrieved_in_run, retrieved_by,
                 text_withheld, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                passage_id,
                id,
                text,
                passage.location.to_string(),
                at.run_id,
                at.retriever_id,
                i64::from(passage.text_withheld),
                now
            ],
        )?;

        retained.passage_ids.push(passage_id);
        if !retained.source_ids.contains(&id) {
            retained.source_ids.push(id);
        }
    }

    store.append(
        NewEvent::new(
            "retrieval.completed.v1",
            json!({
                "retriever_id": at.retriever_id,
                "sq_id": at.sq_id,
                "passage_ids": retained.passage_ids,
                "source_ids": retained.source_ids,
                "coverage": coverage,
                "fetches": passages.len(),
                "latency_ms": latency_ms as u64,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;

    Ok(retained)
}

/// Doc 05 section 7's `hook.denied.v1`.
///
/// A denial names the category and never the item. Doc 05 section 10: the card
/// caveat names the exclusion category without naming the excluded thing,
/// because the whole point of an exclusion is that its contents do not leave.
pub fn record_hook_denial(
    store: &mut Store,
    at: RetrievalRef<'_>,
    hook_id: &str,
    category: &str,
) -> Result<()> {
    store.append(
        NewEvent::new(
            "hook.denied.v1",
            json!({
                "retriever_id": at.retriever_id,
                "hook_id": hook_id,
                "target": category,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}
