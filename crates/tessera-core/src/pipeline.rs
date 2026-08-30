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

use serde_json::{Value, json};
use tessera_agents::{Planner, Router, Synthesizer, Verifier, Visualizer};
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

/// Run the Learning Planner. Doc 17 section 7.
///
/// The run and the board are the caller's, because the Planner is asked about a
/// profile and doc 17 section 6 gives that a board: the map. Nothing is written
/// here, which is the point of the agent as well: it proposes.
pub async fn run_learning_planner(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    run_id: &str,
    packet: Value,
) -> Result<Value, Failure> {
    let out = run_agent(
        &tessera_agents::LearningPlanner,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.to_string(),
            card_id: None,
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await?;
    Ok(out.output)
}

/// Run one Tutor turn. Doc 14 section 3.3.
///
/// A turn, not a session: doc 14 section 3.4's machine is a row that outlives
/// any one run, and each trigger is one decision inside it. The session is read
/// here, the agent decides, and what it decided is written back with the
/// `learn.*` event for the stage that ran.
pub async fn run_tutor_turn(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    stage: &str,
    learner_message: Option<&str>,
    target_card_id: Option<&str>,
) -> Result<Value, Failure> {
    let session = repo::read_learn_session(store, board_id)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?
        .ok_or_else(|| Failure::new("no_session", "this board has no learn session", Recovery::Failed))?;

    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);
    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: None,
            kind: "card",
            depth: None,
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;

    // Doc 17 section 4's item sourcing order, in full: the lesson board's
    // verified cards first, then verified cards anywhere on the map, and when
    // there are none the tutor is told to request one before checking. No item
    // is ever generated from unverified text, which holds because a packet that
    // carries no unverified card cannot offer one.
    let (map_concepts, map_edges) = repo::read_map(store, &ctx.profile_id).unwrap_or_default();
    let concept_rules = tessera_agents::learning::concepts_from(&map_concepts);
    let edge_rules = tessera_agents::learning::edges_from(&map_edges);
    let frontier = tessera_agents::learning::frontier(
        &concept_rules,
        &edge_rules,
        ctx.pack.learning_templates.mastered_at,
    );
    let mut plan = tessera_agents::learning_planner::plan_lesson(&frontier, &concept_rules, &edge_rules);

    // Doc 17 sections 4 and 5 answer two different questions, and a lesson
    // under way answers to the first. The Planner picks the concept a lesson
    // opens on and the rung it opens at, both from the map. Section 4 is about
    // "the next check on that concept": once a check has been asked, the lesson
    // stays on that concept and the rung moves from the check before it. The
    // frontier cannot say either, because one passed check takes a concept off
    // it and the lesson would change subject every turn.
    let mut targets: Vec<String> = plan["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if let Some(carried) = carried_target(&session, &concept_rules) {
        targets = vec![carried];
        plan["targets"] = json!(targets);
    }
    if let Some(level) = ladder_level(&session, &targets) {
        plan["level"] = json!(level);
    }

    let mut cards = repo::cards_for_tutor(store, board_id).unwrap_or_default();
    let mut sourcing = if cards.is_empty() { "none" } else { "board" };
    if cards.is_empty() {
        cards = repo::cards_for_concepts(store, &targets, board_id, 12).unwrap_or_default();
        if !cards.is_empty() {
            sourcing = "map";
        }
    }

    let concepts = repo::concepts_for_packet(store, &ctx.profile_id, 20).unwrap_or_default();
    let mastery = session["mastery"].clone();

    // Doc 14 section 3.5's card budget, counted from what the session already
    // opened rather than from a number this turn carries.
    let requested = session["opened"].as_array().map(Vec::len).unwrap_or(0);

    let templates = &ctx.pack.learning_templates;
    let packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "board_id": board_id,
        "stage": stage,
        "session": session,
        "cards": cards,
        "concepts": concepts.into_iter().map(|c| {
            let id = c["concept_id"].as_str().unwrap_or_default().to_string();
            json!({
                "concept_id": c["concept_id"],
                "term": c["term"],
                "definition": c["definition"],
                "mastery": mastery[&id].as_i64().unwrap_or(0),
            })
        }).collect::<Vec<_>>(),
        "target_card_id": target_card_id,
        // Doc 17 section 6: the Tutor's check selection comes from the
        // Planner's targets and level rather than from a free choice. The rules
        // are deterministic, so this is the Planner's answer without the
        // Planner's model call.
        "plan": plan,
        "sourcing": sourcing,
        "learner_message": learner_message,
        // Doc 14 section 6 question 2, resolved as proposed. The profile has no
        // role field yet, so this is null and intake asks for it; the day the
        // Profile page writes one, intake stops asking.
        "profile": { "role": Value::Null },
        "doctrine": {
            "curriculum_shapes": templates.curriculum_shapes,
            "mastery_threshold": templates.mastery_threshold,
            "intake_questions": templates.intake_questions,
        },
        "budget": { "cards_requested": requested, "cards_max": TUTOR_CARDS_PER_SESSION },
        "effort_budget": { "max_tokens": 1500 }
    });

    let turn = run_agent(
        &tessera_agents::Tutor,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: None,
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    let out = match turn {
        Ok(o) => o.output,
        Err(f) => {
            // Doc 14 section 3.8: the panel says so and the session pauses. The
            // board remains usable, which is why nothing here touches the cards.
            repo::end_run(store, &run_id, "failed")?;
            return Err(f);
        }
    };

    // Doc 17 section 6: a check names the concept it checks, so the shell can
    // hand it back when the answer is graded and the ladder has a row to move.
    // Stamped here rather than asked of the model: the target came from the
    // plan, and a concept the tutor named for itself would be a check about
    // something nobody put the learner on.
    let mut out = out;
    if out["check"].is_object()
        && let Some(target) = targets.first()
    {
        out["check"]["concept_id"] = json!(target);
    }

    let session_id = session["session_id"].as_str().unwrap_or_default().to_string();
    record_turn(store, board_id, &session_id, stage, &session, &out, &run_id)?;

    repo::end_run(store, &run_id, "done")?;
    Ok(out)
}

