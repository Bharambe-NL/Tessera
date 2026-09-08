//! Exercises: made from a board, attempted in the UI, reported from a card.
//!
//! Doc 08. Grading one multiple choice answer needs no agent, so the attempt
//! comes from the shell and the score is computed in the store.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 08 section 3: "on demand from a board". The toolbar's Check
    // understanding, which is the only trigger until Learn mode adds its own.
    r.register("exercise.create", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Make {
            board_id: String,
            #[serde(default)]
            audience_id: Option<String>,
            /// Doc 17 section 4: which rung to ask at. Absent from the toolbar,
            /// which asks a board for an exercise rather than a learner for a
            /// check.
            #[serde(default)]
            level: Option<u8>,
        }
        let p: Make = params(p)?;
        let outcome = core
            .make_exercise(&p.board_id, p.audience_id.as_deref(), p.level)
            .map_err(core_error)?;
        Ok(json!({
            "exercise_id": outcome.exercise_id,
            "run_id": outcome.run_id,
            "items": outcome.items,
            "dropped": outcome.dropped,
        }))
    });

    r.register("exercise.list", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let exercises = repo::list_exercises(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "exercises": exercises }))
    });

    // Doc 08 section 7: the attempt comes from the UI, because grading a
    // multiple choice answer needs no agent. The score is computed in the store
    // from the exercise's own items rather than trusted from the caller, so it
    // is a fact about the exercise and not a number the shell sent.
    r.register("exercise.attempt", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Attempt {
            exercise_id: String,
            /// Item id to chosen option id.
            answers: Value,
        }
        let p: Attempt = params(p)?;
        let (attempt_id, correct, total) =
            repo::record_attempt(&mut core.store, &p.exercise_id, &p.answers).map_err(store_error)?;
        Ok(json!({
            "attempt_id": attempt_id,
            "correct": correct,
            "total": total,
        }))
    });

    // Doc 08 section 11: a wrong item is reported from the card, and the report
    // feeds pack maintenance rather than changing the exercise.
    r.register("exercise.report_item", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Report {
            exercise_id: String,
            item_id: String,
            #[serde(default)]
            reason: Option<String>,
        }
        let p: Report = params(p)?;
        repo::report_exercise_item(&mut core.store, &p.exercise_id, &p.item_id, p.reason.as_deref())
            .map_err(store_error)?;
        Ok(json!({ "reported": p.item_id }))
    });
}
