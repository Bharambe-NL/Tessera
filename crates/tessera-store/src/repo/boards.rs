//! Boards: the row, the listing, the pinned pack, and the trash.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::cards::{CardView, read_cards};
use super::notes::{NoteView, read_notes};
use super::sql::parse_json;
use crate::error::Result;
use crate::event::{NewEvent, Provenance, Source};
use crate::{Store, new_id, now_iso8601};

pub struct NewBoard<'a> {
    pub profile_id: &'a str,
    pub title: &'a str,
    pub doctrine_pack_id: &'a str,
    pub default_depth: &'a str,
    pub named_by_user: bool,
    pub parent_board_id: Option<&'a str>,
    pub seed_label: Option<&'a str>,
    pub context: Option<&'a str>,
}

pub fn create_board(store: &mut Store, b: NewBoard<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row_id, title, pack, depth) = (
        id.clone(),
        b.title.to_string(),
        b.doctrine_pack_id.to_string(),
        b.default_depth.to_string(),
    );
    let (profile, named, parent, seed, context) = (
        b.profile_id.to_string(),
        b.named_by_user,
        b.parent_board_id.map(str::to_string),
        b.seed_label.map(str::to_string),
        b.context.map(str::to_string),
    );

    store.append_with(
        NewEvent::new(
            "board.created.v1",
            json!({ "board_id": id, "title": b.title, "doctrine_pack_id": b.doctrine_pack_id }),
            Provenance::user(),
        )
        .on_board(&id),
        move |tx| {
            tx.execute(
                "INSERT INTO board (id, profile_id, title, named_by_user, doctrine_pack_id, context,
                                    seed_label, parent_board_id, default_depth, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    row_id,
                    profile,
                    title,
                    named as i64,
                    pack,
                    context,
                    seed,
                    parent,
                    depth,
                    now
                ],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardView {
    pub id: String,
    pub title: String,
    pub named_by_user: bool,
    pub doctrine_pack: Value,
    pub default_depth: String,
    pub mode: String,
    pub parent_board_id: Option<String>,
    pub seed_label: Option<String>,
    pub viewport: Value,
    pub cards: Vec<CardView>,
    /// Doc 01 section 4.5's stickies, which hang off the board rather than off
    /// a card even when they quote one.
    pub notes: Vec<NoteView>,
}

pub fn read_board(store: &Store, board_id: &str) -> Result<Option<BoardView>> {
    let conn = store.conn();
    let board = conn
        .query_row(
            "SELECT b.id, b.title, b.named_by_user, p.code, p.version, b.default_depth, b.mode,
                    b.parent_board_id, b.seed_label, b.viewport
             FROM board b JOIN doctrine_pack p ON p.id = b.doctrine_pack_id
             WHERE b.id = ?1",
            params![board_id],
            |r| {
                Ok(BoardView {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    named_by_user: r.get::<_, i64>(2)? != 0,
                    doctrine_pack: json!({ "code": r.get::<_, String>(3)?, "version": r.get::<_, String>(4)? }),
                    default_depth: r.get(5)?,
                    mode: r.get(6)?,
                    parent_board_id: r.get(7)?,
                    seed_label: r.get(8)?,
                    viewport: parse_json(&r.get::<_, String>(9)?),
                    cards: Vec::new(),
                    notes: Vec::new(),
                })
            },
        )
        .optional()?;

    let Some(mut board) = board else { return Ok(None) };
    board.cards = read_cards(store, board_id)?;
    board.notes = read_notes(store, board_id)?;
    Ok(Some(board))
}

/// The Home grid. Doc 09 section 3: boards with their open flag count and last
/// activity.
/// The boards a listing shows. Doc 16 section 3.4 and BN-106.
///
/// Home is the boards a person explores and learns on; the Notebook lists its
/// own sessions; and the Map, when doc 17 builds it, is a board nothing lists
/// at all. A filter rather than three queries, because the difference between
/// them is one column.
pub fn list_boards_in(store: &Store, profile_id: &str, status: &str, modes: &[&str]) -> Result<Vec<Value>> {
    let all = list_boards(store, profile_id, status)?;
    if modes.is_empty() {
        return Ok(all);
    }
    Ok(all
        .into_iter()
        .filter(|b| {
            b.get("mode")
                .and_then(Value::as_str)
                .is_some_and(|m| modes.contains(&m))
        })
        .collect())
}

pub fn list_boards(store: &Store, profile_id: &str, status: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT b.id, b.title, b.updated_at, b.mode,
                (SELECT COUNT(*) FROM card c WHERE c.board_id = b.id) AS cards,
                (SELECT COUNT(*) FROM flag f JOIN card c2 ON c2.id = f.card_id
                 WHERE c2.board_id = b.id AND f.status = 'open') AS open_flags
         FROM board b
         WHERE b.profile_id = ?1 AND b.status = ?2
         ORDER BY b.updated_at DESC",
    )?;
    Ok(stmt
        .query_map(params![profile_id, status], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "updated_at": r.get::<_, String>(2)?,
                "mode": r.get::<_, String>(3)?,
                "cards": r.get::<_, i64>(4)?,
                "open_flags": r.get::<_, i64>(5)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Open a notebook session on a board. Doc 16 section 3.4.
///
/// "Sessions are boards of `mode: notebook` so history, events, memory, and
/// export come free." The mode is what narrows retrieval, so it moves with the
/// session rather than being inferred from what the board looks like.
pub fn start_notebook(store: &mut Store, board_id: &str) -> Result<()> {
    let (board, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "notebook.asked.v1",
            json!({ "board_id": board_id, "opened": true }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET mode = 'notebook', updated_at = ?1 WHERE id = ?2",
                params![now, board],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Give a board a name the user chose.
///
/// `named_by_user` is what stops the next question overwriting it: doc 01
/// section 4.1 lets the first question title an unnamed board, and once a person
/// has typed a title that inference has to stop.
pub fn rename_board(store: &mut Store, board_id: &str, title: &str) -> Result<()> {
    let (id, name, now) = (board_id.to_string(), title.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.renamed.v1",
            json!({ "board_id": board_id, "title": title }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET title = ?1, named_by_user = 1, updated_at = ?2 WHERE id = ?3",
                params![name, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// The doctrine pack a board pinned. Doc 01 section 4.17.
#[derive(Debug, Clone)]
pub struct PinnedPack {
    pub pack_id: String,
    pub code: String,
    pub version: String,
}

pub fn board_pack(store: &Store, board_id: &str) -> Result<Option<PinnedPack>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT p.id, p.code, p.version FROM board b
               JOIN doctrine_pack p ON p.id = b.doctrine_pack_id
              WHERE b.id = ?1",
            params![board_id],
            |r| {
                Ok(PinnedPack {
                    pack_id: r.get(0)?,
                    code: r.get(1)?,
                    version: r.get(2)?,
                })
            },
        )
        .optional()?)
}

/// The cards on a board that a re-verification has something to judge.
///
/// A card with no answer has nothing for the Verifier to read, and a blocked
/// one was never admitted, so neither is re-judged by a pack update. Ordered by
/// creation so a batch runs the board top to bottom and its events read in the
/// order a person would expect.
pub fn cards_to_reverify(store: &Store, board_id: &str) -> Result<Vec<String>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id FROM card
          WHERE board_id = ?1 AND status IN ('done', 'flagged')
          ORDER BY created_at, id",
    )?;
    Ok(stmt
        .query_map(params![board_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Point a board at a newer version of the pack it pinned. Doc 10 section 9.
///
/// The pin moves only here. A pack update never rewrites it on its own, because
/// a board's answers were judged by the rules of the version it names and
/// changing that silently would make the claim untrue after the fact.
pub fn repin_board(
    store: &mut Store,
    board_id: &str,
    to: &PinnedPack,
    from_version: &str,
    cards: usize,
) -> Result<()> {
    let (id, pack_id, now) = (board_id.to_string(), to.pack_id.clone(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.pack_updated.v1",
            json!({
                "board_id": board_id,
                "pack_code": to.code,
                "from_version": from_version,
                "to_version": to.version,
                "cards_to_reverify": cards,
            }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET doctrine_pack_id = ?1, updated_at = ?2 WHERE id = ?3",
                params![pack_id, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Move a board to Trash. Doc 09 open question 1, adopted by doc 11: Trash is a
/// filter on Home rather than a rail item of its own.
///
/// Doc 09 section 5: every verb is undoable within the session except Remove on
/// a board, which goes here instead of vanishing.
pub fn trash_board(store: &mut Store, board_id: &str) -> Result<()> {
    let (id, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.trashed.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET status = 'trashed', trashed_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

pub fn restore_board(store: &mut Store, board_id: &str) -> Result<()> {
    let (id, now) = (board_id.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "board.restored.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            tx.execute(
                "UPDATE board SET status = 'active', trashed_at = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Delete a board and everything hanging from it.
///
/// The events stay. The log is append only and the database enforces it with a
/// trigger, so a purge removes the entities and leaves the trail that says they
/// existed, which is what makes `board.purged.v1` readable afterwards rather
/// than a claim about rows nobody can check.
pub fn purge_board(store: &mut Store, board_id: &str) -> Result<()> {
    let id = board_id.to_string();
    store.append_with(
        NewEvent::new(
            "board.purged.v1",
            json!({ "board_id": board_id }),
            Provenance::user(),
        )
        .on_board(board_id),
        move |tx| {
            // Cards cascade from the board, and citations and flags cascade from
            // the cards. Visuals and notes are keyed on the board the same way.
            tx.execute("DELETE FROM board WHERE id = ?1", params![id])?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Bump a board's last activity, which is what the Home grid sorts on.
pub fn touch_board(store: &Store, board_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE board SET updated_at = ?1 WHERE id = ?2",
        params![now_iso8601(), board_id],
    )?;
    Ok(())
}

/// Doc 09 section 12: board history, rendered from events with filters by agent
/// and by user action.
pub fn board_history(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    Ok(store
        .events(Some(board_id))?
        .into_iter()
        .map(|e| {
            json!({
                "event_id": e.event_id,
                "index": e.monotonic_index,
                "type": e.event_type,
                "payload": e.payload,
                "actor": e.provenance.emitter_id,
                "actor_type": e.provenance.emitter_type,
                "source": e.provenance.source,
                "card_id": e.card_id,
                "at": e.timestamp,
            })
        })
        .collect())
}

/// Test provenance never reaches a user facing history view.
pub fn is_user_visible(source: Source) -> bool {
    matches!(source, Source::Live | Source::Harness)
}
