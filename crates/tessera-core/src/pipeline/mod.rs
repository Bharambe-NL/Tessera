//! The card pipeline.
//!
//! Doc 03 section 2:
//!
//! ```text
//! user / harness ──► Router ──► Planner ──► Retrievers ──► Synthesizer ──► Visualizer ──► Verifier ──► card
//!                       │
//!                       └──(fast path)──────────────────► Synthesizer ──► Visualizer ──► Verifier ──► card
//! ```
//!
//! This build runs the fast path and the deep path through the Planner. The
//! retrievers arrive at M6, so a planned card still reaches the Synthesizer
//! with no passages, and doc 06 section A10 says exactly what happens then: an
//! answer that reports no sources, an empty citation set, confidence 0. It never
//! falls back to model knowledge silently, so the deep path is honest rather
//! than pretending to work.
//!
//! One boundary between two specs is worth naming. Doc 06 section A10 covers
//! retrieval that found nothing; doc 04 section 10 covers having nothing to
//! retrieve with. A profile with no retriever enabled fails the card with a
//! pointer at the fix ("enable at least web or local in Profile") rather than
//! producing a card that quietly never could have had sources.
//!
//! The runs live one to a file: `card` for the card run above, `learn` for the
//! Tutor and what a check does to a concept, `read` for the Reader and
//! `exercise` for the exercise. `packets` holds what the card run hands its
//! agents and `support` the few pieces more than one of them reads. This file
//! keeps what the runs share, and the re-verification, which belongs to none of
//! them because it runs no stage at all.

mod card;
mod exercise;
mod learn;
mod packets;
mod read;
mod support;

use serde_json::{Value, json};
use tessera_agents::Verifier;
use tessera_doctrine::DoctrinePack;
use tessera_harness::{Failure, RunAgent, run_agent};
use tessera_providers::{ModelProvider, ResolvedPolicy};
use tessera_schema::Registry;
use tessera_store::{Source, Store, repo};

pub use card::run_card;
pub use exercise::{ExerciseOutcome, run_exercise};
pub use learn::{Adaptation, Ladder, end_learn_session, record_check, run_learning_planner, run_tutor_turn};
pub use read::run_read;
pub use support::{LEARN, LESSON_FETCH_BUDGET, LESSON_RETRIEVERS, base64};

/// Everything one card run needs that is not the store.
pub struct RunContext<'a> {
    pub registry: &'a Registry,
    pub provider: &'a dyn ModelProvider,
    pub pack: &'a DoctrinePack,
    pub policy: ResolvedPolicy,
    pub profile_id: String,
    pub source: Source,
    /// Doc 10 section 6's limit on retriever assignments in flight.
    pub ledger: &'a tessera_harness::Ledger,
    /// What this profile can retrieve from. Empty until a folder is watched,
    /// which is why a fresh profile answers honestly rather than emptily.
    pub retrievers: &'a crate::retrieval::RetrieverSet,
}

/// What a card is, as opposed to what it asks.
///
/// Both packets carry `kind`, `anchor_text` and `anchor_block_ref`, and both
/// had them hardcoded to `root` and null. A branch spawned from a highlighted
/// phrase looked identical to a question typed from nothing, so doc 03 step 6,
/// "a branch inherits the parent's depth", could never fire.
struct CardIdentity {
    id: String,
    kind: String,
    anchor_text: Option<String>,
    anchor_block_ref: Option<String>,
}

/// What both packet builders need to know about the run in front of them.
///
/// Grouped rather than passed as five positional arguments, because the Router
/// and the Planner want the same five and a builder that took them separately
/// grew past the point where the order was obvious.
struct Subject<'a> {
    card: &'a CardIdentity,
    run_id: &'a str,
    question: &'a str,
    /// Nearest first, capped at doc 04 section 4's three.
    ancestors: &'a [repo::Ancestor],
}

impl CardIdentity {
    fn read(store: &Store, card_id: &str) -> Self {
        store
            .conn()
            .query_row(
                "SELECT id, kind, anchor_text, anchor_block_ref FROM card WHERE id = ?1",
                rusqlite::params![card_id],
                |r| {
                    Ok(Self {
                        id: r.get(0)?,
                        // The card table and the router packet schema name the
                        // Reader's kind differently, `read` against
                        // `read_follow`. Mapping it here keeps the schema guard
                        // guarding rather than rejecting a card the Reader will
                        // legitimately produce at M10.
                        kind: match r.get::<_, String>(1)?.as_str() {
                            "read" => "read_follow".into(),
                            other => other.to_string(),
                        },
                        anchor_text: r.get(2)?,
                        anchor_block_ref: r.get(3)?,
                    })
                },
            )
            .unwrap_or_else(|_| Self {
                id: card_id.to_string(),
                kind: "root".into(),
                anchor_text: None,
                anchor_block_ref: None,
            })
    }
}

