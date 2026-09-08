//! Stickies: the note a person puts on a board.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

// --------------------------------------------------------------- stickies ---
//
// Doc 01 section 4.5's Note, which doc 16 section 7 point 1 calls a sticky
// everywhere a person can read it, to keep it apart from a vault page. The
// table has existed since 0001 with nothing writing to it, so `note.added.v1`
// was in the vocabulary and had never once been emitted.

/// A sticky as it is put on the board.
pub struct NewNote<'a> {
    pub board_id: &'a str,
    pub text: &'a str,
    /// A palette token name, doc 01 section 4.5.
    pub colour: &'a str,
    /// `{x, y, w, h}` in board coordinates.
    pub position: Value,
    /// The card this sticky is about, when it came from a quote. Doc 16 section
    /// 3.6 draws the dashed edge from this.
    pub card_id: Option<&'a str>,
}

/// One sticky as it is read back.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteView {
    pub id: String,
    pub text: String,
    pub colour: String,
    pub position: Value,
    pub card_id: Option<String>,
}

pub fn write_note(store: &mut Store, n: NewNote<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row, board, text, colour, position, card, at) = (
        id.clone(),
        n.board_id.to_string(),
        n.text.to_string(),
        n.colour.to_string(),
        n.position.to_string(),
        n.card_id.map(str::to_string),
        now,
    );

    let mut event = NewEvent::new(
        "note.added.v1",
        json!({
            "note_id": id,
            "board_id": n.board_id,
            "card_id": n.card_id,
            // The words are the person's own and the log is not where they are
            // kept: the row is. What the event records is that a sticky exists
            // and what it is about.
            "characters": n.text.chars().count(),
        }),
        Provenance::user(),
    )
    .on_board(n.board_id);
    if let Some(card_id) = n.card_id {
        event = event.on_card(card_id);
    }

    store.append_with(event, move |tx| {
        tx.execute(
            "INSERT INTO note (id, board_id, text, colour, position, card_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![row, board, text, colour, position, card, at],
        )?;
        Ok(())
    })?;
    Ok(id)
}

/// Replace a sticky's text, or move it, or both.
pub fn edit_note(
    store: &mut Store,
    note_id: &str,
    text: Option<&str>,
    position: Option<Value>,
) -> Result<()> {
    let (id, body, place, now) = (
        note_id.to_string(),
        text.map(str::to_string),
        position.map(|p| p.to_string()),
        now_iso8601(),
    );
    store.append_with(
        NewEvent::new(
            "note.edited.v1",
            json!({
                "note_id": note_id,
                "text_changed": body.is_some(),
                "moved": place.is_some(),
            }),
            Provenance::user(),
        ),
        move |tx| {
            // Two columns, either of which may be left alone. COALESCE rather
            // than two statements, so a move and a rewrite in one call are one
            // write and one row version.
            tx.execute(
                "UPDATE note SET text = COALESCE(?1, text), position = COALESCE(?2, position),
                     updated_at = ?3 WHERE id = ?4",
                params![body, place, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Take a sticky off the board. Doc 09 section 5: every verb has an undo, and
/// this is what Add note's is.
pub fn remove_note(store: &mut Store, note_id: &str) -> Result<()> {
    let id = note_id.to_string();
    // Read the board before the row is gone. An event with no board is an event
    // the board's own history never shows, which for a verb whose whole point
    // is being undoable is the one place it has to appear.
    let board: Option<String> = store
        .conn()
        .query_row("SELECT board_id FROM note WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()?;

    let mut event = NewEvent::new(
        "note.removed.v1",
        json!({ "note_id": note_id }),
        Provenance::user(),
    );
    if let Some(board_id) = &board {
        event = event.on_board(board_id);
    }

    store.append_with(event, move |tx| {
        tx.execute("DELETE FROM note WHERE id = ?1", params![id])?;
        Ok(())
    })?;
    Ok(())
}

/// The stickies on a board, oldest first.
pub fn read_notes(store: &Store, board_id: &str) -> Result<Vec<NoteView>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, text, colour, position, card_id FROM note
         WHERE board_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![board_id], |r| {
        let position: String = r.get(3)?;
        Ok(NoteView {
            id: r.get(0)?,
            text: r.get(1)?,
            colour: r.get(2)?,
            position: serde_json::from_str(&position).unwrap_or_else(|_| json!({})),
            card_id: r.get(4)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}
