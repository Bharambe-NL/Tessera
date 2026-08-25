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
//! This build runs the fast path and the tail of the deep path. The Planner and
//! the retrievers arrive at M5 and M6, so a deep card reaches the Synthesizer
//! with no passages, and doc 06 section A10 says exactly what happens then: an
//! answer that reports no sources, an empty citation set, confidence 0. It never
//! falls back to model knowledge silently, so the deep path is honest rather
//! than pretending to work.

use serde_json::{Value, json};
use tessera_agents::{Router, Synthesizer, Verifier, Visualizer};
use tessera_doctrine::DoctrinePack;
use tessera_harness::{Failure, Recovery, RunAgent, run_agent};
use tessera_providers::{ModelProvider, ResolvedPolicy};
use tessera_schema::Registry;
use tessera_store::{Source, Store, repo};

/// Everything one card run needs that is not the store.
pub struct RunContext<'a> {
    pub registry: &'a Registry,
    pub provider: &'a dyn ModelProvider,
    pub pack: &'a DoctrinePack,
    pub policy: ResolvedPolicy,
    pub profile_id: String,
    pub source: Source,
}

pub struct CardOutcome {
    pub card_id: String,
    pub run_id: String,
    pub status: String,
    pub confidence: f64,
    pub flags: usize,
}

