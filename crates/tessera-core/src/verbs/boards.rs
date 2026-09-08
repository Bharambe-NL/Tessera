//! Boards: the container every card, note and image hangs from.
//!
//! Doc 01 section 4.1's board, in the verbs Home and the canvas call. Create,
//! list and read it, rename it, move it through Trash, and put a picture or a
//! raster of the ink on it.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    r.register("board.create", |core: &mut Core, p| {
        let p: BoardCreate = params(p)?;
        let id = core
            .create_board(
                p.title.as_deref().unwrap_or("Untitled board"),
                p.depth.as_deref().unwrap_or("fast"),
            )
            .map_err(core_error)?;
        Ok(json!({ "board_id": id }))
    });

    r.register("board.list", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            /// Doc 09 open question 1: Trash is a filter on Home, so it is this
            /// word rather than a second method.
            #[serde(default = "default_board_status")]
            status: String,
            /// Which board modes to list. Empty is every mode.
            #[serde(default)]
            modes: Vec<String>,
        }
        let p: Which = params(p).unwrap_or(Which {
            status: default_board_status(),
            modes: Vec::new(),
        });
        if !matches!(p.status.as_str(), "active" | "trashed") {
            return Err(RpcError::core("unknown_status", "A board is active or trashed."));
        }
        // Doc 16 section 3.4 and BN-106: Home is explore and learn, the
        // Notebook lists its own sessions, and the Map is a board nothing lists
        // at all. Absent means every mode, so a caller that has not heard of
        // notebooks still sees what it always saw.
        let modes: Vec<&str> = p.modes.iter().map(String::as_str).collect();
        let boards =
            repo::list_boards_in(&core.store, &core.profile_id, &p.status, &modes).map_err(store_error)?;
        Ok(json!({ "boards": boards }))
    });

    r.register("board.get", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        match repo::read_board(&core.store, &p.board_id).map_err(store_error)? {
            Some(board) => {
                let mut value = serde_json::to_value(board).unwrap_or(Value::Null);
                // Doc 10 section 9: the board is where an update to the pack it
                // pinned is offered, so the read that draws the board is what
                // says one is waiting.
                value["pack_update"] = core.board_pack_update(&p.board_id).map_err(core_error)?;
                Ok(value)
            }
            None => Err(RpcError::core(
                "board_missing",
                "That board is not on this profile.",
            )),
        }
    });

    // Doc 10 section 9: "the board offers update pack, which reruns
    // verify_only". Nothing is retrieved and no answer is rewritten; the cards
    // are judged again under the version the board now names.
    r.register("board.update_pack", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        core.update_board_pack(&p.board_id).map_err(core_error)
    });

    // Doc 09 section 5's Edit verb on a board. Renaming is what turns off the
    // inference that titles an unnamed board from its first question.
    r.register("board.rename", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Rename {
            board_id: String,
            title: String,
        }
        let p: Rename = params(p)?;
        let title = p.title.trim();
        if title.is_empty() {
            return Err(RpcError::core(
                "empty_title",
                "Type a title, or leave the one the first question gave it.",
            ));
        }
        repo::rename_board(&mut core.store, &p.board_id, title).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "title": title }))
    });

    // Doc 09 section 5's Remove verb on a board, and its two undos. Doc 09 open
    // question 1, adopted by doc 11: Trash is a filter on Home rather than a
    // rail item, so these three are what that filter acts on.
    r.register("board.trash", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        repo::trash_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "trashed" }))
    });

    r.register("board.restore", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        repo::restore_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "active" }))
    });

    // The one verb with nothing behind it. A purged board is gone; its events
    // stay, because the log is append only and the database enforces that.
    r.register("board.purge", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let trashed: bool = core
            .store
            .conn()
            .query_row(
                "SELECT status = 'trashed' FROM board WHERE id = ?1",
                rusqlite::params![p.board_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !trashed {
            return Err(RpcError::core(
                "purge_needs_trash",
                "Move the board to Trash first, so a purge is never one click from a board in use.",
            ));
        }
        repo::purge_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "purged" }))
    });

    r.register("board.add_image", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct AddImage {
            board_id: String,
            data: String,
            mime: String,
            width: u32,
            height: u32,
        }
        let p: AddImage = params(p)?;
        let bytes = decode_base64(&p.data)
            .ok_or_else(|| RpcError::core("bad_image", "That image could not be read as an image."))?;
        // A bound, so a paste cannot fill the profile folder in one gesture.
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(RpcError::core(
                "image_too_large",
                "That image is over 20 MB. Scale it down and paste it again.",
            ));
        }
        let id = core
            .add_image(&p.board_id, &bytes, &p.mime, p.width, p.height)
            .map_err(core_error)?;
        Ok(json!({ "image_id": id }))
    });

    // The sketch raster path. Doc 12 phase 9 names it; the ink survives it.
    r.register("board.rasterise_ink", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let image_id = core.rasterise_ink(&p.board_id).map_err(core_error)?;
        Ok(json!({ "image_id": image_id }))
    });

    // Doc 09 section 12: board history, rendered from events.
    r.register("board.history", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let events = repo::board_history(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "events": events }))
    });

    // The events a board has produced since an index, translated for the UI.
    // Pattern 25: the protocol is a view over the log.
    r.register("board.notifications", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Since {
            board_id: String,
            #[serde(default)]
            after: i64,
        }
        let p: Since = params(p)?;
        let events = core.store.events(Some(&p.board_id)).map_err(store_error)?;
        let notifications: Vec<Value> = events
            .iter()
            .filter(|e| e.monotonic_index > p.after)
            .filter_map(crate::bridge::translate)
            .filter_map(|n| serde_json::to_value(n).ok())
            .collect();
        let latest = events.last().map(|e| e.monotonic_index).unwrap_or(p.after);
        Ok(json!({ "notifications": notifications, "index": latest }))
    });
}
