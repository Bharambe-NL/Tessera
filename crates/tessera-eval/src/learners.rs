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
    /// What this learner can actually answer, per concept, per level. Doc 17
    /// section 10's scripted policy.
    ///
    /// Placement never reads it: a placement is decided before any check is
    /// asked, and reading the answer sheet while marking the paper is the
    /// failure the two files exist to prevent. The lesson below does read it,
    /// because a check has been asked by then and something has to answer.
    #[serde(default)]
    pub answers: BTreeMap<String, Vec<u8>>,
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
    /// Doc 17 section 4, one row per check the lesson asked: the rung, whether
    /// it was passed, and the card it came from. The scorer re-derives the
    /// ladder from these rather than from what the product said it did.
    pub checks: Vec<Value>,
    /// The cards the lesson board holds that the Verifier stood behind. Doc 17
    /// section 4's "no item is ever generated from unverified text" is checked
    /// against this rather than against the product's own idea of eligibility.
    pub verified_cards: Vec<String>,
    /// Doc 17 section 5's learning record, as the event recorded it: one entry
    /// per line of the page, naming the row that line came from. Null when the
    /// lesson wrote none.
    ///
    /// Every carried passage is looked up in the store here, because "the page
    /// carries the evidence" is a claim about rows and a passage id that names
    /// nothing is the way that claim fails quietly. BN-143.
    pub record: Option<Value>,
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
        checks: Vec::new(),
        verified_cards: Vec::new(),
        record: None,
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
            // The product's own id, so the scorer can join a claim to the
            // checks that tested it. Doc 17 section 3's whole point is that one
            // is a claim and the other is evidence, and a metric about the
            // first catching up with the second needs both named the same way.
            "concept_id": concept["concept_id"],
            "term": concept["term"],
            "self_rating": concept["self_rating"],
            "mastery": concept["mastery"],
            "learning_state": concept["learning_state"],
        }));
    }

    record
}

/// How many checks one lesson asks. Doc 17 section 4's ladder needs a run of
/// them to be a ladder, and six is enough to walk from rung 1 to rung 4 and
/// back down twice.
const CHECKS_PER_LESSON: usize = 6;

