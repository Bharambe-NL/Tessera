//! The learning side of the pipeline: the Learning Planner, the Tutor turn, and
//! what a check does to a concept.
//!
//! Doc 14 section 3.4's machine is a session row that outlives any one run, so
//! everything here reads that row, decides one thing, and writes it back.

use serde_json::{Value, json};
use tessera_harness::{Failure, Recovery, RunAgent, run_agent};
use tessera_store::{Store, repo};

use super::RunContext;

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