/// The concept this lesson is already checking, when there is one.
///
/// Nothing once it is mastered: the ladder has topped out and doc 17 section 4
/// has nothing further to ask about it, so the frontier picks what comes next.
fn carried_target(session: &Value, concepts: &[tessera_agents::learning::Concept]) -> Option<String> {
    let last = session["checks"].as_array()?.last()?;
    let id = last["concept_ids"].as_array()?.first()?.as_str()?;
    let done = concepts
        .iter()
        .find(|c| c.id == id)
        .is_some_and(|c| c.state.as_deref() == Some("mastered"));
    (!done).then(|| id.to_string())
}

/// Where this session's ladder stands on the concepts a lesson is targeting.
///
/// Doc 17 section 4: pass at n moves the next check to n+1, fail to n-1. The
/// last check on a target is what that reads from, and a session with no check
/// on any of them has nothing to say, so the Planner's opening rung stands.
fn ladder_level(session: &Value, targets: &[String]) -> Option<u8> {
    let checks = session["checks"].as_array()?;
    let last = checks.iter().rev().find(|check| {
        check["concept_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|c| c.as_str().is_some_and(|id| targets.iter().any(|t| t == id)))
    })?;
    let level = last["level"].as_u64().unwrap_or(1).clamp(1, 4) as u8;
    Some(tessera_agents::learning::next_level(
        Some(level),
        last["correct"] == true,
    ))
}

/// The active mission's `sources_hint`, for a board that is running a lesson.
///
/// Read here rather than carried down from the caller: the Planner packet is
/// built for every card, and a lookup that returns nothing outside a lesson is
/// cheaper to read than a parameter threaded through every path that does not
/// use it.
fn mission_sources(store: &Store, ctx: &RunContext<'_>, board_mode: &str) -> Vec<String> {
    if board_mode != LEARN {
        return Vec::new();
    }
    tessera_store::repo::active_mission(store, &ctx.profile_id).unwrap_or(Value::Null)["sources_hint"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Doc 17 section 5: "the research retriever enabled ... plus the vault and
/// boards retrievers". Local is left out on purpose: a lesson is about a topic
/// rather than about the learner's documents, and doc 17 names three.
pub const LESSON_RETRIEVERS: &[&str] = &["web", "vault", "boards"];

/// Doc 17 section 5's larger fetch budget. Doc 05 section 8.1's eight is what a
/// card gets; a lesson reads more widely because it is building somebody's
/// understanding of a topic rather than answering one question.
pub const LESSON_FETCH_BUDGET: usize = 16;

/// The board mode a lesson runs in. Doc 14 section 2.
pub const LEARN: &str = "learn";

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

/// Doc 14 section 3.5's per session card budget.
const TUTOR_CARDS_PER_SESSION: usize = 8;

/// Write what a turn decided, with the `learn.*` event doc 14 section 2 names
/// for that stage.
///
/// Two of the five stages record nothing and say so by returning early. Asking
/// the intake questions changes no session state, because the answer is what
/// changes it and `learn.intake_answered.v1` already carries that; a reply with
/// no card to open changes none either. The first version of this reached for
/// the nearest declared event and wrote `learn.check_asked.v1` for both, which
/// put two checks that were never asked into an append-only log, where anything
/// counting checks would have believed them and nothing could take them back.
fn record_turn(
    store: &mut Store,
    board_id: &str,
    session_id: &str,
    stage: &str,
    session: &Value,
    out: &Value,
    run_id: &str,
) -> Result<(), Failure> {
    let update = match stage {
        "intake" => return Ok(()),
        "building" => repo::LearnUpdate {
            actor: repo::Actor::Agent("tutor", run_id),
            session_id,
            board_id,
            status: Some("building"),
            set: vec![("plan", out["plan"]["cards"].clone())],
            event: "learn.planned.v1",
            payload: json!({
                "session_id": session_id,
                "title": out["plan"]["title"].clone(),
                "cards": out["plan"]["cards"].as_array().map(Vec::len).unwrap_or(0),
            }),
        },
        "checking" => repo::LearnUpdate {
            actor: repo::Actor::Agent("tutor", run_id),
            session_id,
            board_id,
            status: Some("checking"),
            set: vec![],
            event: "learn.check_asked.v1",
            payload: json!({
                "session_id": session_id,
                "item_id": out["check"]["item"]["id"].clone(),
                "card_id": out["check"]["item"]["source_card_id"].clone(),
            }),
        },
        _ => {
            // A reply, and possibly a card to open. Doc 14 section 3.4: the
            // learner can type at any time and the session stays where it was.
            let Some(question) = out["open"].as_str() else {
                return Ok(());
            };
            let mut opened = session["opened"].as_array().cloned().unwrap_or_default();
            opened.push(json!({ "question": question, "reason": "asked" }));
            repo::LearnUpdate {
                actor: repo::Actor::Agent("tutor", run_id),
                session_id,
                board_id,
                status: None,
                set: vec![("opened", Value::Array(opened))],
                event: "learn.card_opened.v1",
                payload: json!({
                    "session_id": session_id,
                    "reason": "asked",
                    "open": question,
                }),
            }
        }
    };

    // Doc 12's walkthrough asks for every act in board history with the right
    // actor, and these are the tutor's acts rather than the learner's: it chose
    // the plan, it wrote the check, it decided the card to open. The learner's
    // own acts keep `Provenance::user`, which is where they were already.
    repo::update_learn_session(store, update)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))
}

/// Record what a learner answered. Doc 14 sections 3.3 and 3.6.
///
/// No agent: grading one multiple choice answer needs none, the same reason doc
/// 08 section 7 has the UI record an attempt.
pub fn record_check(
    store: &mut Store,
    board_id: &str,
    item: &Value,
    picked: &str,
    concept_ids: &[String],
    ladder: &Ladder<'_>,
) -> Result<Adaptation, Failure> {
    let session = repo::read_learn_session(store, board_id)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?
        .ok_or_else(|| Failure::new("no_session", "this board has no learn session", Recovery::Failed))?;

    let correct = item["answer_id"].as_str() == Some(picked);
    let session_id = session["session_id"].as_str().unwrap_or_default().to_string();

    let mut checks = session["checks"].as_array().cloned().unwrap_or_default();
    // Doc 17 section 2.4 moves mastery onto the concept row, and the concepts
    // an item checked are what the fold needs to move it. Recorded on the check
    // rather than on the event alone, so the session's own count can be derived
    // from its transcript instead of stored a second time beside it.
    let repeated = checks
        .iter()
        .any(|c| c["item_id"] == item["id"] && c["item_id"] != Value::Null);
    // Doc 17 section 4's rung, on the check as well as on the event. The
    // adaptation rule counts consecutive failures at level 1, and a count the
    // session's own transcript cannot produce would have to be stored a second
    // time beside it.
    let level = item["level"].as_u64().unwrap_or(1).clamp(1, 4) as u8;
    checks.push(json!({
        "item_id": item["id"].clone(),
        "card_id": item["source_card_id"].clone(),
        "picked": picked,
        "correct": correct,
        "level": level,
        "concept_ids": concept_ids,
        "at": tessera_store::now_iso8601(),
    }));
    // Counted before the write, so the check being recorded is included: doc 17
    // section 4's "two fails at level 1" means this one and the one before it.
    let fails_at_one = concept_ids
        .first()
        .map(|id| trailing_fails_at_one(&checks, id))
        .unwrap_or(0);

    repo::update_learn_session(
        store,
        repo::LearnUpdate {
            actor: repo::Actor::Learner,
            session_id: &session_id,
            board_id,
            status: Some("checking"),
            // `mastery` is no longer written. Doc 17 section 2.4 keeps the
            // score on the concept, and the session's count is derived from
            // these checks by `repo::session_mastery`.
            set: vec![("checks", Value::Array(checks))],
            event: "learn.check_answered.v1",
            payload: json!({
                "session_id": session_id,
                "item_id": item["id"].clone(),
                "correct": correct,
                // Doc 17 section 9's `check.answered.v1 { correct, level }`,
                // plus the concepts the item checked. The level arrives with
                // the exercise levels at 13c; until then a check is the level 1
                // recall question the Exercise agent has been writing.
                "concept_ids": concept_ids,
                "level": level,
                "repeated": repeated,
            }),
        },
    )
    .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;

    // Doc 17 section 2.3's transitions, said by the layer that can read the
    // pack's threshold. The projection folded the score and stopped at
    // `checked` for exactly this reason, so the state moves here or not at all.
    for concept_id in concept_ids {
        let Some((was, mastery)) = concept_standing(store, concept_id)? else {
            continue;
        };
        let now = tessera_agents::learning::state_after_check(
            was.as_deref(),
            mastery.unwrap_or(0.0),
            level,
            correct,
            ladder.mastered_at,
        );
        if was.as_deref() == Some(now.as_str()) {
            continue;
        }
        store
            .append(
                tessera_store::NewEvent::new(
                    "concept.state_changed.v1",
                    json!({
                        "concept_id": concept_id,
                        "from": was,
                        "to": now.as_str(),
                        "evidence": {
                            "kind": "check",
                            "level": level,
                            "correct": correct,
                            "item_id": item["id"].clone(),
                        },
                    }),
                    tessera_store::Provenance::user(),
                )
                .on_board(board_id),
            )
            .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;
    }

    // Doc 17 section 4's ladder, for the concept the check was about. A check
    // naming no concept adapts nothing: there is no row to move and no next
    // rung to be on.
    let remedy = match concept_ids.first() {
        Some(concept_id) => {
            tessera_agents::learning::remedy(concept_id, level, correct, fails_at_one, ladder.edges)
        }
        None => tessera_agents::learning::Remedy::None,
    };

    Ok(Adaptation {
        correct,
        level,
        next_level: tessera_agents::learning::next_level(Some(level), correct),
        remedy,
    })
}

/// What a check decided, beyond whether it was right. Doc 17 section 4.
#[derive(Debug, Clone)]
pub struct Adaptation {
    pub correct: bool,
    /// The rung the check just answered stood on.
    pub level: u8,
    /// The rung the next check on this concept opens at.
    pub next_level: u8,
    pub remedy: tessera_agents::learning::Remedy,
}

impl Adaptation {
    /// The shape the shell reads. Doc 14 section 3.7: the learner sees every
    /// decision as a choice, so a remedy is offered here and never taken.
    pub fn to_json(&self) -> Value {
        let remedy = match &self.remedy {
            tessera_agents::learning::Remedy::None => json!({ "kind": "none" }),
            tessera_agents::learning::Remedy::Card { level } => {
                json!({ "kind": "card", "level": level })
            }
            tessera_agents::learning::Remedy::Prerequisite { concept_id, level } => {
                json!({ "kind": "prerequisite", "concept_id": concept_id, "level": level })
            }
        };
        json!({
            "correct": self.correct,
            "level": self.level,
            "next_level": self.next_level,
            "remedy": remedy,
        })
    }
}

/// What the ladder needs that the store cannot say: the pack's mastery
/// threshold and the map's prerequisite edges.
pub struct Ladder<'a> {
    pub mastered_at: f64,
    pub edges: &'a [tessera_agents::learning::Edge],
}