/// Run one lesson on a board the corpus already filled, answering each check
/// the way the policy says the learner would.
///
/// Doc 17 section 4's two rules are what this exists to measure: the rung of
/// each check follows the last one, and every check names a card the Verifier
/// stood behind. The tutor writes the question, the product grades it, and
/// nothing here decides either.
pub fn teach(
    core: &mut Core,
    router: &Router<Core>,
    truth: &LearningTruth,
    learner: &Learner,
    board_id: &str,
    record: &mut SessionRecord,
) {
    record.verified_cards = verified_cards(core, board_id);
    if record.verified_cards.is_empty() {
        record.note = "the lesson board holds no card the Verifier stood behind".into();
        return;
    }

    if let Err(e) = call(
        router,
        core,
        "learn.start",
        json!({ "board_id": board_id, "topic": "the synthetic path" }),
    ) {
        record.note = format!("learn.start: {e}");
        return;
    }

    // The corpus names concepts by its own ids and the product by ULIDs, so the
    // policy is looked up through the terms both agree on.
    let map = call(router, core, "map.read", json!({})).unwrap_or_else(|_| json!({}));
    let term_of: BTreeMap<String, String> = map["concepts"]
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
    let corpus_of: BTreeMap<String, String> = truth
        .path
        .iter()
        .map(|c| (c.term.to_lowercase(), c.concept_id.clone()))
        .collect();

    for _ in 0..CHECKS_PER_LESSON {
        let turn = match call(router, core, "learn.check", json!({ "board_id": board_id })) {
            Ok(turn) => turn,
            Err(e) => {
                record.note = format!("learn.check: {e}");
                return;
            }
        };
        let item = turn["turn"]["check"]["item"].clone();
        let Some(item_id) = item["id"].as_str() else {
            // Doc 17 section 4's last resort, or a check the rules dropped.
            // Either way there is nothing to answer and saying so beats an
            // empty row that reads as a check that happened.
            record.note = "a turn produced no check".into();
            return;
        };
        let _ = item_id;

        // Doc 17 section 6: the turn names the concept its check is about, which
        // is what the shell hands back when the answer is graded.
        let target = turn["turn"]["check"]["concept_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let level = item["level"].as_u64().unwrap_or(1) as u8;
        let knows = term_of
            .get(&target)
            .and_then(|term| corpus_of.get(term))
            .and_then(|id| learner.answers.get(id))
            .is_some_and(|levels| levels.contains(&level));

        let answer_id = item["answer_id"].as_str().unwrap_or_default().to_string();
        let wrong = item["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o["id"].as_str())
            .find(|id| *id != answer_id)
            .unwrap_or("")
            .to_string();
        let picked = if knows { answer_id } else { wrong };

        let graded = match call(
            router,
            core,
            "learn.answer_check",
            json!({
                "board_id": board_id,
                "item": item,
                "picked": picked,
                "concept_ids": if target.is_empty() { vec![] } else { vec![target.clone()] },
            }),
        ) {
            Ok(graded) => graded,
            Err(e) => {
                record.note = format!("learn.answer_check: {e}");
                return;
            }
        };

        record.checks.push(json!({
            "concept_id": target,
            "level": level,
            "correct": graded["correct"],
            "next_level": graded["next_level"],
            "remedy": graded["remedy"],
            "card_id": item["source_card_id"],
        }));
    }

    let _ = call(router, core, "learn.end", json!({ "board_id": board_id }));
    record.record = saved_record(core, board_id);
}

/// The learning record this lesson wrote, read back off the event log.
///
/// Off the log rather than off the reply, because doc 17 section 10 asks
/// whether the record traces to what the session recorded and the log is what
/// the session recorded. Each carried passage is then looked up, so a page
/// citing evidence that is not there fails the gate rather than counting as a
/// citation.
fn saved_record(core: &Core, board_id: &str) -> Option<Value> {
    let mut saved = core
        .store
        .events(Some(board_id))
        .ok()?
        .into_iter()
        .find(|e| e.event_type == "learning_record.saved.v1")?
        .payload;

    let missing: Vec<String> = saved["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|line| line["passages"].as_array().cloned().unwrap_or_default())
        .filter_map(|id| id.as_str().map(str::to_string))
        .filter(|id| {
            core.store
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM passage WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                == 0
        })
        .collect();
    saved["passages_missing"] = json!(missing);
    Some(saved)
}

/// The cards on a board that are done or warn flagged, read from the store
/// rather than from the product's own eligibility rule.
fn verified_cards(core: &Core, board_id: &str) -> Vec<String> {
    let conn = core.store.conn();
    let Ok(mut stmt) = conn.prepare(
        "SELECT c.id FROM card c
         WHERE c.board_id = ?1 AND c.status IN ('done', 'flagged') AND c.answer IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM flag f
             WHERE f.card_id = c.id AND f.status = 'open' AND f.severity = 'block'
           )",
    ) else {
        return Vec::new();
    };
    stmt.query_map(rusqlite::params![board_id], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(std::result::Result::ok).collect())
        .unwrap_or_default()
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
        "| Learner | Frontier | Expected | Level | Proposed | Applied | Rungs asked |\n\
         | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for r in records {
        let rungs: Vec<String> = r
            .checks
            .iter()
            .map(|c| {
                let level = c["level"].as_u64().unwrap_or(0);
                // The rung and whether it was passed, because a run of fours
                // that were all wrong is a different lesson from a run of fours
                // that were right.
                format!("{level}{}", if c["correct"] == true { "" } else { "x" })
            })
            .collect();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            r.learner_id,
            r.frontier.join(" "),
            r.expected_frontier.join(" "),
            r.lesson_level
                .map(|l| l.to_string())
                .unwrap_or_else(|| "none".into()),
            r.proposed_concepts + r.proposed_edges,
            r.confirmed_edges_not_from_the_path,
            if rungs.is_empty() {
                "none".to_string()
            } else {
                rungs.join(" ")
            },
        ));
        if !r.note.is_empty() {
            out.push_str(&format!("| {} | | | | | | {} |\n", r.learner_id, r.note));
        }
    }
    out
}
