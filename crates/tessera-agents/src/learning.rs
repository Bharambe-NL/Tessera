//! Doc 17's deterministic half, as pure functions.
//!
//! Doc 17 section 7: "deterministic where possible: frontier selection and
//! level selection are rules; only decomposition of a new topic into concepts
//! and prerequisites is model work". This module is that sentence's first half.
//! Nothing here reads a store, calls a provider or writes an event, so the
//! rules can be tested against the cases doc 17 describes rather than against a
//! session that happened to reach them.
//!
//! What is not here: whether a score counts as mastered, and how long a
//! concept stays mastered without evidence. Both are doctrine, and both arrive
//! as arguments from the pack that stated them.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Doc 17 section 2.3's six states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Unseen,
    Exposed,
    Rated,
    Checked,
    Mastered,
    Decayed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Unseen => "unseen",
            State::Exposed => "exposed",
            State::Rated => "rated",
            State::Checked => "checked",
            State::Mastered => "mastered",
            State::Decayed => "decayed",
        }
    }

    /// A stored state, or `Unseen` for a concept nobody has touched.
    ///
    /// Null reads as unseen here and is stored as null on purpose: the column
    /// says nothing has happened, and this says what nothing looks like.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("exposed") => State::Exposed,
            Some("rated") => State::Rated,
            Some("checked") => State::Checked,
            Some("mastered") => State::Mastered,
            Some("decayed") => State::Decayed,
            _ => State::Unseen,
        }
    }
}

/// One concept as the rules see it. Doc 17 section 2.1's learning columns, plus
/// the domain, which decides the decay window.
#[derive(Debug, Clone, Default)]
pub struct Concept {
    pub id: String,
    pub term: String,
    pub state: Option<String>,
    pub self_rating: Option<i64>,
    pub mastery: Option<f64>,
    pub difficulty_level: Option<u8>,
    /// RFC 3339, as the column stores it.
    pub last_evidence_at: Option<String>,
    pub domain: Option<String>,
}

/// One prerequisite edge. Only `prerequisite_of` orders anything; the other two
/// relations describe a concept rather than gate it.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub status: String,
    pub weight: f64,
}

impl Edge {
    fn gates(&self) -> bool {
        self.relation == "prerequisite_of"
    }
}

/// The map's concept rows as the rules see them.
///
/// The shape is `repo::read_map`'s, which is also the Planner packet's, so the
/// core and the agent read one map the same way rather than each keeping a
/// reader that drifts from the other.
pub fn concepts_from(rows: &[Value]) -> Vec<Concept> {
    rows.iter()
        .map(|c| Concept {
            id: c["concept_id"].as_str().unwrap_or_default().to_string(),
            term: c["term"].as_str().unwrap_or_default().to_string(),
            state: c["learning_state"].as_str().map(str::to_string),
            self_rating: c["self_rating"].as_i64(),
            mastery: c["mastery"].as_f64(),
            difficulty_level: c["difficulty_level"].as_u64().map(|l| l as u8),
            last_evidence_at: c["last_evidence_at"].as_str().map(str::to_string),
            domain: c["domain"].as_str().map(str::to_string),
        })
        .collect()
}

/// The map's edge rows as the rules see them.
///
/// An edge with no relation is a prerequisite, because that is the only
/// relation the map draws today, and one with no status is proposed: the
/// learner has to say so for it to be theirs.
pub fn edges_from(rows: &[Value]) -> Vec<Edge> {
    rows.iter()
        .map(|e| Edge {
            from: e["from_concept_id"].as_str().unwrap_or_default().to_string(),
            to: e["to_concept_id"].as_str().unwrap_or_default().to_string(),
            relation: e["relation"].as_str().unwrap_or("prerequisite_of").to_string(),
            status: e["status"].as_str().unwrap_or("proposed").to_string(),
            weight: e["weight"].as_f64().unwrap_or(1.0),
        })
        .collect()
}

// ------------------------------------------------------------------ depth ---