/// Consecutive failures at level 1 on this concept, most recent first.
///
/// A pass at any level, or a failure at a higher one, ends the run: doc 17
/// section 4 opens a prerequisite after two failures at the bottom rung, and a
/// learner who got one right in between is not stuck there.
fn trailing_fails_at_one(checks: &[Value], concept_id: &str) -> u32 {
    let mut count = 0;
    for check in checks.iter().rev() {
        let about = check["concept_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|c| c.as_str() == Some(concept_id));
        if !about {
            continue;
        }
        if check["correct"] == true || check["level"].as_u64().unwrap_or(1) != 1 {
            break;
        }
        count += 1;
    }
    count
}

/// A concept's state and score as they stand right now, either absent when
/// nothing has moved them yet.
type Standing = (Option<String>, Option<f64>);

/// Read one concept's standing, or nothing when the row does not exist.
fn concept_standing(store: &Store, concept_id: &str) -> Result<Option<Standing>, Failure> {
    use rusqlite::OptionalExtension;
    store
        .conn()
        .query_row(
            "SELECT learning_state, mastery FROM concept WHERE id = ?1",
            rusqlite::params![concept_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))
}

/// End a session. Doc 14 section 3.4: the board stays in explore mode with the
/// session attached, so everything the learner made survives.
pub fn end_learn_session(store: &mut Store, board_id: &str) -> Result<Value, Failure> {
    let session = repo::read_learn_session(store, board_id)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?
        .ok_or_else(|| Failure::new("no_session", "this board has no learn session", Recovery::Failed))?;

    let checks = session["checks"].as_array().cloned().unwrap_or_default();
    let correct = checks.iter().filter(|c| c["correct"] == true).count();
    let session_id = session["session_id"].as_str().unwrap_or_default().to_string();

    repo::update_learn_session(
        store,
        repo::LearnUpdate {
            actor: repo::Actor::Learner,
            session_id: &session_id,
            board_id,
            status: Some("ended"),
            set: vec![],
            event: "learn.ended.v1",
            payload: json!({
                "session_id": session_id,
                "checks": checks.len(),
                "correct": correct,
            }),
        },
    )
    .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;

    store
        .conn()
        .execute(
            "UPDATE board SET mode = 'explore' WHERE id = ?1",
            rusqlite::params![board_id],
        )
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;

    Ok(json!({
        "checks": checks.len(),
        "correct": correct,
        "mastery": session["mastery"].clone(),
    }))
}

/// Read an image into a card. Doc 07 part A.
///
/// Doc 07 section A8 point 5: the harness runs the Visualizer on the summary,
/// "so the clean visual follows the same binding rules", and then the Verifier.
/// A Reader card is a card, which is why it goes through the same two agents as
/// one the Synthesizer wrote rather than being admitted on the Reader's word.
pub async fn run_read(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    image_id: &str,
) -> Result<CardOutcome, Failure> {
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);

    let (image, bytes) = repo::read_image(store, image_id)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?
        .ok_or_else(|| Failure::new("image_unreadable", "no such image", Recovery::Failed))?;

    // Doc 01 section 4.4's `read` card kind. The question is what the reader
    // asked of the picture, and "read this" is the whole of it.
    let card_id = repo::create_card(
        store,
        repo::NewCard {
            board_id,
            parent_card_id: None,
            kind: "read",
            question: READ_QUESTION,
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )?;

    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: Some(&card_id),
            kind: "read",
            depth: Some("deep"),
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;
    let at = repo::CardRef {
        card_id: &card_id,
        board_id,
        run_id: &run_id,
    };

    let mut packet_image = image.clone();
    // The bytes ride in the packet and are never persisted with it: doc 10
    // section 7's content blocks are one call's copy, and a step row carrying a
    // base64 image would be a megabyte of audit trail per read.
    packet_image["data"] = json!(base64(&bytes));

    let packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "mode": "card",
        "image": packet_image,
        "notes_text": [],
        "board_context": { "title": board_title(store, board_id) },
        // Doctrine, not substrate. Doc 07 section A2: what to extract
        // first is the pack's business. The finance pack names figures,
        // dates and article references; a pack that names none gets the
        // model's own judgment, which is the honest default.
        "doctrine": { "extract_first": reader_extract_first(ctx.pack) },
        "effort_budget": { "max_tokens": 2500 }
    });

    let read = run_agent(
        &tessera_agents::Reader,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.clone()),
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    let read = match read {
        Ok(o) => o.output,
        Err(f) => {
            // Doc 07 section A10's `image_unreadable`: a card with the
            // description and legibility 0, not a board with nothing on it. The
            // reader pasted something and is owed an answer about it.
            repo::write_answer(
                store,
                at,
                UNREADABLE,
                &json!([]),
                &json!({ "agent_id": "reader", "run_id": run_id }),
                json!({ "card_id": card_id, "reader_failed": f.kind }),
            )?;
            repo::finish_read(
                store,
                at,
                repo::ReadResult {
                    image_id,
                    kind: "unrecognised",
                    legibility: 0.0,
                    injection_suspected: false,
                    notable: &json!([]),
                },
            )?;
            repo::finish_card(store, at, 0.0, &[], &json!([]), &[])?;
            repo::end_run(store, &run_id, "failed")?;
            return Ok(CardOutcome {
                card_id,
                run_id,
                status: "flagged".into(),
                confidence: 0.0,
                flags: 0,
                passages_seen: 0,
                unsupported: 0,
            });
        }
    };

    let structure = read["recovered_structure"].clone();
    let summary = read["structured_summary"].clone();

    // Doc 07 section A5's harness rule, checked at the boundary rather than
    // trusted from the agent that built it. `summarise` constructs values out of
    // the structure so this holds by construction; it runs because the day a
    // second path into `values` appears, this is what says so.
    let traceable = tessera_agents::reader::values_traceable(&summary, &structure);

    repo::write_answer(
        store,
        at,
        read["description"].as_str().unwrap_or(UNREADABLE),
        &json!([]),
        &json!({ "agent_id": "reader", "run_id": run_id }),
        json!({
            "card_id": card_id,
            "kind": structure["kind"].clone(),
            "legibility": read["legibility"].clone(),
            "structure_traceable": traceable,
        }),
    )?;

    repo::finish_read(
        store,
        at,
        repo::ReadResult {
            image_id,
            kind: structure["kind"].as_str().unwrap_or("unrecognised"),
            legibility: read["legibility"].as_f64().unwrap_or(0.0),
            injection_suspected: read["injection_suspected"].as_bool().unwrap_or(false),
            notable: &read["notable"],
        },
    )?;

    // Doc 07 section A10: injection is a warn flag and the run continues with
    // the block excluded, which the agent has already done.
    if read["injection_suspected"].as_bool() == Some(true) {
        repo::write_flag(
            store,
            at,
            repo::NewFlag {
                rule_id: "injection_suspected",
                severity: "warn",
                target: json!({ "image_id": image_id }),
                reason: "Text in this image reads as an instruction. It was transcribed and left out of the summary.",
                evidence: Some(json!({ "image_id": image_id })),
            },
        )?;
    }

    let flags = usize::from(read["injection_suspected"].as_bool() == Some(true));
    let confidence = read["confidence"].as_f64().unwrap_or(0.0);
    repo::finish_card(store, at, confidence, &[], &json!([]), &[])?;
    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    Ok(CardOutcome {
        card_id,
        run_id,
        status: if flags > 0 {
            "flagged".into()
        } else {
            "done".into()
        },
        confidence,
        flags,
        // A read looks at an image rather than at the corpus.
        passages_seen: 0,
        unsupported: 0,
    })
}

