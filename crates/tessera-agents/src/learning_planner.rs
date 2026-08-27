//! The Learning Planner. Doc 17 section 7.
//!
//! The eleventh agent, and the one that calls a model least. Doc 17: "reads the
//! map, missions, paths, and recent evidence. Writes: prerequisite proposals for
//! new concepts (one model call with the medium alias, constrained to the
//! concepts on the map plus at most three new ones), the frontier, the next
//! lesson plan. Deterministic where possible: frontier selection and level
//! selection are rules; only decomposition of a new topic into concepts and
//! prerequisites is model work, and it is proposed, not applied."
//!
//! So a run with no new topic makes no model call at all. The frontier and the
//! level come from `tessera_core::learning`'s rules, which is why they are pure
//! functions in a module of their own: an agent that asked a model where the
//! learner stands would be guessing at something the map already knows.
//!
//! Nothing here writes. Proposals leave as proposals, and doc 01 section 4.10's
//! rule decides what happens to them: an agent proposes, a person confirms.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::learning;
use crate::prompts;

pub struct LearningPlanner;

const SYSTEM: &str = "\
You break a topic into the ideas someone has to understand, and say which ones \
come first.

Name ideas, not lessons: each one is something a person could be asked a \
question about. Use the terms already on the map where they fit, and add as few \
new ones as the topic needs. Say nothing about how well anyone knows them: that \
is measured, not guessed.";

#[async_trait]
impl Agent for LearningPlanner {
    fn id(&self) -> &str {
        "learning_planner"
    }
    fn packet_schema(&self) -> &'static str {
        ids::PACKET_LEARNING_PLANNER
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_LEARNING_PLANNER
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::LEARNING_PLANNER
    }
    fn completion_event(&self) -> Option<&'static str> {
        // The core emits `frontier.computed.v1` with the row writes, because
        // the frontier is only worth recording once something acts on it.
        None
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        advance(ctx, "reading_map")?;
        let concepts = read_concepts(packet);
        let edges = read_edges(packet);
        let mastered_at = packet["doctrine"]["mastered_at"].as_f64().unwrap_or(0.8);

        advance(ctx, "computing_frontier")?;
        let frontier = learning::frontier(&concepts, &edges, mastered_at);
        let lesson = plan_lesson(&frontier, &concepts, &edges);

        advance(ctx, "decomposing")?;
        // Doc 17 section 7: the only model work. A run that has a map and no
        // new topic asks nobody anything, which is most runs.
        let topic = packet["topic"].as_str().filter(|t| !t.trim().is_empty());
        let decomposed = match topic {
            Some(topic) => self.decompose(ctx, packet, topic).await?,
            None => json!({ "concepts": [], "edges": [] }),
        };

        advance(ctx, "checking_proposals")?;
        let max_new = packet["doctrine"]["max_new_concepts"].as_u64().unwrap_or(3) as usize;
        let (proposed_concepts, proposed_edges, dropped) = keep_proposals(&decomposed, &concepts, max_new);

        advance(ctx, "emitting")?;
        advance(ctx, "done")?;

        Ok(json!({
            "schema_version": "1.0",
            "agent_id": "learning_planner",
            "run_id": ctx.run_id,
            "proposed_concepts": proposed_concepts,
            "proposed_edges": proposed_edges,
            "frontier": frontier,
            "lesson": lesson,
            "declined_reason": Value::Null,
            // Doc 17 section 7: the frontier is a rule, so a plan built from
            // rules alone is as certain as the map it read. What lowers it is a
            // decomposition that had to be cut.
            "confidence": if dropped == 0 { 1.0 } else { 0.6 },
            "caveats": caveats(dropped),
        }))
    }
}