/// Run one card from request to answer.
///
/// Every stage writes its Step and its events as it completes, so a card that
/// dies halfway leaves a readable trail rather than nothing.
pub async fn run_card(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    card_id: &str,
    question: &str,
    depth_override: Option<&str>,
) -> Result<CardOutcome, Failure> {
    let board = board_row(store, board_id)?;
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);
    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: Some(card_id),
            kind: "card",
            depth: depth_override.or(board["default_depth"].as_str()),
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;
    let at = repo::CardRef {
        card_id,
        board_id,
        run_id: &run_id,
    };

    let mut sequence = 0i64;
    let mut next_seq = || {
        sequence += 1;
        sequence
    };

    // ------------------------------------------------------------ Router --
    let router_packet = build_router_packet(&board, card_id, &run_id, question, depth_override, ctx);
    let routed = run_agent(
        &Router,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.to_string()),
            board_id: Some(board_id.to_string()),
            sequence: next_seq(),
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        router_packet.clone(),
    )
    .await;

    let routed = match routed {
        Ok(o) => o.output,
        Err(f) => return fail(store, card_id, board_id, &run_id, f),
    };

    let mode = routed["depth"]["chosen"].as_str().unwrap_or("fast").to_string();
    let early_flags = routed["early_flags"].clone();

    // Doc 03 section 7 emits flag.raised.v1 for every early flag, whatever its
    // severity. A warn flag travels on to the Synthesizer and the Verifier and
    // still belongs in the queue, so it is written here rather than only when
    // the run is about to stop.
    for flag in early_flags.as_array().into_iter().flatten() {
        repo::write_flag(
            store,
            at,
            repo::NewFlag {
                rule_id: flag["rule_id"].as_str().unwrap_or("unknown"),
                severity: flag["severity"].as_str().unwrap_or("info"),
                target: json!({ "kind": "whole_card" }),
                reason: flag["reason"]
                    .as_str()
                    .unwrap_or("A doctrine rule matched the request."),
                evidence: Some(flag["evidence"].clone()),
            },
        )?;
    }

    // Doc 03 section 10 `override_conflict`: a block severity early flag wins
    // over the depth override, and the run stops before any spend.
    if early_flags
        .as_array()
        .is_some_and(|f| f.iter().any(|x| x["severity"] == "block"))
    {
        repo::end_run(store, &run_id, "cancelled")?;
        return Ok(CardOutcome {
            card_id: card_id.to_string(),
            run_id,
            status: "flagged".into(),
            confidence: 0.0,
            flags: early_flags.as_array().map(Vec::len).unwrap_or(0),
        });
    }

    // ------------------------------------------------- Planner, retrievers --
    // M5 and M6. Until then a deep card carries no passages, and doc 06 section
    // A10 turns that into an honest "no sources" card rather than a guess.
    let passages: Vec<Value> = Vec::new();

    // ------------------------------------------------------- Synthesizer --
    let synth_packet = build_synth_packet(&routed, &mode, question, &passages, ctx);
    let synthesized = run_agent(
        &Synthesizer,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.to_string()),
            board_id: Some(board_id.to_string()),
            sequence: next_seq(),
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        synth_packet,
    )
    .await;

    let synthesized = match synthesized {
        Ok(o) => o.output,
        Err(f) => return fail(store, card_id, board_id, &run_id, f),
    };

    let produced_by = json!({
        "agent_id": "synthesizer",
        "model_alias": ctx.policy.get("synthesize").map(|s| s.alias.clone()),
        "provider": ctx.provider.id(),
        "run_id": run_id,
    });

    repo::write_answer(
        store,
        at,
        synthesized["answer"].as_str().unwrap_or_default(),
        &synthesized["findings"],
        &produced_by,
        json!({
            "card_id": card_id,
            "mode": mode,
            "citation_count": synthesized["citations"].as_array().map(Vec::len).unwrap_or(0),
            "conflict_count": synthesized["conflicts"].as_array().map(Vec::len).unwrap_or(0),
            "unsupported_count": synthesized["unsupported_statements"].as_array().map(Vec::len).unwrap_or(0),
            "advice_handling": synthesized["advice_handling"].clone(),
        }),
    )?;

    // Citations, each with its source and passage, in one transaction each.
    for citation in synthesized["citations"].as_array().into_iter().flatten() {
        let ordinal = citation["n"].as_i64().unwrap_or(0);
        let Some(passage) = passages.get((ordinal.max(1) - 1) as usize) else {
            continue;
        };
        repo::write_citation(
            store,
            &ctx.profile_id,
            at,
            repo::NewCitation {
                ordinal,
                source_title: passage["source"]["title"].as_str().unwrap_or("A source"),
                source_class: passage["source"]["class"].as_str().unwrap_or("web"),
                locator: passage["source"]["locator"].as_str().unwrap_or(""),
                issuer: passage["source"]["issuer"].as_str(),
                freshness_class: passage["source"]["freshness_class"].as_str().unwrap_or("general"),
                trust_rank: ctx.pack.trust_rank(
                    passage["source"]["class"].as_str().unwrap_or("web"),
                    passage["source"]["issuer"].as_str(),
                ),
                passage_text: passage["text"].as_str().unwrap_or(""),
                claim_span: citation["claim_span"].clone(),
                binding: citation["binding"].as_str().unwrap_or("answer"),
            },
        )?;
    }

    // -------------------------------------------------------- Visualizer --
    let vis_packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "structured_summary": synthesized["structured_summary"].clone(),
        "citations": synthesized["citations"].clone(),
        "visual_hint": routed["visual_hint"].clone(),
        "question_type": routed["classification"]["question_type"].clone(),
        "audience_id": routed["classification"]["audience_id"].clone(),
        "doctrine": {
            "type_preferences": ctx.pack.visual_preferences.type_preferences,
            "max_nodes": ctx.pack.visual_preferences.max_nodes,
            "max_rows": ctx.pack.visual_preferences.max_rows
        },
        "effort_budget": { "max_tokens": 1500 }
    });

    let visual = run_agent(
        &Visualizer,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.to_string()),
            board_id: Some(board_id.to_string()),
            sequence: next_seq(),
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        vis_packet,
    )
    .await;

    // Doc 06 section B10: a card without a visual is acceptable. A Visualizer
    // failure degrades the card, it does not kill it.
    let visual = match visual {
        Ok(o) => o.output,
        Err(f) => {
            store.append(
                tessera_store::NewEvent::new(
                    "visual.declined.v1",
                    json!({ "card_id": card_id, "reason": f.detail }),
                    tessera_store::Provenance::agent("visualizer", run_id.clone()).with_source(ctx.source),
                )
                .on_board(board_id)
                .on_card(card_id),
            )?;
            json!({ "type": "none", "block_index": [] })
        }
    };

    if visual["type"] != "none" {
        repo::write_visual(
            store,
            at,
            visual["type"].as_str().unwrap_or("list"),
            visual["title"].as_str().unwrap_or("Summary"),
            &visual["payload"],
            &visual["block_index"],
            &json!({
                "agent_id": "visualizer",
                "model_alias": ctx.policy.get("visualize").map(|s| s.alias.clone()),
                "provider": ctx.provider.id(),
                "run_id": run_id,
            }),
        )?;
    }

    // ----------------------------------------------------------- Verifier --
    let verify_packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "mode": mode,
        "kind": "root",
        "answer": synthesized["answer"].clone(),
        "findings": synthesized["findings"].clone(),
        "citations": synthesized["citations"].clone(),
        "passages": passages,
        "visual": visual,
        "structured_summary": synthesized["structured_summary"].clone(),
        "unsupported_statements": synthesized["unsupported_statements"].clone(),
        "early_flags": early_flags,
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
            sequence: next_seq(),
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        verify_packet,
    )
    .await;

    // Doc 07 section B10: fail closed. A Verifier that could not run leaves a
    // block flag, never an admitted card.
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
                        "Verification did not complete, so this card is held back. {}",
                        f.detail
                    ),
                    evidence: Some(json!({ "failure": f.kind, "detail": f.detail })),
                },
            )?;
            json!({
                "citation_verdicts": [], "flags": [], "block_actions": [],
                "card_confidence": 0.0, "card_status": "flagged", "checks_run": []
            })
        }
    };

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
    }

    let verdicts: Vec<(i64, String)> = verified["citation_verdicts"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| Some((x["n"].as_i64()?, x["verdict"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let confidence = verified["card_confidence"].as_f64().unwrap_or(0.0);
    repo::finish_card(store, at, confidence, &verdicts, &verified["checks_run"])?;
    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    let open_flags = verified["flags"].as_array().map(Vec::len).unwrap_or(0);
    Ok(CardOutcome {
        card_id: card_id.to_string(),
        run_id,
        status: if verified["flags"].as_array().is_some_and(|f| {
            f.iter()
                .any(|x| matches!(x["severity"].as_str(), Some("warn" | "block")))
        }) {
            "flagged".into()
        } else {
            "done".into()
        },
        confidence,
        flags: open_flags,
    })
}

fn fail(
    store: &mut Store,
    card_id: &str,
    board_id: &str,
    run_id: &str,
    f: Failure,
) -> Result<CardOutcome, Failure> {
    let detail = serde_json::to_value(&f).unwrap_or(json!({ "type": f.kind.clone() }));
    repo::fail_card(store, card_id, board_id, &detail)?;
    repo::end_run(store, run_id, "failed")?;
    Err(f)
}

fn board_row(store: &Store, board_id: &str) -> Result<Value, Failure> {
    store
        .conn()
        .query_row(
            "SELECT b.id, b.title, b.default_depth, b.seed_label, b.context, b.mode, p.code, p.version
             FROM board b JOIN doctrine_pack p ON p.id = b.doctrine_pack_id WHERE b.id = ?1",
            rusqlite::params![board_id],
            |r| {
                Ok(json!({
                    "board_id": r.get::<_, String>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "default_depth": r.get::<_, String>(2)?,
                    "seed_label": r.get::<_, Option<String>>(3)?,
                    "context": r.get::<_, Option<String>>(4)?,
                    "mode": r.get::<_, String>(5)?,
                    "doctrine_pack": { "code": r.get::<_, String>(6)?, "version": r.get::<_, String>(7)? }
                }))
            },
        )
        .map_err(|e| {
            Failure::new(
                "packet_invalid",
                format!("the board is missing: {e}"),
                Recovery::Failed,
            )
        })
}

fn build_router_packet(
    board: &Value,
    card_id: &str,
    run_id: &str,
    question: &str,
    depth_override: Option<&str>,
    ctx: &RunContext<'_>,
) -> Value {
    json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "request": {
            "text": question,
            "kind": "root",
            "anchor_text": null,
            "anchor_block_ref": null,
            "depth_override": depth_override,
            "model_override": null,
            "audience_override": null
        },
        "board": board,
        "parent": null,
        "profile": {
            "role": null,
            "default_depth": board["default_depth"].clone(),
            "model_policy": {}
        },
        "doctrine": {
            "audiences": serde_json::to_value(&ctx.pack.audiences).unwrap_or(json!([])),
            "domains": serde_json::to_value(&ctx.pack.domains).unwrap_or(json!([])),
            "domain_vocabulary": serde_json::to_value(&ctx.pack.domain_vocabulary).unwrap_or(json!({})),
            "sensitivity_rules": serde_json::to_value(&ctx.pack.sensitivity_rules).unwrap_or(json!([])),
            "depth_hints": serde_json::to_value(&ctx.pack.depth_hints).unwrap_or(json!({})),
            "type_preferences": serde_json::to_value(&ctx.pack.visual_preferences.type_preferences).unwrap_or(json!({}))
        },
        "recent": [],
        "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
    })
}

fn build_synth_packet(
    routed: &Value,
    mode: &str,
    question: &str,
    passages: &[Value],
    ctx: &RunContext<'_>,
) -> Value {
    json!({
        "schema_version": "1.0",
        "run_id": routed["run_id"].clone(),
        "mode": mode,
        "request": { "text": question, "kind": "root", "anchor_text": null },
        "plan": Value::Null,
        "passages": passages,
        "ancestors": [],
        "flags": routed["early_flags"].clone(),
        "audience": Value::Null,
        "writing_rules": serde_json::to_value(&ctx.pack.writing_rules).unwrap_or(json!({})),
        "profile": { "role": null, "context": null },
        "standing_instructions": Value::Null,
        "effort_budget": {
            "max_tokens": 3000,
            "answer_max_words": if mode == "fast" { 140 } else { 180 },
            "findings_max": 5
        }
    })
}
