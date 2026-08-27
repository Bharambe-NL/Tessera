//! The learner leg. Doc 17 section 10.
//!
//! Four scripted learners walk the corpus's twenty concept path through the
//! product's own RPC surface: load the path, rate every concept the way the
//! policy claims, ask the Learning Planner where they stand. What the store
//! recorded is written out, and the scorer re-derives the three numbers from
//! that rather than from anything this file decided.
//!
//! The policies never reach the product. A learner's ratings are claims it
//! reads; the answers it could actually give are ground truth the corpus keeps,
//! which is what makes the frontier a thing to be right or wrong about. This
//! leg does not read those answers at all: a placement is decided before any
//! check is asked, and reading them here would be holding the answer sheet
//! while marking the paper.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessera_core::{Core, Router, rpc::Request};

#[derive(Debug, Clone, Deserialize)]
pub struct PathConcept {
    pub concept_id: String,
    pub term: String,
    #[serde(default)]
    pub prerequisite_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Learner {
    pub learner_id: String,
    pub policy: String,
    pub ratings: BTreeMap<String, i64>,
    #[serde(default)]
    pub expected_frontier: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LearningTruth {
    pub path: Vec<PathConcept>,
    pub learners: Vec<Learner>,
}

/// What one learner's placement recorded. Written as `learn_sessions.jsonl`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub learner_id: String,
    pub policy: String,
    /// Doc 17 section 3's frontier, in the corpus's own concept names so the
    /// scorer can compare it against the path without a translation table.
    pub frontier: Vec<String>,
    pub expected_frontier: Vec<String>,
    pub lesson_level: Option<i64>,
    /// Doc 17 section 7: proposals are proposed, never applied. Both counts, so
    /// a scorer can see the difference rather than take a boolean's word.
    pub proposed_concepts: usize,
    pub proposed_edges: usize,
    pub confirmed_edges_not_from_the_path: usize,
    /// Doc 17 section 2.4's honesty rule, as rows rather than a verdict: every
    /// concept whose only evidence is a rating, with the score it ended at.
    pub rated_only: Vec<Value>,
    pub note: String,
}

pub fn load(corpus: &Path) -> Result<LearningTruth, String> {
    let path = corpus.join("learning.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("learning.json: {e}"))
}

/// Walk one learner through placement and record what the store holds after.
pub fn place(
    core: &mut Core,
    router: &Router<Core>,
    truth: &LearningTruth,
    learner: &Learner,
) -> SessionRecord {
    let mut record = SessionRecord {
        learner_id: learner.learner_id.clone(),
        policy: learner.policy.clone(),
        frontier: Vec::new(),
        expected_frontier: learner.expected_frontier.clone(),
        lesson_level: None,
        proposed_concepts: 0,
        proposed_edges: 0,
        confirmed_edges_not_from_the_path: 0,
        rated_only: Vec::new(),
        note: String::new(),
    };

    // Doc 17 section 2.1: the path creates the concepts and its edges arrive
    // confirmed. The corpus names prerequisites by concept id and the product
    // takes terms, because a term is what a person recognises.
    let by_id: BTreeMap<&str, &PathConcept> = truth.path.iter().map(|c| (c.concept_id.as_str(), c)).collect();
    let concepts: Vec<Value> = truth
        .path
        .iter()
        .map(|c| {
            json!({
                "concept_term": c.term,
                "prerequisite_terms": c.prerequisite_ids
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).map(|p| p.term.clone()))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    if let Err(e) = call(
        router,
        core,
        "path.load",
        json!({ "path": { "code": "synthetic", "title": "The synthetic path", "concepts": concepts } }),
    ) {
        record.note = format!("path.load: {e}");
        return record;
    }

    // The map, so a corpus concept id can be matched to the row the product
    // wrote for it.
    let map = match call(router, core, "map.read", json!({})) {
        Ok(map) => map,
        Err(e) => {
            record.note = format!("map.read: {e}");
            return record;
        }
    };
    let ids: BTreeMap<String, String> = map["concepts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["term"].as_str()?.to_lowercase(),
                c["concept_id"].as_str()?.to_string(),
            ))
        })
        .collect();
    let corpus_id: BTreeMap<String, String> = truth
        .path
        .iter()
        .filter_map(|c| {
            ids.get(&c.term.to_lowercase())
                .map(|store_id| (store_id.clone(), c.concept_id.clone()))
        })
        .collect();

    // Doc 17 section 3: a rating per concept, and the learner may skip a tile.
    // A rating of 0 is a claim like any other and is recorded; the policies
    // never skip, because a skipped tile is the absence this leg is not
    // measuring.
    for concept in &truth.path {
        let Some(store_id) = ids.get(&concept.term.to_lowercase()) else {
            continue;
        };
        let rating = learner.ratings.get(&concept.concept_id).copied().unwrap_or(0);
        if let Err(e) = call(
            router,
            core,
            "concept.rate",
            json!({ "concept_id": store_id, "rating": rating }),
        ) {
            record.note = format!("concept.rate: {e}");
            return record;
        }
    }

    let plan = match call(router, core, "learning.plan", json!({ "reason": "path_loaded" })) {
        Ok(plan) => plan,
        Err(e) => {
            record.note = format!("learning.plan: {e}");
            return record;
        }
    };

    record.frontier = plan["frontier"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().and_then(|id| corpus_id.get(id).cloned()))
        .collect();
    record.frontier.sort();
    record.lesson_level = plan["lesson"]["level"].as_i64();
    record.proposed_concepts = plan["proposed_concepts"].as_array().map(Vec::len).unwrap_or(0);
    record.proposed_edges = plan["proposed_edges"].as_array().map(Vec::len).unwrap_or(0);

    // Doc 17 section 7: proposed, never applied. The path's own edges are
    // confirmed by design, so what would show a proposal being applied is a
    // confirmed edge the path did not draw.
    let after = call(router, core, "map.read", json!({})).unwrap_or_else(|_| json!({}));
    let mut planted: std::collections::BTreeSet<(String, String)> = Default::default();
    for concept in &truth.path {
        for id in &concept.prerequisite_ids {
            if let Some(prerequisite) = by_id.get(id.as_str()) {
                planted.insert((prerequisite.term.to_lowercase(), concept.term.to_lowercase()));
            }
        }
    }
    let term_of: BTreeMap<String, String> = after["concepts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["concept_id"].as_str()?.to_string(),
                c["term"].as_str()?.to_lowercase(),
            ))
        })
        .collect();
    for edge in after["edges"].as_array().into_iter().flatten() {
        if edge["status"].as_str() != Some("confirmed") {
            continue;
        }
        let (Some(from), Some(to)) = (
            edge["from_concept_id"].as_str().and_then(|id| term_of.get(id)),
            edge["to_concept_id"].as_str().and_then(|id| term_of.get(id)),
        ) else {
            continue;
        };
        if !planted.contains(&(from.clone(), to.clone())) {
            record.confirmed_edges_not_from_the_path += 1;
        }
    }

    // Doc 17 section 2.4: a rating can never move mastery above 0.5. Every
    // concept here has been rated and checked on nothing, so every one of them
    // is a row the honesty rule applies to.
    for concept in after["concepts"].as_array().into_iter().flatten() {
        if concept["difficulty_level"].as_i64().is_some() {
            continue;
        }
        record.rated_only.push(json!({
            "term": concept["term"],
            "self_rating": concept["self_rating"],
            "mastery": concept["mastery"],
            "learning_state": concept["learning_state"],
        }));
    }

    record
}

fn call(router: &Router<Core>, core: &mut Core, method: &str, params: Value) -> Result<Value, String> {
    let response = router
        .dispatch(core, Request::new(method, params, 1))
        .ok_or_else(|| format!("{method} is not registered"))?;
    if let Some(error) = response.error {
        return Err(format!("{}: {}", error.code, error.message));
    }
    response
        .result
        .ok_or_else(|| format!("{method} returned nothing"))
}

/// The line the run prints. The scorer does the arithmetic; this says what
/// happened.
pub fn report(records: &[SessionRecord]) -> String {
    let mut out = String::from(
        "| Learner | Frontier | Expected | Level | Proposed | Applied |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for r in records {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            r.learner_id,
            r.frontier.join(" "),
            r.expected_frontier.join(" "),
            r.lesson_level
                .map(|l| l.to_string())
                .unwrap_or_else(|| "none".into()),
            r.proposed_concepts + r.proposed_edges,
            r.confirmed_edges_not_from_the_path,
        ));
        if !r.note.is_empty() {
            out.push_str(&format!("| | | | | | {} |\n", r.note));
        }
    }
    out
}
