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
                })
            },
        )
        .optional()?;

    let Some(mut board) = board else { return Ok(None) };
    board.cards = read_cards(store, board_id)?;
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
                c.produced_by, c.position, c.visual_id, c.builds_on
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
        self.citations.iter().filter(|c| c["stale"] == json!(true)).count()
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
        .query_row("SELECT parent_card_id FROM card WHERE id = ?1", params![card_id], |r| r.get(0))
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
        "SELECT c.ordinal, s.title, s.class, s.locator, c.verifier_verdict, s.stale, p.text
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

// ------------------------------------------------------------- retrieval ---

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
pub fn mark_source_stale(
    store: &mut Store,
    source_id: &str,
    reason: &str,
    run_id: &str,
) -> Result<usize> {
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
                            sid, profile, class, title, locator, issuer, published, now, hash,
                            freshness, rank, dedupe, version
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
