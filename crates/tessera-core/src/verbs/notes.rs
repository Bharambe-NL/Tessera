//! Stickies: the one thing on the canvas that is entirely the person's own.
//!
//! Doc 01 section 4.5's Note. Nothing verifies it, nothing cites it, and no
//! agent reads it.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 01 section 4.5's Note, doc 16 section 3.6's "Add note". A sticky is
    // the one thing on the canvas that is entirely the person's own: nothing
    // verifies it, nothing cites it, and no agent reads it. What it does carry
    // is the card it was written beside, which is what the dashed edge draws.
    r.register("note.create", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Create {
            board_id: String,
            text: String,
            #[serde(default)]
            colour: Option<String>,
            #[serde(default)]
            position: Option<Value>,
            #[serde(default)]
            card_id: Option<String>,
        }
        let p: Create = params(p)?;
        let text = p.text.trim();
        if text.is_empty() {
            return Err(RpcError::core(
                "empty_note",
                "Write something on the sticky first.",
            ));
        }
        let note_id = repo::write_note(
            &mut core.store,
            repo::NewNote {
                board_id: &p.board_id,
                text,
                colour: p.colour.as_deref().unwrap_or(NOTE_COLOUR),
                position: p.position.clone().unwrap_or_else(default_place),
                card_id: p.card_id.as_deref(),
            },
        )
        .map_err(store_error)?;
        Ok(json!({ "note_id": note_id, "board_id": p.board_id }))
    });

    r.register("note.edit", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Edit {
            note_id: String,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            position: Option<Value>,
        }
        let p: Edit = params(p)?;
        if p.text.is_none() && p.position.is_none() {
            return Err(RpcError::core(
                "nothing_to_change",
                "Say what the sticky should become.",
            ));
        }
        repo::edit_note(&mut core.store, &p.note_id, p.text.as_deref(), p.position.clone())
            .map_err(store_error)?;
        Ok(json!({ "note_id": p.note_id }))
    });

    // Doc 09 section 5: every verb has an undo, and taking the sticky off again
    // is Add note's. There is nothing to restore afterwards, because a sticky
    // holds no work: the words were the person's and they are the ones who
    // removed them.
    r.register("note.remove", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Remove {
            note_id: String,
        }
        let p: Remove = params(p)?;
        repo::remove_note(&mut core.store, &p.note_id).map_err(store_error)?;
        Ok(json!({ "note_id": p.note_id }))
    });
}
