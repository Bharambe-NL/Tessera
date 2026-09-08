//! The card run: Router, Planner, retrievers, Synthesizer, Visualizer, Verifier.
//!
//! One question in, one answered card out. The stages are doc 03 section 2's
//! deep path in order, and each writes its Step before the next one starts.

use serde_json::{Value, json};
use tessera_agents::{Planner, Router, Synthesizer, Verifier, Visualizer};
use tessera_harness::{Failure, Recovery, RunAgent, run_agent};
use tessera_store::{Store, repo};

use super::packets::{build_planner_packet, build_router_packet, build_synth_packet};
use super::support::{LEARN, LESSON_FETCH_BUDGET, fail};
use super::{CardIdentity, CardOutcome, RunContext, Subject};

/// How much of each kind of structure the Synthesizer returned.
///
/// Counts rather than content: the shape is what doc 06 section B8 point 1
/// selects a visual type from, and the answer itself is already in the record.
fn summary_shape(summary: &Value) -> Value {
    let len = |key: &str| {
        summary
            .get(key)
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0)
    };
    json!({
        "entities": len("entities"),
        "relations": len("relations"),
        "values": len("values"),
        "steps": len("steps"),
        "groups": len("groups"),
    })
}

/// Doc 17 section 8: a pack ranks sources for learning differently from how it
/// ranks them for answering.
///
/// `source_hierarchy` answers "who has authority over this claim"; the learning
/// quality ranking answers "who explains it best". They are different questions
/// and a pack gives different answers, so a lesson reads the second. A pack
/// that declares no quality ranking falls back to the first, which is the
/// honest degrade: one ranking is better than none.
fn research_ranks(ctx: &RunContext<'_>, learning: bool) -> Vec<Value> {
    let quality = &ctx.pack.learning_templates.quality_ranking;
    if learning && !quality.classes.is_empty() {
        return quality
            .classes
            .iter()
            .enumerate()
            .map(|(i, class)| {
                json!({
                    "class": class,
                    "issuer_pattern": Value::Null,
                    // Best first, and rank 1 is the best doc 01 section 4.8
                    // has, so the position in the list is the rank.
                    "rank": i + 1,
                })
            })
            .chain(quality.issuer_patterns.iter().map(|pattern| {
                // Doc 17 section 8's "issuers a lesson reaches for first". An
                // issuer rule outranks a bare class by being more specific,
                // which `Doctrine::rank_for` already knows.
                json!({ "class": Value::Null, "issuer_pattern": pattern, "rank": 1 })
            }))
            .collect();
    }
    ctx.pack
        .source_hierarchy
        .iter()
        .map(|r| {
            json!({
                "class": r.class,
                "issuer_pattern": r.issuer_pattern,
                "rank": r.trust_rank,
            })
        })
        .collect()
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
    model_override: Option<&str>,
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

    // Doc 03 section 4 hands the Router the parent card and doc 04 section 4
    // hands the Planner up to three ancestors. Both read the same chain, so it
    // is walked once here rather than twice inside the builders.
    let card = CardIdentity::read(store, card_id);
    let ancestors = repo::ancestor_chain(store, card_id, 3).unwrap_or_default();
    let subject = Subject {
        card: &card,
        run_id: &run_id,
        question,
        ancestors: &ancestors,
    };

    // ------------------------------------------------------------ Router --
    let router_packet = build_router_packet(&board, &subject, depth_override, model_override, ctx);
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
            // The run stopped before retrieval, so nothing was looked at.
            passages_seen: 0,
            unsupported: 0,
        });
    }

    // ----------------------------------------------------------- Planner --
    // Doc 04 section 3: only when the Router set plan_required, never in fast.
    let plan = if routed["plan_required"].as_bool().unwrap_or(false) {
        let planner_packet = build_planner_packet(store, &board, &routed, &subject, ctx)?;
        let planned = run_agent(
            &Planner,
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
            planner_packet,
        )
        .await;

        let planned = match planned {
            Ok(o) => o.output,
            Err(f) => return fail(store, card_id, board_id, &run_id, f),
        };

        // Doc 04 section 7: one entity.resolved.v1 per literal the Planner
        // resolved, so the Concept graph work at M9 has an audit trail to read.
        for entity in planned["resolved_entities"].as_array().into_iter().flatten() {
            store.append(
                tessera_store::NewEvent::new(
                    "entity.resolved.v1",
                    json!({
                        "card_id": card_id,
                        "literal": entity["literal"].clone(),
                        "concept_id": entity["concept_id"].clone(),
                        "ambiguity": entity["ambiguity"].clone(),
                    }),
                    tessera_store::Provenance::agent("planner", &run_id),
                )
                .on_board(board_id)
                .on_card(card_id),
            )?;
        }

        Some(planned)
    } else {
        None
    };

    // -------------------------------------------------------- Retrievers --
    // Doc 05 section 2. Fast never retrieves: doc 06 section A8 says a fast
    // card is written from model knowledge and marked unverified, and going to
    // the corpus for it would be a different product.
    //
    // With no retriever configured this returns nothing and doc 06 section A10
    // turns that into an honest "no sources" card, which is what a profile
    // that has not been pointed at a folder yet deserves.
    let mut builds_on: Vec<Value> = Vec::new();
    let passages: Vec<Value> = if mode == "fast" {
        Vec::new()
    } else {
        // Doc 17 section 5's research posture, for a card asked on a lesson
        // board. Three things change and one does not.
        //
        // The ranking changes: doc 17 section 8 gives a pack a separate quality
        // ranking for learning, because a lesson prefers a primary source that
        // explains while an answer prefers the source with authority over the
        // claim. The budget changes, because a lesson is building somebody's
        // understanding of a topic rather than answering one question. The set
        // narrows to what doc 17 section 5 names.
        //
        // What does not change is the Verifier. A research card is checked like
        // any other, which is why reaching more widely is safe to do at all.
        let learning = board["mode"].as_str() == Some(LEARN);
        let doctrine = json!({
            "trust_ranks": research_ranks(ctx, learning),
            "denied_domains": [],
        });
        let must_exclude = ctx.pack.must_exclude();
        let must_include: Vec<String> = if learning {
            tessera_store::repo::active_mission(store, &ctx.profile_id).unwrap_or(Value::Null)["sources_hint"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        } else {
            Vec::new()
        };
        let profile_id = ctx.profile_id.clone();
        let fan = crate::retrieval::run(
            store,
            ctx.ledger,
            ctx.retrievers,
            &profile_id,
            tessera_store::repo::RetrievalRef {
                run_id: &run_id,
                board_id,
                card_id,
                retriever_id: "",
                sq_id: None,
            },
            plan.as_ref(),
            question,
            &doctrine,
            &must_exclude,
            if learning {
                // The set arrives narrowed. Doc 16 section 4's notebook and doc
                // 17 section 5's lesson are both properties of the board, so
                // `Core::ask` restricts once and everything that reads the set
                // reads the same answer, the Planner packet included. What is
                // left here is what only a lesson has: the path's own locators
                // and a wider budget.
                crate::retrieval::Posture {
                    allow: None,
                    must_include: &must_include,
                    fetch_budget: Some(LESSON_FETCH_BUDGET),
                }
            } else {
                crate::retrieval::Posture::default()
            },
        );
        for caveat in &fan.caveats {
            // Doc 05 section 10: the card says a category was excluded and
            // never which item, because the item is the thing being protected.
            store.append(
                tessera_store::NewEvent::new(
                    "context.stale_noted.v1",
                    json!({ "card_id": card_id, "note": format!("excluded: {caveat}") }),
                    tessera_store::Provenance::harness("retrieval", Some(run_id.clone())),
                )
                .on_board(board_id)
                .on_card(card_id),
            )?;
        }
        builds_on = fan.builds_on;
        fan.passages
    };

    // ------------------------------------------------------- Synthesizer --
    let synth_packet = build_synth_packet(&routed, plan.as_ref(), &mode, &subject, &passages, ctx);
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
            // Doc 06 section B8 point 1 selects the visual type from the shape
            // of this summary, so a record without the shape cannot say why a
            // card got the visual it got. BN-110: the first paid run scored
            // visual_type_match 0.083 and the record could not tell whether the
            // rule reached the doctrine hint at all.
            "summary_shape": summary_shape(&synthesized["structured_summary"]),
            "unsupported_count": synthesized["unsupported_statements"].as_array().map(Vec::len).unwrap_or(0),
            // Doc 06 section A7 lists this in the payload and it was missing, so
            // the event log could not say which audience an answer was written
            // for. Null until the audience rewrite lands.
            "audience_id": synthesized["audience_applied"].clone(),
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
    } else {
        // Doc 06 section B7. A Visualizer that declined said why, and until now
        // only a Visualizer that failed left a trace. Every grounded run
        // declined every visual and the event log recorded nothing at all, so
        // the audit trail could not tell a card that wanted no diagram from one
        // whose diagram was dropped.
        store.append(
            tessera_store::NewEvent::new(
                "visual.declined.v1",
                json!({
                    "card_id": card_id,
                    "reason": visual["declined_reason"]
                        .as_str()
                        .unwrap_or("The summary carried no structure to draw."),
                }),
                tessera_store::Provenance::agent("visualizer", run_id.clone()).with_source(ctx.source),
            )
            .on_board(board_id)
            .on_card(card_id),
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
    repo::finish_card(
        store,
        at,
        confidence,
        &verdicts,
        &verified["checks_run"],
        &builds_on,
    )?;
    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    // Doc 05 section 8.5: the boards index is "updated on `card.answered.v1`".
    // Eligibility is checked inside, so a card that does not qualify is simply
    // not remembered, and one that has stopped qualifying is removed.
    let _ = tessera_retrievers::boards::index_card(
        store.conn(),
        &ctx.profile_id,
        card_id,
        ctx.retrievers.embedder.as_deref(),
    );

    // Doc 01 section 4.10: "Agents propose; the user confirms." The Router named
    // these entities at the top of the run, and until M9 they reached the log
    // and nothing else. A failure here is not the card's failure: the answer is
    // written and verified, and a graph that missed a term is a Library with one
    // fewer row rather than a card the reader loses.
    let entities: Vec<String> = routed["classification"]["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e.as_str().map(str::to_string))
        .collect();
    if !entities.is_empty()
        && let Ok(pack_id) = repo::ensure_pack(store, &serde_json::to_value(ctx.pack).unwrap_or(Value::Null))
        && let Err(e) = repo::propose_concepts(store, at, &ctx.profile_id, &pack_id, &entities, "router")
    {
        tracing::warn!(error = %e, "the concepts this card named were not proposed");
    }

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
        // Doc 16 section 3.4's three states are computed from these by the
        // caller: no passages is ungrounded, unsupported claims are partly
        // grounded, and neither is grounded.
        passages_seen: passages.len(),
        unsupported: synthesized["unsupported_statements"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 17 section 8: a pack ranks sources for learning differently from how
    /// it ranks them for answering, and the two answer different questions.
    ///
    /// The ranking is a rule over the pack rather than something a run can be
    /// asked about afterwards: a retriever packet is not persisted, so this is
    /// where the rule is readable.
    #[test]
    fn a_lesson_ranks_by_what_explains_and_a_card_by_what_has_authority() {
        let registry = tessera_schema::Registry::load().expect("schemas");
        let packs = tessera_doctrine::PackLibrary::load_built_in(&registry).expect("packs");
        let pack = packs.get("finance-eu-synthetic").expect("pack");

        let ranks = |learning: bool| -> Vec<(Option<String>, Option<String>, i64)> {
            let ctx = RunContext {
                registry: &registry,
                provider: &tessera_providers::MockProvider::new(),
                pack,
                policy: Default::default(),
                profile_id: "p".into(),
                source: tessera_store::event::Source::Test,
                ledger: &tessera_harness::Ledger::new(),
                retrievers: &crate::retrieval::RetrieverSet::default(),
            };
            research_ranks(&ctx, learning)
                .into_iter()
                .map(|r| {
                    (
                        r["class"].as_str().map(str::to_string),
                        r["issuer_pattern"].as_str().map(str::to_string),
                        r["rank"].as_i64().unwrap_or(0),
                    )
                })
                .collect()
        };

        // The learning ranking is the pack's quality ranking, best first, so a
        // class's position in that list is its rank.
        let learning = ranks(true);
        for (i, class) in pack.learning_templates.quality_ranking.classes.iter().enumerate() {
            assert!(
                learning.contains(&(Some(class.clone()), None, i as i64 + 1)),
                "{class} is not ranked {} for learning: {learning:?}",
                i + 1
            );
        }
        // Doc 17 section 8's "issuers a lesson reaches for first", which outrank
        // a bare class by being more specific.
        for pattern in &pack.learning_templates.quality_ranking.issuer_patterns {
            assert!(
                learning.contains(&(None, Some(pattern.clone()), 1)),
                "{pattern} is not reached for first: {learning:?}"
            );
        }

        // A card outside a lesson reads the source hierarchy, unchanged.
        let answering = ranks(false);
        for rule in &pack.source_hierarchy {
            assert!(
                answering.contains(&(
                    Some(rule.class.clone()),
                    rule.issuer_pattern.clone(),
                    rule.trust_rank
                )),
                "{} lost its answering rank: {answering:?}",
                rule.class
            );
        }
        assert_ne!(learning, answering, "one ranking is doing both jobs");
    }
}
