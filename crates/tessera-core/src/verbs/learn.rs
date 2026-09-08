//! Learn mode: the Tutor's turn machine, one method per trigger.
//!
//! Doc 14 section 3.3's triggers, as the surface the panel drives. One method
//! per trigger rather than one that infers the stage, because doc 14 section
//! 3.4's machine moves on what the learner did.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::pipeline;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 14 section 3.3's triggers, as the surface the panel drives.
    //
    // One method per trigger rather than one that infers the stage, because doc
    // 14 section 3.4's machine moves on what the learner did and a turn that
    // guessed which move it was would be guessing at the learner.
    r.register("learn.start", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Start {
            board_id: String,
            topic: String,
        }
        let p: Start = params(p)?;
        if p.topic.trim().is_empty() {
            return Err(RpcError::core(
                "empty_topic",
                "Say what you want to learn about first.",
            ));
        }
        let session_id =
            repo::start_learn_session(&mut core.store, &p.board_id, p.topic.trim()).map_err(store_error)?;

        // Doc 17 section 3: "the first lesson checks the frontier before
        // teaching anything, so an overconfident rating is caught within the
        // first two questions". Placement already asked how much the learner
        // knows and wrote down the answer, so intake would be asking it twice
        // and teaching first would be teaching on the strength of a claim.
        //
        // A check that produced no item falls back to intake rather than
        // opening a panel with nothing in it: doc 17 section 4's sourcing order
        // ends at "request a card first", and a profile with no verified card
        // anywhere has nothing to ask about yet.
        let claimed = core.claimed_but_unchecked(p.topic.trim());
        let checked_first = claimed
            .then(|| core.tutor_turn(&p.board_id, "checking", None, None).ok())
            .flatten()
            .filter(|turn| turn["check"]["item"]["id"].is_string());
        let opened_with_a_check = checked_first.is_some();
        let turn = match checked_first {
            Some(turn) => turn,
            None => core
                .tutor_turn(&p.board_id, "intake", None, None)
                .map_err(core_error)?,
        };
        Ok(json!({
            "session_id": session_id,
            "turn": turn,
            "opened_with_a_check": opened_with_a_check,
        }))
    });

    r.register("learn.get", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let session = repo::read_learn_session(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "session": session }))
    });

    // Doc 14 section 3.4: the learner may skip intake with "just build it", so
    // answering is optional and building is its own call.
    r.register("learn.answer_intake", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Answer {
            board_id: String,
            q: String,
            a: String,
        }
        let p: Answer = params(p)?;
        let session = repo::read_learn_session(&core.store, &p.board_id)
            .map_err(store_error)?
            .ok_or_else(|| RpcError::core("no_session", "This board has no learn session."))?;

        let mut intake = session["intake"].as_array().cloned().unwrap_or_default();
        intake.push(json!({ "q": p.q, "a": p.a }));
        let session_id = session["session_id"].as_str().unwrap_or_default().to_string();

        repo::update_learn_session(
            &mut core.store,
            repo::LearnUpdate {
                actor: repo::Actor::Learner,
                session_id: &session_id,
                board_id: &p.board_id,
                status: None,
                set: vec![("intake", Value::Array(intake))],
                event: "learn.intake_answered.v1",
                payload: json!({ "session_id": session_id, "q": p.q, "a": p.a }),
            },
        )
        .map_err(store_error)?;
        Ok(json!({ "recorded": true }))
    });

    r.register("learn.build", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "building", None, None)
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    r.register("learn.check", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Check {
            board_id: String,
            #[serde(default)]
            card_id: Option<String>,
        }
        let p: Check = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "checking", None, p.card_id.as_deref())
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    // Doc 14 section 3.6. No agent: grading one multiple choice answer needs
    // none, the same reason doc 08 section 7 has the UI record an attempt.
    r.register("learn.answer_check", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Answered {
            board_id: String,
            item: Value,
            picked: String,
            #[serde(default)]
            concept_ids: Vec<String>,
        }
        let p: Answered = params(p)?;
        let adaptation = core
            .record_check(&p.board_id, &p.item, &p.picked, &p.concept_ids)
            .map_err(core_error)?;
        Ok(adaptation.to_json())
    });

    r.register("learn.say", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Say {
            board_id: String,
            message: String,
        }
        let p: Say = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "reading", Some(&p.message), None)
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    r.register("learn.end", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        // The record is built before the session ends, because ending it takes
        // the board out of learn mode and the record is a note about the lesson
        // that was.
        let record = core.write_learning_record(&p.board_id).ok().flatten();
        let mut summary = pipeline::end_learn_session(&mut core.store, &p.board_id)
            .map_err(|f| RpcError::core("learn", f.to_string()))?;
        summary["record_page_id"] = json!(record);
        Ok(summary)
    });
}