/// What a read card records as its question. Doc 01 section 4.4 requires one and
/// nobody typed this: the reader pointed at a picture.
const READ_QUESTION: &str = "What does this image show?";

/// Doc 07 section A10's `image_unreadable` description, in the reader's words.
const UNREADABLE: &str = "Could not read this image.";

/// Doc 07 section A2's `extract_first`, read from the pack's writing rules.
///
/// A pack that says nothing gets an empty list rather than a guess, so the
/// prompt asks for nothing in particular and the model reports what it sees.
fn reader_extract_first(pack: &tessera_doctrine::DoctrinePack) -> Vec<String> {
    pack.reader_extract_first.clone()
}

fn board_title(store: &Store, board_id: &str) -> String {
    store
        .conn()
        .query_row(
            "SELECT title FROM board WHERE id = ?1",
            rusqlite::params![board_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
}

/// Base64 for one content block. Doc 10 section 7: the encoded copy is for one
/// call and is never persisted.
pub fn base64(bytes: &[u8]) -> String {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(SET[(n >> 18) as usize & 63] as char);
        out.push(SET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            SET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            SET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Generate an exercise from the cards a board already holds. Doc 08.
///
/// A run of its own, kind `exercise`, because it is not a card: nothing is
/// retrieved, nothing is verified, and no claim is added to the board. Doc 08
/// section 1 is explicit that this reads what exists and never asks for a new
/// fact, and giving it its own run kind is what makes that visible in the log.
pub async fn run_exercise(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    audience_id: Option<&str>,
    level: Option<u8>,
) -> Result<ExerciseOutcome, Failure> {
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);

    // Doc 08 section 8 point 1: cap by budget. Eight items over at most eight
    // cards, because the packet's own budget is eight.
    let cards = repo::cards_for_exercise(store, board_id, 8)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;
    let scope: Vec<String> = cards
        .iter()
        .filter_map(|c| c["card_id"].as_str().map(str::to_string))
        .collect();

    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: None,
            kind: "exercise",
            depth: None,
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;

    // Doc 08 section 10's `no_eligible_cards`, decided before the packet rather
    // than inside the agent, because the packet schema requires at least one
    // card and a schema violation is the wrong way to report an empty board.
    if cards.is_empty() {
        repo::end_run(store, &run_id, "done")?;
        return Ok(ExerciseOutcome {
            exercise_id: None,
            run_id,
            items: 0,
            dropped: 0,
        });
    }

    let template = ctx.pack.exercise_templates.first();
    let template_id = template.map(|t| t.id.as_str()).unwrap_or("default");
    let mut packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "board_id": board_id,
        "scope": { "card_ids": scope },
        "cards": cards,
        "concepts": repo::concepts_for_packet(store, &ctx.profile_id, 20).unwrap_or_default(),
        "template": {
            "id": template_id,
            "item_kinds": template
                .map(|t| t.item_kinds.clone())
                .unwrap_or_else(|| vec!["recall".into(), "apply".into()]),
            // Doc 17 section 4's ladder, carried as the pack wrote it. The
            // agent reads which kinds a level asks for and which level a kind
            // sits at from this, so the mapping stays doctrine.
            "levels": ctx.pack.learning_templates.check_templates,
            "items_per_card_max": template.and_then(|t| t.items_per_card_max).unwrap_or(2),
            "options": level
                .and_then(|l| level_options(ctx, l))
                .map(|o| o as usize)
                .or_else(|| template.and_then(|t| t.options))
                .unwrap_or(4),
        },
        "audience_id": audience_id,
        "effort_budget": { "max_tokens": 2500, "max_items": 8 }
    });
    // Absent rather than null when no level was asked for. The packet schema
    // types this as an integer, and a null is a value that fails at the
    // boundary rather than a field that is not there.
    if let Some(level) = level {
        packet["template"]["level"] = json!(level);
    }

    let drafted = run_agent(
        &tessera_agents::Exercise,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: None,
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    let output = match drafted {
        Ok(o) => o.output,
        Err(f) => {
            repo::end_run(store, &run_id, "failed")?;
            return Err(f);
        }
    };

    let items = output["items"].clone();
    let count = items.as_array().map(Vec::len).unwrap_or(0);
    // Doc 08 section 9: the ratio of items that passed both checks. A caveat
    // means some were dropped, and the count of dropped ones is what the caveat
    // states, so the outcome carries it rather than the prose.
    let dropped = output["caveats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|c| c.split_whitespace().next())
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();

    let exercise_id = repo::write_exercise(
        store,
        repo::NewExercise {
            board_id,
            run_id: &run_id,
            template_id,
            audience_id,
            scope: &scope,
            items: &items,
            produced_by: &json!({ "agent_id": "exercise", "run_id": run_id }),
        },
    )
    .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;

    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    Ok(ExerciseOutcome {
        exercise_id: Some(exercise_id),
        run_id,
        items: count,
        dropped,
    })
}

/// The option count a level asks for, when the pack's check template names one.
fn level_options(ctx: &RunContext<'_>, level: u8) -> Option<u32> {
    ctx.pack
        .learning_templates
        .check_templates
        .iter()
        .find(|t| t.level == level)
        .and_then(|t| t.options)
}

/// What one exercise run produced. `exercise_id` is absent when the board had
/// no card worth testing, which is an outcome rather than a failure.
#[derive(Debug, Clone)]
pub struct ExerciseOutcome {
    pub exercise_id: Option<String>,
    pub run_id: String,
    pub items: usize,
    pub dropped: usize,
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

/// Doc 03 section 4's `parent` block, from the card's own ancestry.
///
/// This was `null` for every card, which made every follow-up a question with
/// no subject. "Which article says so?" retrieves nothing on its own, and
/// nothing is what it retrieved: recall on standalone questions measured 1.000
/// and on follow-ups 0.485, and the whole of that gap was this field.
fn parent_block(ancestors: &[repo::Ancestor]) -> Value {
    let Some(parent) = ancestors.first() else {
        return Value::Null;
    };
    json!({
        "card_id": parent.card_id,
        "question": parent.question,
        // Doc 03 section 6: the prompt gets the first 600 characters of the
        // parent answer, so there is no reason to carry more than that.
        "answer": parent.answer.as_deref().map(|a| truncate_chars(a, 600)).unwrap_or_default(),
        "depth": parent.depth,
        "confidence": parent.confidence,
        "answered_at": parent.answered_at,
        "citation_count": parent.citations.len(),
        "stale_citations": parent.stale_citations()
    })
}

/// Cut to a character count without splitting a character in half.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

fn build_router_packet(
    board: &Value,
    subject: &Subject<'_>,
    depth_override: Option<&str>,
    model_override: Option<&str>,
    ctx: &RunContext<'_>,
) -> Value {
    let card = subject.card;
    json!({
        "schema_version": "1.0",
        "run_id": subject.run_id,
        "card_id": card.id,
        "request": {
            "text": subject.question,
            "kind": card.kind,
            "anchor_text": card.anchor_text,
            "anchor_block_ref": card.anchor_block_ref,
            "depth_override": depth_override,
            // The user's model choice from the chat window names the alias that
            // writes the answer. Doc 01 section 5's per stage shape carries it.
            "model_override": model_override
                .map(|alias| json!({ "stage": "synthesize", "alias": alias }))
                .unwrap_or(Value::Null),
            "audience_override": null
        },
        "board": board,
        "parent": parent_block(subject.ancestors),
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
            "type_preferences": serde_json::to_value(&ctx.pack.visual_preferences.type_preferences).unwrap_or(json!({}))
        },
        "recent": [],
        "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
    })
}

/// Doc 04 section 4, assembled from what exists at M5.
///
/// `concepts` is empty because the Concept graph has no write path until M9;
/// entity resolution degrades exactly as the spec says it should, to literals
/// marked `unknown`. `retrievers` comes from the pack's list with the pack's
/// own defaults, because per-profile retriever configuration is an M9 Profile
/// surface. Doc 05 v0.2 section 8.5 adds `boards` when the profile has memory
/// on, which is doc 01 section 4.16's default.
/// Doc 04 section 4's ancestor chain, capped at three by the schema.
///
/// The Planner's job, doc 04 section 9, is "carrying the board context (parent
/// answer, seed, highlighted phrase) into each sub-question". It cannot do that
/// from an empty array, which is what it was given.
fn ancestor_blocks(ancestors: &[repo::Ancestor]) -> Value {
    Value::Array(
        ancestors
            .iter()
            .take(3)
            .map(|a| {
                json!({
                    "card_id": a.card_id,
                    "question": a.question,
                    // The schema caps the excerpt at 800 characters. Sending
                    // more would be rejected at the boundary, which is the
                    // schema guard doing its job and not a reason to send it.
                    "answer_excerpt": a.answer.as_deref().map(|t| truncate_chars(t, 800)).unwrap_or_default(),
                    "citations": a.citations.iter().map(|c| json!({
                        "ordinal": c["ordinal"],
                        "source_title": c["source_title"],
                        "source_class": c["source_class"],
                        "stale": c["stale"]
                    })).collect::<Vec<_>>()
                })
            })
            .collect(),
    )
}

fn build_planner_packet(
    store: &Store,
    board: &Value,
    routed: &Value,
    subject: &Subject<'_>,
    ctx: &RunContext<'_>,
) -> Result<Value, Failure> {
    let card = subject.card;
    let depth = routed["depth"]["chosen"].as_str().unwrap_or("deep");
    // Doc 04 section 4: 3 sub-questions for research, 1 for deep.
    let max_sub_questions = if depth == "research" { 3 } else { 1 };

    // Capped, because the packet has an effort budget and a profile with a
    // thousand terms would spend it on a glossary.
    // A read the store could not serve is not the Planner's failure to recover
    // from, so it degrades to the empty array the packet carried before M9
    // rather than killing a card over a glossary.
    let concepts = repo::concepts_for_packet(store, &ctx.profile_id, 40).unwrap_or_default();

    // Doc 16 section 3.4: what a notebook question may open is the vault and
    // the profile's own cards, and the Planner has to be able to tell that from
    // a profile that has configured nothing at all.
    let board_mode = board["mode"].as_str().unwrap_or("explore").to_string();

    // Doc 05 section 10 separates a retriever doctrine wants from one the
    // profile has told where to read, and the Planner is told the second.
    //
    // It plans assignments, and an assignment naming a connector the fan-out
    // will skip is a sub-question with no source behind it: the card comes back
    // thin and nothing says why. It also decides doc 04 section 10's
    // `no_retriever_enabled`, whose message reads "Enable at least web or local
    // in Profile", which only means anything if what it read is what Profile
    // controls. BN-140.
    let mut retrievers: Vec<Value> = ctx
        .pack
        .retrievers
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "enabled": r.enabled_by_default && ctx.retrievers.configured(&r.id),
                "config_summary": "",
            })
        })
        .collect();

    let memory_enabled: bool = store
        .conn()
        .query_row(
            "SELECT memory_enabled FROM profile WHERE id = ?1",
            [&ctx.profile_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v != 0)
        .unwrap_or(true);
    if memory_enabled && !retrievers.iter().any(|r| r["id"] == "boards") {
        retrievers.push(json!({ "id": "boards", "enabled": true, "config_summary": "" }));
    }

    let must_exclude: Vec<String> = ctx
        .pack
        .retrievers
        .iter()
        .flat_map(|r| r.must_exclude.iter().cloned())
        .collect();

    let domain = routed["classification"]["domain"].as_str().unwrap_or("unknown");
    let vocabulary = ctx
        .pack
        .domain_vocabulary
        .get(domain)
        .cloned()
        .unwrap_or_default();

    Ok(json!({
        "schema_version": "1.0",
        "run_id": subject.run_id,
        "card_id": card.id,
        "request": {
            "text": subject.question,
            "kind": card.kind,
            "anchor_text": card.anchor_text,
            "anchor_block_ref": card.anchor_block_ref
        },
        "routing": {
            "question_type": routed["classification"]["question_type"].clone(),
            "domain": domain,
            "audience_id": routed["classification"]["audience_id"].clone(),
            "entities": routed["classification"]["entities"].clone(),
            "needs_current_information": routed["classification"]["needs_current_information"].clone(),
            "needs_internal_documents": routed["classification"]["needs_internal_documents"].clone(),
            "needs_structured_data": routed["classification"]["needs_structured_data"].clone(),
            "regulatory_stakes": routed["classification"]["regulatory_stakes"].clone(),
            "depth": depth,
            "router_confidence": routed["confidence"].clone(),
            "early_flags": routed["early_flags"].clone()
        },
        "context": {
            // Doc 04 section 9 puts the board seed and context in scope for the
            // Planner. They were null while the board carried both, so a board
            // opened with a seed answered as though it had none.
            "board_mode": board_mode,
            "board_seed": board["seed_label"].clone(),
            "board_context": board["context"].clone(),
            "ancestors": ancestor_blocks(subject.ancestors),
            "parent_visual_block": null
        },
        // Doc 04 section 4. Empty until M9, because the Concept graph had no
        // write path and entity resolution degraded to literals marked
        // `unknown` exactly as doc 04 says it should when the graph is empty.
        // It is written now, so the Planner reads what the profile knows.
        "concepts": concepts,
        "retrievers": retrievers,
        "doctrine": {
            "must_exclude": must_exclude,
            // Doc 17 section 5: "the learner's sources hint from a path is
            // passed to the Planner as `must_include` locators". Empty outside
            // a lesson, because a mission is what carries them and a board in
            // any other mode is not planned against one.
            "must_include": mission_sources(store, ctx, &board_mode),
            "domain_vocabulary": vocabulary,
            "freshness_classes": {}
        },
        "effort_budget": {
            "max_tokens": 2500,
            "max_sub_questions": max_sub_questions,
            "max_passages_total": 40
        }
    }))
}

