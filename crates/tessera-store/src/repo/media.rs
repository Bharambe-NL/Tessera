//! Images and ink, and the Reader card that finishes over one.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::cards::CardRef;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

/// Store an image and the row that points at it. Doc 01 section 4.6.
///
/// The bytes go to the blob store by hash, so a board forked from a bundle never
/// duplicates a picture, and the row carries the size the Reader's packet needs.
pub struct NewImage<'a> {
    pub board_id: &'a str,
    pub origin: &'a str,
    pub bytes: &'a [u8],
    pub mime: &'a str,
    pub width: u32,
    pub height: u32,
    pub source_ink_ids: Option<&'a str>,
}

pub fn write_image(store: &mut Store, image: NewImage<'_>) -> Result<String> {
    let blob_ref = store.blobs().put(image.bytes)?;
    let id = new_id();
    let now = now_iso8601();
    let (row, board, origin, blob, mime, ink) = (
        id.clone(),
        image.board_id.to_string(),
        image.origin.to_string(),
        blob_ref.clone(),
        image.mime.to_string(),
        image.source_ink_ids.map(str::to_string),
    );
    let (w, h) = (image.width as i64, image.height as i64);

    let event = match image.origin {
        // Doc 01 section 6.3 has two names here and they mean different things:
        // one is a person putting a picture on a board, the other is the product
        // making one.
        "generated" => "image.generated.v1",
        "sketch_raster" => "sketch.rasterised.v1",
        _ => "image.pasted.v1",
    };

    store.append_with(
        NewEvent::new(
            event,
            json!({
                "image_id": id, "origin": image.origin, "blob_ref": blob_ref,
                "mime": image.mime, "width": image.width, "height": image.height
            }),
            Provenance::user(),
        )
        .on_board(image.board_id),
        move |tx| {
            tx.execute(
                "INSERT INTO image (id, board_id, origin, blob_ref, mime, width, height,
                                    position, source_ink_ids, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{\"x\":0,\"y\":0}', ?8, ?9)",
                params![row, board, origin, blob, mime, w, h, ink, now],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// One image row, with its bytes. What doc 07 section A4's packet is built from.
pub fn read_image(store: &Store, image_id: &str) -> Result<Option<(Value, Vec<u8>)>> {
    let row: Option<(String, String, String, i64, i64, String)> = store
        .conn()
        .query_row(
            "SELECT id, origin, blob_ref, width, height, mime FROM image WHERE id = ?1",
            params![image_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;

    let Some((id, origin, blob_ref, width, height, mime)) = row else {
        return Ok(None);
    };
    let bytes = store.blobs().get(&blob_ref)?;
    Ok(Some((
        json!({
            "image_id": id,
            "origin": origin,
            "blob_ref": blob_ref,
            "mime": mime,
            "width": width,
            "height": height,
        }),
        bytes,
    )))
}

/// The ink a board holds, for the sketch raster path.
pub fn read_ink(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt =
        conn.prepare("SELECT points, colour, width FROM ink WHERE board_id = ?1 ORDER BY created_at")?;
    Ok(stmt
        .query_map(params![board_id], |r| {
            let points: String = r.get(0)?;
            Ok(json!({
                "points": serde_json::from_str::<Value>(&points).unwrap_or_else(|_| json!([])),
                "colour": r.get::<_, String>(1)?,
                "width": r.get::<_, f64>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Finish a Reader card. Doc 07 section A7's `read.completed.v1`.
pub struct ReadResult<'a> {
    pub image_id: &'a str,
    pub kind: &'a str,
    pub legibility: f64,
    pub injection_suspected: bool,
    pub notable: &'a Value,
}

pub fn finish_read(store: &mut Store, at: CardRef<'_>, result: ReadResult<'_>) -> Result<()> {
    store.append(
        NewEvent::new(
            "read.completed.v1",
            json!({
                "card_id": at.card_id,
                "image_id": result.image_id,
                "kind": result.kind,
                "legibility": result.legibility,
                "injection_suspected": result.injection_suspected,
                "notable_count": result.notable.as_array().map(Vec::len).unwrap_or(0),
            }),
            Provenance::agent("reader", at.run_id.to_string()),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}
