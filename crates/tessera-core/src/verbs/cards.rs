//! Cards: asking, moving, reading and keeping one answer.
//!
//! Doc 09 section 5's verbs on a card. Ask runs the pipeline, Rerun checks the
//! answer again, and Save as page hands the card to the vault.

use serde::Deserialize;
use serde_json::json;
use tessera_store::repo;

use crate::core::{Anchor, Core};
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 17 section 2.2: "a card that links the concept is read" moves it from
    // unseen to exposed, and doc 17 section 2.4 gives that reading its own
    // small, capped evidence.
    //
    // The shell decides what reading is, because only the shell can see it: doc
    // 17 open question 2 settles on a dwell of `EXPOSURE_MS`, which is a guess
    // written down as a named constant rather than a rule the core can check.
    // What the core owns is the rest: the event is the only writer of the
    // learning columns, so the fold happens where every other one does.
    // Doc 01 section 4.2: `position` carries the user's offset and whether the
    // card is pinned. The layout has honoured both since M0 and nothing ever set
    // them, because the canvas had one drag and it panned the whole world.
    r.register("card.move", |core: &mut Core, p| {
        /// Validated rather than passed through as free JSON. Doc 12 operating
        /// principle 1 validates at every boundary, and `position` is read back
        /// by the layout, so a malformed one puts a card nowhere.
        #[derive(Deserialize)]
        struct Position {
            x: f64,
            y: f64,
            dx: f64,
            dy: f64,
            pinned: bool,
        }
        #[derive(Deserialize)]
        struct Move {
            board_id: String,
            card_id: String,
            position: Position,
        }
        let p: Move = params(p)?;
        // A card of this board's, for the reason `card.viewed` gives: the
        // payload names a card and moving somebody else's is not this call.
        let exists: i64 = core
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM card WHERE id = ?1 AND board_id = ?2",
                rusqlite::params![p.card_id, p.board_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Err(RpcError::core("no_such_card", "That card is not on this board."));
        }
        let position = json!({
            "x": p.position.x,
            "y": p.position.y,
            "dx": p.position.dx,
            "dy": p.position.dy,
            "pinned": p.position.pinned,
        });
        repo::move_card(&mut core.store, &p.board_id, &p.card_id, &position).map_err(store_error)?;
        Ok(json!({ "card_id": p.card_id }))
    });

    r.register("card.viewed", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Viewed {
            board_id: String,
            card_id: String,
        }
        let p: Viewed = params(p)?;
        // A card of this profile's, because the payload names a card and an
        // event naming somebody else's would fold into their map.
        let exists: i64 = core
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM card WHERE id = ?1 AND board_id = ?2",
                rusqlite::params![p.card_id, p.board_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Err(RpcError::core("no_such_card", "That card is not on this board."));
        }
        core.store
            .append(
                tessera_store::event::NewEvent::new(
                    "card.viewed.v1",
                    json!({ "card_id": p.card_id }),
                    tessera_store::event::Provenance::user(),
                )
                .on_board(&p.board_id)
                .on_card(&p.card_id),
            )
            .map_err(store_error)?;
        Ok(json!({ "card_id": p.card_id }))
    });

    // Doc 07 section A3: "on demand from Read sketch, Read this image, or Read
    // on an Image row".
    r.register("card.read", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Read {
            board_id: String,
            image_id: String,
        }
        let p: Read = params(p)?;
        let outcome = core.read_image(&p.board_id, &p.image_id).map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags,
        }))
    });

    r.register("card.ask", |core: &mut Core, p| {
        let p: Ask = params(p)?;
        if p.question.trim().is_empty() {
            return Err(RpcError::core("empty_question", "Type a question first."));
        }
        let anchor = Anchor {
            parent_card_id: p.parent_card_id.as_deref(),
            anchor_text: p.anchor_text.as_deref(),
            anchor_block_ref: p.anchor_block_ref.as_deref(),
            with_web: false,
        };
        // An anchor names a span on a card, so without the card it names
        // nothing. Refuse here rather than storing a root card carrying a
        // pointer into a visual it has no parent to read.
        if anchor.parent_card_id.is_none() && anchor.anchored() {
            return Err(RpcError::core(
                "anchor_without_parent",
                "Branching from a highlight needs the card it was highlighted on.",
            ));
        }
        let outcome = core
            .ask_on(
                &p.board_id,
                p.question.trim(),
                p.depth.as_deref(),
                p.model.as_deref(),
                anchor,
            )
            .map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags
        }))
    });

    // Doc 09 section 5's Rerun verb on a card. Nothing is retrieved and no
    // answer is rewritten: the card is checked again against the corpus as it
    // stands, which is what a stale flag asks the reader to do.
    r.register("card.verify", |core: &mut Core, p| {
        let p: CardRef = params(p)?;
        let outcome = core.verify_card(&p.board_id, &p.card_id).map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags
        }))
    });

    // Doc 16 section 3.2's ninth verb. A card the person wants to keep becomes
    // a page they own, carrying the card's citations rather than becoming one.
    r.register("card.save_as_page", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Save {
            board_id: String,
            card_id: String,
        }
        let p: Save = params(p)?;
        let card = repo::read_cards(&core.store, &p.board_id)
            .map_err(store_error)?
            .into_iter()
            .find(|c| c.id == p.card_id)
            .ok_or_else(|| RpcError::core("card_missing", "That card is not on this board."))?;

        if let Some(existing) = card.page_id.clone() {
            // Saving twice is not an error and not a second page: the person
            // pressed a button whose work was already done.
            let page = repo::read_page(&core.store, &existing).map_err(store_error)?;
            return Ok(json!({
                "page_id": existing,
                "title": page.as_ref().map(|p| p.title.clone()),
                "file_path": page.map(|p| p.file_path),
                "created": false,
            }));
        }

        let pack_id = core.active_pack_id().map_err(core_error)?;
        let profile_id = core.profile_id.clone();
        let page_id = crate::vault::save_card_as_page(&mut core.store, &profile_id, Some(&pack_id), &card)
            .map_err(|e| match e {
                crate::vault::SaveError::Refused(why) => RpcError::core(
                    "card_not_saveable",
                    match why {
                        crate::vault::BLOCKED => {
                            "This card is blocked, so it stays on the board until the flag is decided."
                        }
                        _ => "This card has no answer yet, so there is nothing to keep.",
                    },
                ),
                crate::vault::SaveError::Store(e) => store_error(e),
            })?;

        let page = repo::read_page(&core.store, &page_id).map_err(store_error)?;
        Ok(json!({
            "page_id": page_id,
            "title": page.as_ref().map(|p| p.title.clone()),
            "file_path": page.as_ref().map(|p| p.file_path.clone()),
            "citations_carried": page
                .as_ref()
                .and_then(|p| p.citations_carried.as_array().map(Vec::len))
                .unwrap_or(0),
            "created": true,
        }))
    });
}