pub struct CardOutcome {
    pub card_id: String,
    pub run_id: String,
    pub status: String,
    pub confidence: f64,
    pub flags: usize,
    /// How many passages retrieval put in front of the Synthesizer.
    ///
    /// Doc 16 section 3.4's ungrounded state is `no_passages`, and the caller
    /// cannot tell that from the citations: a card can retrieve ten passages
    /// and cite none. Zero here is the vault having nothing to say.
    pub passages_seen: usize,
    /// Statements the Synthesizer could not support. Doc 16 section 3.4's
    /// partly grounded state is exactly "some claims unsupported".
    pub unsupported: usize,
}

/// Re-verify a card that was answered earlier, against the corpus as it stands
/// now. Doc 07 section B3 and B8.4.
///
/// Nothing is retrieved and nothing is rewritten. The card's own citations are
/// read back with the current state of the sources behind them, so a source a
/// re-verification marked stale reaches the Verifier's freshness check and can
/// flip a done card to flagged months after it was written.
///
/// The answer is not re-synthesised, so the deterministic checks that read the
/// draft run against the text the card already carries.
pub async fn run_verify_only(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    card_id: &str,
) -> Result<CardOutcome, Failure> {
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);
    let card = repo::read_card_for_verify(store, card_id)
        .map_err(|e| Failure::fail_closed("verify_only", e.to_string()))?
        .ok_or_else(|| Failure::fail_closed("verify_only", "no such card"))?;

    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: Some(card_id),
            kind: "verify_only",
            depth: Some(&card.depth),
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;
    let at = repo::CardRef {
        card_id,
        board_id,
        run_id: &run_id,
    };

    // Doc 07 section B5: fast mode runs only the checks that need no passages,
    // and this run has passages, so the mode the card was written at stands.
    let packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "mode": card.depth,
        "kind": "verify_only",
        "answer": card.answer,
        "findings": card.findings,
        "citations": card.citations,
        "passages": card.passages,
        "visual": Value::Null,
        "unsupported_statements": json!([]),
        "early_flags": json!([]),
        "plan_constraints": { "must_exclude": ctx.pack.must_exclude(), "value_policy": "cite_only" },
        "doctrine": {
            "flag_rules": serde_json::to_value(&ctx.pack.flag_rules).unwrap_or(json!([])),
            "freshness_classes": serde_json::to_value(&ctx.pack.freshness_classes).unwrap_or(json!({})),
            "writing_rules": serde_json::to_value(&ctx.pack.writing_rules).unwrap_or(json!({}))
        },
        "effort_budget": { "max_tokens": 3000, "answer_max_words": 180 }
    });

    let verified = run_agent(
        &Verifier,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.to_string()),
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    // Fail closed, as doc 07 section B10 requires everywhere the Verifier runs.
    // A re-verification that could not complete leaves the card held back rather
    // than quietly confirming it.
    let verified = match verified {
        Ok(o) => o.output,
        Err(f) => {
            repo::write_flag(
                store,
                at,
                repo::NewFlag {
                    rule_id: "verification_failed",
                    severity: "block",
                    target: json!({ "kind": "whole_card" }),
                    reason: &format!(
                        "Re-verification did not complete, so this card is held back. {}",
                        f.detail
                    ),
                    evidence: Some(json!({ "failure": f.kind, "detail": f.detail })),
                },
            )?;
            json!({
                "flags": [], "card_confidence": 0.0, "card_status": "flagged"
            })
        }
    };

    let mut flags = 0usize;
    for flag in verified["flags"].as_array().into_iter().flatten() {
        repo::write_flag(
            store,
            at,
            repo::NewFlag {
                rule_id: flag["rule_id"].as_str().unwrap_or("unknown"),
                severity: flag["severity"].as_str().unwrap_or("info"),
                target: flag["target"].clone(),
                reason: flag["reason"].as_str().unwrap_or("A doctrine rule matched."),
                evidence: Some(flag["evidence"].clone()),
            },
        )?;
        flags += 1;
    }

    let status = verified["card_status"].as_str().unwrap_or("flagged").to_string();
    let confidence = verified["card_confidence"].as_f64().unwrap_or(0.0);
    store
        .conn()
        .execute(
            "UPDATE card SET status = ?1, confidence = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![status, confidence, tessera_store::now_iso8601(), card_id],
        )
        .map_err(|e| Failure::fail_closed("verify_only", e.to_string()))?;

    store.append(
        tessera_store::NewEvent::new(
            "verify.completed.v1",
            json!({ "card_id": card_id, "status": status, "flags": flags, "kind": "verify_only" }),
            tessera_store::Provenance::agent("verifier", run_id.clone()).with_source(ctx.source),
        )
        .on_board(board_id)
        .on_card(card_id),
    )?;
    repo::end_run(store, &run_id, "done")?;

    Ok(CardOutcome {
        card_id: card_id.to_string(),
        run_id,
        status,
        confidence,
        flags,
        // A re-verification retrieves nothing. Doc 16 section 3.4's states are
        // about what a question found, and this asked none.
        passages_seen: 0,
        unsupported: 0,
    })
}