/// How deep each concept sits in the prerequisite order, counting from zero.
///
/// A cycle cannot be ordered, and the learner did not draw one on purpose: an
/// edge that would deepen a concept already on the path is ignored rather than
/// looped over, so a map with a cycle still lays out and the concepts in it sit
/// at the depth their other prerequisites give them.
pub fn depths(concepts: &[Concept], edges: &[Edge]) -> BTreeMap<String, usize> {
    let ids: BTreeSet<&str> = concepts.iter().map(|c| c.id.as_str()).collect();
    let mut depth: BTreeMap<String, usize> = concepts.iter().map(|c| (c.id.clone(), 0)).collect();

    // One pass per concept settles any acyclic map, and bounds the walk when
    // the map is not one.
    for _ in 0..concepts.len() {
        let mut moved = false;
        for edge in edges.iter().filter(|e| e.gates()) {
            if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str()) {
                continue;
            }
            let from = depth.get(&edge.from).copied().unwrap_or(0);
            let to = depth.get(&edge.to).copied().unwrap_or(0);
            if to <= from {
                depth.insert(edge.to.clone(), from + 1);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    depth
}

// --------------------------------------------------------------- frontier ---

/// Doc 17 section 3: "the lowest prerequisite level where rated concepts have a
/// rating of 2 or more and mastery is still unverified".
///
/// Unverified means no check has moved the score: a rating is a claim and the
/// prior it sets is not evidence, so a concept sitting at exactly its rating's
/// prior is unverified however confident the rating was. That is the whole
/// point of the placement flow: the first lesson checks the frontier before
/// teaching anything, and an overconfident rating is caught in two questions.
///
/// Returns every concept at that depth, because a learner who rated three
/// things at one level has three places the lesson could start and the Planner
/// picks among them with the mission.
pub fn frontier(concepts: &[Concept], edges: &[Edge], mastered_at: f64) -> Vec<String> {
    let depth = depths(concepts, edges);
    let mut by_depth: BTreeMap<usize, Vec<String>> = BTreeMap::new();

    for concept in concepts {
        if !unverified_claim(concept, mastered_at) {
            continue;
        }
        let d = depth.get(&concept.id).copied().unwrap_or(0);
        by_depth.entry(d).or_default().push(concept.id.clone());
    }

    by_depth
        .into_iter()
        .next()
        .map(|(_, ids)| ids)
        .unwrap_or_default()
}

/// Doc 17 section 3: a concept the learner claimed and nobody has checked.
///
/// The frontier's own filter, named and public, because doc 17 section 3 asks
/// two things of it. The frontier is where the lowest of these sit, and a
/// lesson on any one of them opens with a check rather than with teaching:
/// placement recorded a claim, and a check is the only thing that turns a
/// claim into evidence.
pub fn unverified_claim(concept: &Concept, mastered_at: f64) -> bool {
    claims_to_know(concept) && !verified(concept, mastered_at)
}

/// A rating of 2 or more: "can explain it" or "can apply it".
fn claims_to_know(concept: &Concept) -> bool {
    concept.self_rating.is_some_and(|r| r >= 2)
}

/// Whether a check has ever moved this concept's score.
///
/// `difficulty_level` is set only by a passed check, and `checked` and above
/// are states only a check can reach, so either is evidence. Mastery alone is
/// not: exposure and a rating both move it and neither is a check.
fn verified(concept: &Concept, mastered_at: f64) -> bool {
    let state = State::parse(concept.state.as_deref());
    concept.difficulty_level.is_some()
        || matches!(state, State::Checked | State::Mastered)
        || concept.mastery.is_some_and(|m| m >= mastered_at)
}

// ------------------------------------------------------------------ level ---

/// Doc 17 section 4's ladder: "pass at level n moves the next check on that
/// concept to n+1; fail moves it to n-1".
///
/// Clamped at both ends. Level 4 is the last rung doc 17 names, and a failure
/// at level 1 has nowhere lower to go: what happens after two of those is
/// [`Remedy::Prerequisite`], not a level 0.
pub fn next_level(current: Option<u8>, passed: bool) -> u8 {
    let current = current.unwrap_or(1).clamp(1, 4);
    if passed {
        (current + 1).min(4)
    } else {
        current.saturating_sub(1).max(1)
    }
}

/// What a failed check calls for. Doc 17 section 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    /// A pass. The next check on this concept is one rung up.
    None,
    /// A remedial card on the concept itself, at the level the ladder dropped
    /// to.
    Card { level: u8 },
    /// Doc 17 section 4: "two fails at level 1 open a card on the concept's
    /// strongest prerequisite instead". Strongest is the heaviest confirmed
    /// prerequisite edge, and the learner's own confirmation outranks a
    /// proposal, so a proposed edge is only reached for when nothing is
    /// confirmed.
    Prerequisite { concept_id: String, level: u8 },
}

/// Decide what to do after a check.
///
/// `fails_at_one` counts consecutive failures at level 1 on this concept,
/// including the one being recorded.
pub fn remedy(concept_id: &str, level: u8, passed: bool, fails_at_one: u32, edges: &[Edge]) -> Remedy {
    if passed {
        return Remedy::None;
    }
    if level <= 1
        && fails_at_one >= 2
        && let Some(prerequisite) = strongest_prerequisite(concept_id, edges)
    {
        return Remedy::Prerequisite {
            concept_id: prerequisite,
            level: 1,
        };
    }
    Remedy::Card {
        level: next_level(Some(level), false),
    }
}

/// The heaviest prerequisite of a concept, confirmed ones first.
pub fn strongest_prerequisite(concept_id: &str, edges: &[Edge]) -> Option<String> {
    let mut candidates: Vec<&Edge> = edges.iter().filter(|e| e.gates() && e.to == concept_id).collect();
    candidates.sort_by(|a, b| {
        let confirmed = |e: &Edge| e.status == "confirmed";
        confirmed(b)
            .cmp(&confirmed(a))
            .then(
                b.weight
                    .partial_cmp(&a.weight)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.from.cmp(&b.from))
    });
    candidates.first().map(|e| e.from.clone())
}

// ------------------------------------------------------------------ state ---

/// Where a concept stands after evidence. Doc 17 section 2.3.
///
/// Called by the core, which is the layer that can read the pack's threshold.
/// The projection folds the score and stops at `checked` for exactly that
/// reason.
pub fn state_after_check(
    current: Option<&str>,
    mastery: f64,
    level: u8,
    passed: bool,
    mastered_at: f64,
) -> State {
    let current = State::parse(current);
    if !passed {
        // Doc 17 section 2.3: "a failed check can move mastered back to
        // checked". Never further: the learner has still been checked.
        return match current {
            State::Mastered | State::Decayed => State::Checked,
            State::Unseen | State::Exposed | State::Rated => State::Checked,
            other => other,
        };
    }
    // Doc 17 section 2.3: mastered needs the score and a passed check at level
    // 3 or higher. A run of level 1 passes is a person who can recite it.
    if mastery >= mastered_at && level >= 3 {
        State::Mastered
    } else {
        State::Checked
    }
}

/// Doc 17 section 2.3: mastered past the doctrine's freshness window without
/// new evidence is `decayed`, "computed, no scheduler needed".
///
/// Only `mastered` decays. A concept at `checked` has never been claimed to be
/// finished, so there is nothing to take back, and one at `exposed` even less.
pub fn decayed(state: &str, last_evidence_at: Option<&str>, now: &str, window_days: u32) -> bool {
    if state != State::Mastered.as_str() {
        return false;
    }
    let (Some(last), Some(now)) = (parse_time(last_evidence_at), parse_time(Some(now))) else {
        // No evidence recorded is not evidence of age. A concept whose
        // timestamp cannot be read stays where it is rather than being demoted
        // by a parse failure.
        return false;
    };
    now.saturating_sub(last) > i64::from(window_days) * 86_400
}

/// Seconds since the epoch, for the two timestamps a decay check compares.
fn parse_time(value: Option<&str>) -> Option<i64> {
    let text = value?;
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept(id: &str, rating: Option<i64>, mastery: Option<f64>, state: Option<&str>) -> Concept {
        Concept {
            id: id.to_string(),
            term: id.to_string(),
            state: state.map(str::to_string),
            self_rating: rating,
            mastery,
            ..Concept::default()
        }
    }

    fn edge(from: &str, to: &str) -> Edge {
        Edge {
            from: from.to_string(),
            to: to.to_string(),
            relation: "prerequisite_of".into(),
            status: "confirmed".into(),
            weight: 1.0,
        }
    }

    #[test]
    fn the_frontier_is_the_lowest_thing_the_learner_claims_and_nobody_checked() {
        // a is a prerequisite of b, which is a prerequisite of c. The learner
        // says they can explain all three and has been checked on none.
        let concepts = vec![
            concept("a", Some(2), None, None),
            concept("b", Some(3), None, None),
            concept("c", Some(2), None, None),
        ];
        let edges = vec![edge("a", "b"), edge("b", "c")];
        assert_eq!(frontier(&concepts, &edges, 0.8), vec!["a".to_string()]);
    }

    #[test]
    fn a_checked_concept_is_not_the_frontier_however_it_was_rated() {
        // Doc 17 section 3: the frontier is where mastery is "still
        // unverified". A pass at level 1 is verification, even a shaky one.
        let concepts = vec![
            Concept {
                difficulty_level: Some(1),
                ..concept("a", Some(3), Some(0.15), Some("checked"))
            },
            concept("b", Some(2), None, None),
        ];
        assert_eq!(frontier(&concepts, &[edge("a", "b")], 0.8), vec!["b".to_string()]);
    }

    #[test]
    fn a_rating_alone_never_verifies_itself() {
        // The prior a rating sets is not evidence, so a concept sitting at
        // exactly its prior is still the frontier. This is the overconfident
        // rater doc 17 section 3 is written against.
        let concepts = vec![concept("a", Some(3), Some(0.5), Some("rated"))];
        assert_eq!(frontier(&concepts, &[], 0.8), vec!["a".to_string()]);
    }

    #[test]
    fn nothing_rated_has_no_frontier() {
        // Doc 17 section 3 asks for a rating per concept and lets the learner
        // skip any tile. Skipping every one leaves the Planner nothing to place
        // them at, and an empty list says so rather than guessing the first.
        let concepts = vec![concept("a", None, None, None), concept("b", Some(1), None, None)];
        assert!(frontier(&concepts, &[], 0.8).is_empty());
    }

    #[test]
    fn a_cycle_still_lays_out() {
        let concepts = vec![
            concept("a", Some(2), None, None),
            concept("b", Some(2), None, None),
        ];
        let edges = vec![edge("a", "b"), edge("b", "a")];
        // Whatever it settles on, it settles: no loop, and both concepts have a
        // depth.
        let depth = depths(&concepts, &edges);
        assert_eq!(depth.len(), 2);
        assert!(!frontier(&concepts, &edges, 0.8).is_empty());
    }

    #[test]
    fn the_ladder_goes_up_on_a_pass_and_down_on_a_failure() {
        assert_eq!(next_level(Some(1), true), 2);
        assert_eq!(next_level(Some(3), true), 4);
        assert_eq!(next_level(Some(4), true), 4, "there is no level 5");
        assert_eq!(next_level(Some(3), false), 2);
        assert_eq!(next_level(Some(1), false), 1, "there is no level 0");
        assert_eq!(next_level(None, true), 2, "a first check is a level 1 check");
    }

    #[test]
    fn two_failures_at_the_bottom_open_the_prerequisite() {
        // Doc 17 section 4. The first failure is a remedial card on the concept
        // itself; the second says the problem is underneath it.
        let edges = vec![
            Edge {
                weight: 0.4,
                ..edge("weak", "target")
            },
            Edge {
                weight: 0.9,
                ..edge("strong", "target")
            },
        ];
        assert_eq!(remedy("target", 1, false, 1, &edges), Remedy::Card { level: 1 });
        assert_eq!(
            remedy("target", 1, false, 2, &edges),
            Remedy::Prerequisite {
                concept_id: "strong".into(),
                level: 1
            }
        );
        // A pass asks for nothing.
        assert_eq!(remedy("target", 1, true, 5, &edges), Remedy::None);

        // With nothing underneath it, the remedial card is still the concept's
        // own: there is no prerequisite to fall back to.
        assert_eq!(remedy("target", 1, false, 3, &[]), Remedy::Card { level: 1 });
    }

    #[test]
    fn a_confirmed_prerequisite_outranks_a_heavier_proposal() {
        // Doc 01 section 4.10: an agent proposes and a person confirms. A
        // planner's confident guess does not outrank what the learner said.
        let edges = vec![
            Edge {
                status: "proposed".into(),
                weight: 1.0,
                ..edge("guess", "target")
            },
            Edge {
                status: "confirmed".into(),
                weight: 0.3,
                ..edge("agreed", "target")
            },
        ];
        assert_eq!(
            strongest_prerequisite("target", &edges).as_deref(),
            Some("agreed")
        );
    }

    #[test]
    fn mastered_needs_the_score_and_a_hard_enough_check() {
        // Doc 17 section 2.3: "mastery at or above the doctrine threshold with
        // a passed check at level 3 or higher".
        assert_eq!(
            state_after_check(Some("checked"), 0.9, 3, true, 0.8),
            State::Mastered
        );
        assert_eq!(
            state_after_check(Some("checked"), 0.9, 1, true, 0.8),
            State::Checked,
            "a run of recall questions is not mastery"
        );
        assert_eq!(
            state_after_check(Some("checked"), 0.6, 4, true, 0.8),
            State::Checked,
            "one hard pass is not a score"
        );
        // And a failure takes mastery back without taking the checking back.
        assert_eq!(
            state_after_check(Some("mastered"), 0.9, 3, false, 0.8),
            State::Checked
        );
        assert_eq!(state_after_check(None, 0.1, 1, false, 0.8), State::Checked);
    }

    #[test]
    fn only_mastered_decays_and_only_past_the_window() {
        let then = "2026-01-01T00:00:00Z";
        let soon = "2026-03-01T00:00:00Z";
        let late = "2026-09-01T00:00:00Z";

        assert!(!decayed("mastered", Some(then), soon, 180));
        assert!(decayed("mastered", Some(then), late, 180));
        // Nothing else decays: a concept at checked was never claimed finished.
        assert!(!decayed("checked", Some(then), late, 180));
        assert!(!decayed("exposed", Some(then), late, 180));
        // And an unreadable or missing timestamp is not evidence of age.
        assert!(!decayed("mastered", None, late, 180));
        assert!(!decayed("mastered", Some("last tuesday"), late, 180));
    }
}