/// The ancestors in the shape doc 06 section A4 declares: what was asked, an
/// excerpt of what was answered, and whether it still stands.
fn synth_ancestors(ancestors: &[repo::Ancestor]) -> Value {
    Value::Array(
        ancestors
            .iter()
            .take(3)
            .map(|a| {
                json!({
                    "question": a.question,
                    "answer_excerpt": a.answer.as_deref()
                        .map(|t| truncate_chars(t, 800))
                        .unwrap_or_default(),
                    "stale": a.stale_citations() > 0,
                })
            })
            .collect(),
    )
}

fn build_synth_packet(
    routed: &Value,
    plan: Option<&Value>,
    mode: &str,
    subject: &Subject<'_>,
    passages: &[Value],
    ctx: &RunContext<'_>,
) -> Value {
    json!({
        "schema_version": "1.0",
        "run_id": routed["run_id"].clone(),
        "mode": mode,
        // The card's own kind and anchor, not `root` and null. A branch spawned
        // from a highlighted phrase read as a question typed from nothing, so
        // the prompt's "it came from the highlighted phrase" line could never
        // fire and doc 06 section A4's `request.kind` was always the same word.
        "request": {
            "text": subject.question,
            "kind": subject.card.kind,
            "anchor_text": subject.card.anchor_text,
        },
        // Doc 06 section A4: the Synthesizer reads the plan's constraints, so
        // the answer scope the Verifier checks is the one the Planner declared.
        "plan": plan.cloned().unwrap_or(Value::Null),
        "passages": passages,
        // Doc 06 section A2: "Reads the plan, the passages, the ancestors". The
        // field was hardcoded empty, so the prompt loop that reads it never ran
        // and a follow-up was written as though nothing preceded it. A stale
        // ancestor is marked, so the answer does not lean on a value that has
        // since moved.
        "ancestors": synth_ancestors(subject.ancestors),
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