fn advance(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

fn caveats(dropped: usize) -> Vec<String> {
    if dropped == 0 {
        return Vec::new();
    }
    vec![format!(
        "{dropped} proposed ideas were left out, because a plan may add only so many at once."
    )]
}

impl LearningPlanner {
    /// The one model call. Doc 17 section 7's decomposition, constrained to the
    /// map plus at most `max_new_concepts` new terms.
    async fn decompose(
        &self,
        ctx: &mut AgentContext<'_>,
        packet: &Value,
        topic: &str,
    ) -> Result<Value, Failure> {
        let known: Vec<&str> = packet["concepts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["term"].as_str())
            .collect();
        let max_new = packet["doctrine"]["max_new_concepts"].as_u64().unwrap_or(3);

        let mut prompt = format!("The learner wants to understand: {topic}\n\n");
        if known.is_empty() {
            prompt.push_str("Their map is empty, so every idea you name is a new one.\n");
        } else {
            prompt.push_str(&format!(
                "Ideas already on their map: {}\n\
                 Use these terms exactly where they fit.\n",
                known.join(", ")
            ));
        }
        prompt.push_str(&format!(
            "\nName at most {max_new} ideas that are not on the map yet, and say which idea has \
             to come before which.\n"
        ));

        let schema = json!({
            "type": "object",
            "required": ["concepts"],
            "additionalProperties": false,
            "properties": {
                "concepts": { "type": "array", "items": {
                    "type": "object", "required": ["term"], "additionalProperties": false,
                    "properties": { "term": { "type": "string" }, "why": { "type": "string" } }
                }},
                "edges": { "type": "array", "items": {
                    "type": "object", "required": ["from_term", "to_term"],
                    "additionalProperties": false,
                    "properties": {
                        "from_term": { "type": "string" }, "to_term": { "type": "string" }
                    }
                }}
            }
        });

        let mut system = format!(
            "{SYSTEM}\n\n{}\n\n{}",
            prompts::HOUSE_STYLE,
            prompts::json_only(&schema)
        );
        if let Some(notice) = ctx.violation_notice() {
            system.push_str("\n\n");
            system.push_str(&notice);
        }

        let completion = ctx
            .call(
                &CompletionRequest::new(ctx.model_for("learning_plan"), "learning_plan")
                    .system(system)
                    .user(prompt)
                    .effort(Effort::Medium)
                    .max_tokens(1200)
                    .expecting(schema),
            )
            .await?;

        completion.json().map_err(|e| Failure {
            kind: "schema_violation".into(),
            detail: e.to_string(),
            recovery: Recovery::Retried,
            evidence: None,
            recoverable: true,
        })
    }
}

// ------------------------------------------------------------------ rules ---

fn read_concepts(packet: &Value) -> Vec<learning::Concept> {
    learning::concepts_from(
        packet["concepts"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default(),
    )
}

fn read_edges(packet: &Value) -> Vec<learning::Edge> {
    learning::edges_from(packet["edges"].as_array().map(Vec::as_slice).unwrap_or_default())
}

/// Doc 17 section 5: "the plan targets one or two concepts at the frontier plus
/// their immediate prerequisites".
///
/// The level is the ladder's, per doc 17 section 4: a concept nobody has passed
/// a check on opens at 1, and one that has opens one rung above where it last
/// passed. Null when there is no frontier, because a learner who rated nothing
/// has not said where to start and guessing would teach them something they may
/// already know.
pub fn plan_lesson(frontier: &[String], concepts: &[learning::Concept], edges: &[learning::Edge]) -> Value {
    let targets: Vec<String> = frontier.iter().take(2).cloned().collect();
    if targets.is_empty() {
        return Value::Null;
    }

    let mut prerequisites: Vec<String> = Vec::new();
    for target in &targets {
        for edge in edges {
            if edge.relation == "prerequisite_of"
                && &edge.to == target
                && !prerequisites.contains(&edge.from)
                && !targets.contains(&edge.from)
            {
                prerequisites.push(edge.from.clone());
            }
        }
    }

    // The lowest rung any target opens at: a lesson that opened at the highest
    // would ask the hardest question first about the thing the learner is least
    // sure of.
    let level = targets
        .iter()
        .filter_map(|id| concepts.iter().find(|c| &c.id == id))
        .map(|c| match c.difficulty_level {
            // Nobody has passed a check on it, so the next check is the first
            // one: level 1. `next_level` answers a different question, which is
            // where the ladder goes after a pass that happened.
            None => 1,
            Some(level) => learning::next_level(Some(level), true),
        })
        .min()
        .unwrap_or(1);

    json!({
        "targets": targets,
        "include_prerequisites": prerequisites,
        "level": level,
    })
}

/// Keep what the map can hold. Doc 17 section 7's "at most three new ones",
/// enforced here rather than hoped for in the prompt, and every edge dropped
/// whose ends are not a concept that exists or one being proposed.
fn keep_proposals(
    decomposed: &Value,
    known: &[learning::Concept],
    max_new: usize,
) -> (Vec<Value>, Vec<Value>, usize) {
    let on_map: std::collections::BTreeSet<String> = known.iter().map(|c| c.term.to_lowercase()).collect();

    let mut fresh: Vec<Value> = Vec::new();
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut dropped = 0usize;

    for concept in decomposed["concepts"].as_array().into_iter().flatten() {
        let Some(term) = concept["term"].as_str().map(str::trim).filter(|t| !t.is_empty()) else {
            continue;
        };
        let key = term.to_lowercase();
        // A term already on the map is not a proposal: the map has it, and
        // proposing it again would ask the learner to confirm what they
        // confirmed once already.
        if on_map.contains(&key) || !names.insert(key) {
            continue;
        }
        if fresh.len() >= max_new {
            dropped += 1;
            continue;
        }
        let mut out = json!({ "term": term });
        if let Some(why) = concept["why"].as_str() {
            out["why"] = json!(why);
        }
        fresh.push(out);
    }

    let proposable: std::collections::BTreeSet<String> = on_map
        .iter()
        .cloned()
        .chain(
            fresh
                .iter()
                .filter_map(|c| c["term"].as_str().map(str::to_lowercase)),
        )
        .collect();

    let mut edges: Vec<Value> = Vec::new();
    for edge in decomposed["edges"].as_array().into_iter().flatten() {
        let (Some(from), Some(to)) = (edge["from_term"].as_str(), edge["to_term"].as_str()) else {
            continue;
        };
        // An edge to an idea nobody proposed points at nothing. Dropped rather
        // than carried, because a proposal the learner cannot see is one they
        // cannot refuse.
        if !proposable.contains(&from.to_lowercase()) || !proposable.contains(&to.to_lowercase()) {
            dropped += 1;
            continue;
        }
        edges.push(json!({
            "from_term": from,
            "to_term": to,
            "relation": "prerequisite_of",
            "weight": 1.0,
        }));
    }

    (fresh, edges, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(id: &str, term: &str) -> learning::Concept {
        learning::Concept {
            id: id.to_string(),
            term: term.to_string(),
            ..learning::Concept::default()
        }
    }

    #[test]
    fn a_proposal_stops_at_the_packets_limit() {
        // Doc 17 section 7: "at most three new ones". A model that names eight
        // is cut here, because a prompt is a request and this is the check.
        let decomposed = json!({
            "concepts": (0..8).map(|i| json!({ "term": format!("idea {i}") })).collect::<Vec<_>>(),
            "edges": []
        });
        let (kept, _, dropped) = keep_proposals(&decomposed, &[], 3);
        assert_eq!(kept.len(), 3);
        assert_eq!(dropped, 5);
    }

    #[test]
    fn a_term_already_on_the_map_is_not_proposed_again() {
        let known = vec![concept("c1", "Liquidity coverage ratio")];
        let decomposed = json!({
            "concepts": [
                { "term": "liquidity coverage ratio" },
                { "term": "High quality liquid assets" },
                { "term": "high quality liquid assets" }
            ],
            "edges": [
                { "from_term": "High quality liquid assets", "to_term": "Liquidity coverage ratio" },
                { "from_term": "Something nobody named", "to_term": "Liquidity coverage ratio" }
            ]
        });
        let (kept, edges, _) = keep_proposals(&decomposed, &known, 3);
        assert_eq!(kept.len(), 1, "the map's own term came back as a proposal");
        assert_eq!(kept[0]["term"], "High quality liquid assets");
        // The edge between a proposal and a concept on the map stands; the one
        // naming an idea nobody proposed does not.
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0]["to_term"], "Liquidity coverage ratio");
    }

    #[test]
    fn a_lesson_opens_at_the_lowest_rung_its_targets_are_ready_for() {
        let concepts = vec![
            learning::Concept {
                difficulty_level: Some(3),
                ..concept("a", "A")
            },
            concept("b", "B"),
        ];
        let edges = vec![learning::Edge {
            from: "prereq".into(),
            to: "a".into(),
            relation: "prerequisite_of".into(),
            status: "confirmed".into(),
            weight: 1.0,
        }];
        let lesson = plan_lesson(&["a".to_string(), "b".to_string()], &concepts, &edges);
        assert_eq!(lesson["targets"], json!(["a", "b"]));
        // b has passed nothing, so it opens at 1, and the lesson takes the
        // lower of the two rather than asking its hardest question first.
        assert_eq!(lesson["level"], 1);
        assert_eq!(lesson["include_prerequisites"], json!(["prereq"]));
    }

    #[test]
    fn nothing_rated_plans_no_lesson() {
        // Doc 17 section 3 lets the learner skip every tile. A plan invented
        // from no frontier would teach them something they may know already.
        assert!(plan_lesson(&[], &[], &[]).is_null());
    }
}
