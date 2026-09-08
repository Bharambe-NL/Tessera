//! The learning map, its concepts, its missions and the plan over them.
//!
//! Doc 17. The map is read, rated and planned against; nothing here teaches,
//! because a lesson is a board and the Tutor runs it.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    r.register("map.read", |core: &mut Core, _| {
        let (concepts, edges) = repo::read_map(&core.store, &core.profile_id).map_err(store_error)?;
        let mission = repo::active_mission(&core.store, &core.profile_id).map_err(store_error)?;
        let board_id = core.map_board().map_err(core_error)?;
        let mastered_at = core
            .packs
            .get(&core.pack_code)
            .map(|p| p.learning_templates.mastered_at)
            .unwrap_or(0.8);

        // Doc 17 section 6: the map is "layered by prerequisite depth, never
        // hand arranged", and the frontier is a band across it. Both are rules
        // the product already owns, so the view is told the answer rather than
        // deriving a second one from the edges it was handed.
        let rules = tessera_agents::learning::concepts_from(&concepts);
        let edge_rules = tessera_agents::learning::edges_from(&edges);
        let depths = tessera_agents::learning::depths(&rules, &edge_rules);
        let frontier = tessera_agents::learning::frontier(&rules, &edge_rules, mastered_at);
        let concepts: Vec<Value> = concepts
            .into_iter()
            .map(|mut c| {
                let id = c["concept_id"].as_str().unwrap_or_default().to_string();
                c["depth"] = json!(depths.get(&id).copied().unwrap_or(0));
                c
            })
            .collect();

        Ok(json!({
            "board_id": board_id,
            "concepts": concepts,
            "edges": edges,
            "frontier": frontier,
            "mission": mission,
            "mastered_at": mastered_at,
        }))
    });

    // Doc 17 section 6's node panel: what this concept is linked to, read when
    // a node is opened rather than carried on every map read. A map of two
    // hundred concepts would otherwise ship every card on every one of them to
    // draw twenty circles.
    r.register("map.concept", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            concept_id: String,
        }
        let p: Which = params(p)?;
        let (cards, pages) = repo::concept_links(&core.store, &p.concept_id).map_err(store_error)?;
        Ok(json!({ "cards": cards, "pages": pages }))
    });

    // Doc 17 section 2.1: loading a path creates or links the concepts, creates
    // the edges as confirmed, and offers a mission.
    r.register("path.load", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Load {
            /// A path from the active pack, by code.
            #[serde(default)]
            code: Option<String>,
            /// Or the path itself, which is what an authored one is.
            #[serde(default)]
            path: Option<Value>,
        }
        let p: Load = params(p)?;
        let path = match (p.path, p.code) {
            (Some(path), _) => path,
            (None, Some(code)) => {
                let pack = core
                    .packs
                    .get(&core.pack_code)
                    .map_err(|e| RpcError::core("pack_missing", e.to_string()))?;
                pack.learning_templates
                    .learning_paths
                    .iter()
                    .find(|path| path["code"].as_str() == Some(code.as_str()))
                    .cloned()
                    .ok_or_else(|| {
                        RpcError::core("no_such_path", "This pack ships no path with that code.")
                    })?
            }
            (None, None) => {
                return Err(RpcError::core(
                    "no_path",
                    "Name a path in the pack, or send one to load.",
                ));
            }
        };
        core.load_path(&path).map_err(core_error)
    });

    // Doc 17 section 2.1: a rating is a claim, never evidence.
    r.register("concept.rate", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Rate {
            concept_id: String,
            rating: i64,
        }
        let p: Rate = params(p)?;
        if !(0..=3).contains(&p.rating) {
            return Err(RpcError::core(
                "rating_out_of_range",
                "A rating runs from 0, never heard of it, to 3, can apply it.",
            ));
        }
        repo::rate_concept(&mut core.store, &p.concept_id, p.rating).map_err(store_error)?;
        Ok(json!({ "concept_id": p.concept_id, "rating": p.rating }))
    });

    // Doc 01 section 4.10, one layer up: the learner confirms an edge an agent
    // proposed.
    r.register("concept.confirm_edge", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Confirm {
            edge_id: String,
        }
        let p: Confirm = params(p)?;
        repo::confirm_edge(&mut core.store, &p.edge_id).map_err(store_error)?;
        Ok(json!({ "edge_id": p.edge_id }))
    });

    r.register("mission.create", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Create {
            statement: String,
            #[serde(default)]
            target_concept_ids: Vec<String>,
            /// Doc 17 section 5: the locators the path this mission came from
            /// named. Absent for a mission the learner wrote themselves.
            #[serde(default)]
            sources_hint: Vec<String>,
        }
        let p: Create = params(p)?;
        if p.statement.trim().is_empty() {
            return Err(RpcError::core(
                "empty_mission",
                "Say why you want this, so a lesson can fit the reason.",
            ));
        }
        let profile_id = core.profile_id.clone();
        let mission_id = repo::create_mission(
            &mut core.store,
            &profile_id,
            &p.statement,
            &p.target_concept_ids,
            &p.sources_hint,
        )
        .map_err(store_error)?;
        Ok(json!({ "mission_id": mission_id }))
    });

    // Doc 17 section 7: runs on mission creation, path load, lesson end, and on
    // demand. The reason travels because the Planner reads it.
    r.register("learning.plan", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Plan {
            #[serde(default = "on_demand")]
            reason: String,
            #[serde(default)]
            topic: Option<String>,
        }
        fn on_demand() -> String {
            "on_demand".to_string()
        }
        let p: Plan = params(p)?;
        core.plan_learning(&p.reason, p.topic.as_deref())
            .map_err(core_error)
    });

    // Doc 17 section 6's last line: "Home shows, per mission, the fraction of
    // concepts at checked or better and the current frontier concept".
    //
    // Both are rules rather than counts, so they are answered here: what counts
    // as "checked or better" is doc 17 section 2.3's ordering, and the frontier
    // is section 3's. Home draws the answer.
    r.register("mission.summary", |core: &mut Core, _| {
        let mission = repo::active_mission(&core.store, &core.profile_id).map_err(store_error)?;
        let (concepts, edges) = repo::read_map(&core.store, &core.profile_id).map_err(store_error)?;
        let mastered_at = core
            .packs
            .get(&core.pack_code)
            .map(|p| p.learning_templates.mastered_at)
            .unwrap_or(0.8);

        // A mission that named targets is about those; one that named none is
        // about everything the learner has on the map, which is what a mission
        // with no targets yet means rather than a mission about nothing.
        let targets: Vec<String> = mission["target_concept_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let mine: Vec<Value> = concepts
            .iter()
            .filter(|c| {
                targets.is_empty()
                    || c["concept_id"]
                        .as_str()
                        .is_some_and(|id| targets.iter().any(|t| t == id))
            })
            .cloned()
            .collect();

        let rules = tessera_agents::learning::concepts_from(&mine);
        let checked = rules
            .iter()
            .filter(|c| tessera_agents::learning::at_least_checked(c))
            .count();

        // The frontier is read over the whole map rather than over the
        // mission's slice, because a prerequisite outside the mission still has
        // to come first and a frontier that ignored it would send the learner
        // at something they are not ready for.
        let all = tessera_agents::learning::concepts_from(&concepts);
        let frontier = tessera_agents::learning::frontier(
            &all,
            &tessera_agents::learning::edges_from(&edges),
            mastered_at,
        );
        let terms: Vec<String> = frontier
            .iter()
            .filter_map(|id| all.iter().find(|c| &c.id == id))
            .map(|c| c.term.clone())
            .collect();

        Ok(json!({
            "mission": mission,
            "concepts": mine.len(),
            "checked_or_better": checked,
            "frontier": terms,
        }))
    });

    // Doc 09 section 9's Concepts row actions. Doc 01 section 4.10: agents
    // propose and the user confirms, so this is the confirming half.
    r.register("concept.decide", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Decide {
            concept_id: String,
            accept: bool,
        }
        let p: Decide = params(p)?;
        match repo::decide_concept(&mut core.store, &p.concept_id, p.accept).map_err(store_error)? {
            Some(term) => Ok(json!({ "concept_id": p.concept_id, "term": term })),
            None => Err(RpcError::core(
                "no_proposed_concept",
                "That concept was decided already. Reload Library to see where it went.",
            )),
        }
    });
}
