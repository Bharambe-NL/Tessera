//! The Notebook: a question asked of the person's own writing.
//!
//! Doc 16 section 3.4. A session is a board, so everything a board has comes
//! with it: history, events, memory and export.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::{Anchor, Core, NOTEBOOK, truncate_title};
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    r.register("notebook.open", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Open {
            #[serde(default)]
            board_id: Option<String>,
        }
        let p: Open = params(p)?;
        let board_id = match p.board_id {
            Some(id) => id,
            // Doc 16 section 3.4's chat layout starts somewhere, and a notebook
            // question runs at deep because that is what reads the vault.
            None => core.create_board(NOTEBOOK_TITLE, "deep").map_err(core_error)?,
        };
        repo::start_notebook(&mut core.store, &board_id).map_err(store_error)?;
        Ok(json!({ "board_id": board_id, "mode": crate::core::NOTEBOOK }))
    });

    r.register("notebook.session", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            board_id: String,
        }
        let p: Which = params(p)?;
        let board = repo::read_board(&core.store, &p.board_id)
            .map_err(store_error)?
            .ok_or_else(|| RpcError::core("board_missing", "That session is not on this profile."))?;

        // Doc 16 section 3.4's three states, read from the log rather than
        // recomputed here: the core wrote them when the answer settled, and two
        // places deciding how grounded an answer is would eventually disagree.
        let grounding: std::collections::BTreeMap<String, Value> = core
            .store
            .events(Some(&p.board_id))
            .map_err(store_error)?
            .into_iter()
            .filter(|e| e.event_type == "notebook.grounding.v1")
            .filter_map(|e| Some((e.card_id?, e.payload)))
            .collect();

        let turns: Vec<Value> = board
            .cards
            .iter()
            .map(|card| {
                json!({
                    "card_id": card.id,
                    "question": card.question,
                    "answer": card.answer,
                    "status": card.status,
                    "page_id": card.page_id,
                    "citations": card.citations,
                    "grounding": grounding
                        .get(&card.id)
                        .and_then(|g| g["state"].as_str())
                        // A card with no grounding event was asked before this
                        // board became a session. Saying so beats calling it
                        // grounded, which is the reading a missing value would
                        // otherwise get.
                        .unwrap_or("unknown"),
                })
            })
            .collect();

        Ok(json!({
            "board_id": board.id,
            "title": board.title,
            "mode": board.mode,
            "turns": turns,
        }))
    });

    // Doc 16 section 3.4: "Open on a board (creates a root card from the
    // session)". The question is asked again rather than the card moved,
    // because a board is where a question grows follow-ups and branches, and a
    // card that arrived without a run of its own would have no trail behind it.
    r.register("notebook.open_on_board", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            board_id: String,
            card_id: String,
        }
        let p: Which = params(p)?;
        let question = repo::read_cards(&core.store, &p.board_id)
            .map_err(store_error)?
            .into_iter()
            .find(|c| c.id == p.card_id)
            .map(|c| c.question)
            .ok_or_else(|| RpcError::core("card_missing", "That question is not in this session."))?;

        let board_id = core
            .create_board(&truncate_title(&question), "deep")
            .map_err(core_error)?;
        let outcome = core.ask(&board_id, &question, Some("deep")).map_err(core_error)?;
        Ok(json!({
            "board_id": board_id,
            "card_id": outcome.card_id,
            "status": outcome.status,
        }))
    });

    // Doc 16 section 3.4's one click way out of an ungrounded answer, which
    // waited for the web retriever and now has it. The question is asked again
    // with the web allowed, and doc 16 section 6's open point resolves as
    // proposed: the first answer is superseded rather than discarded, so the
    // session still shows that the vault had nothing to say.
    r.register("notebook.search_web", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            board_id: String,
            card_id: String,
        }
        let p: Which = params(p)?;
        if !core.retrievers.configured("web") {
            // Doc 05 section 10's `connector_unavailable`, said on the page
            // that can fix it rather than at the bottom of a card.
            return Err(RpcError::core(
                "no_web",
                "No web source is set up yet. Add one in Profile first.",
            ));
        }
        let question = repo::read_cards(&core.store, &p.board_id)
            .map_err(store_error)?
            .into_iter()
            .find(|c| c.id == p.card_id)
            .map(|c| c.question)
            .ok_or_else(|| RpcError::core("card_missing", "That question is not in this session."))?;

        let outcome = core
            .ask_on(
                &p.board_id,
                &question,
                Some("deep"),
                None,
                Anchor {
                    with_web: true,
                    ..Anchor::default()
                },
            )
            .map_err(core_error)?;

        core.store
            .append(
                tessera_store::event::NewEvent::new(
                    "notebook.superseded.v1",
                    json!({ "card_id": p.card_id, "by_card_id": outcome.card_id }),
                    tessera_store::event::Provenance::user(),
                )
                .on_board(&p.board_id)
                .on_card(&p.card_id),
            )
            .map_err(store_error)?;

        Ok(json!({ "card_id": outcome.card_id, "status": outcome.status }))
    });

    r.register("notebook.sessions", |core: &mut Core, _| {
        let boards = repo::list_boards_in(&core.store, &core.profile_id, "active", &[NOTEBOOK])
            .map_err(store_error)?;
        Ok(json!({ "sessions": boards }))
    });
}
