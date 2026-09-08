//! The vault: pages, the links out of them, and the links back in.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::sql::parse_json;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

// ------------------------------------------------------------------ vault ---

/// A page as it is written. Doc 16 section 3.1.
pub struct NewPage<'a> {
    pub profile_id: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    /// `vault/<slug>.md`, relative to the profile folder. The caller computes
    /// it, because the slug rule belongs with the mirror that writes the file
    /// and not with the table that indexes it.
    pub file_path: &'a str,
    /// Set by Save as page. Doc 16 section 3.2.
    pub source_card_id: Option<&'a str>,
    /// `[{ordinal, passage_id}]`, copied from the card. Doc 16 section 2.2: a
    /// page is context and the passages it carries are the evidence, so they
    /// are copied once and never re-derived from the page's own text.
    pub citations_carried: Value,
    pub doctrine_pack_id: Option<&'a str>,
}

/// A page as it is read.
#[derive(Debug, Clone)]
pub struct PageRow {
    pub id: String,
    pub title: String,
    pub body: String,
    pub file_path: String,
    pub source_card_id: Option<String>,
    pub citations_carried: Value,
    /// The hash of the text this row and its file last agreed on.
    ///
    /// Not the hash of `body`, which is derivable from `body`. Deciding which
    /// of the two copies moved needs a third value, and this is it: an edit in
    /// the app leaves it alone, and the mirror writes it when it reconciles.
    pub synced_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

/// What a title collision looks like to a caller.
///
/// Doc 16 section 3.1 makes the title unique per profile and case insensitive,
/// so this is a rule the person meets rather than a constraint error they
/// should have to read.
pub const PAGE_TITLE_TAKEN: &str = "page_title_taken";

fn page_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PageRow> {
    Ok(PageRow {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        file_path: r.get(3)?,
        source_card_id: r.get(4)?,
        citations_carried: parse_json(&r.get::<_, String>(5)?),
        synced_hash: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

const PAGE_COLUMNS: &str = "id, title, body, file_path, source_card_id, citations_carried,
     synced_hash, created_at, updated_at";

/// Write a page. Doc 16 section 3.1.
///
/// The event depends on where the page came from: a page saved from a card is
/// `page.created_from_card.v1`, which doc 16 section 3.2 names, and one written
/// by hand is `page.created.v1`. One writer, two events, because the two are
/// different claims about where the text came from and the log is what a person
/// reads to find out.
pub fn create_page(store: &mut Store, p: NewPage<'_>) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    // The file does not exist yet and the mirror writes it from this body, so
    // the two agree on it from the start. A sync that crashes before the write
    // finds the file missing next time and writes it, which lands in the same
    // place.
    let hash = crate::blob::BlobStore::hash(p.body.as_bytes());
    let event_type = if p.source_card_id.is_some() {
        "page.created_from_card.v1"
    } else {
        "page.created.v1"
    };

    let (row_id, profile_id, title, body, file_path, card, carried, pack, hash_for_row, now_for_row) = (
        id.clone(),
        p.profile_id.to_string(),
        p.title.trim().to_string(),
        p.body.to_string(),
        p.file_path.to_string(),
        p.source_card_id.map(str::to_string),
        p.citations_carried.to_string(),
        p.doctrine_pack_id.map(str::to_string),
        hash.clone(),
        now.clone(),
    );

    let mut event = NewEvent::new(
        event_type,
        json!({
            "page_id": id,
            "profile_id": p.profile_id,
            "title": p.title.trim(),
            "file_path": p.file_path,
            "source_card_id": p.source_card_id,
            "citations_carried": p.citations_carried.as_array().map(Vec::len).unwrap_or(0),
        }),
        Provenance::user(),
    );
    if let Some(card_id) = p.source_card_id {
        event = event.on_card(card_id);
    }

    store.append_with(event, move |tx| {
        tx.execute(
            "INSERT INTO page (id, profile_id, title, body, file_path, source_card_id,
                 citations_carried, doctrine_pack_id, synced_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                row_id,
                profile_id,
                title,
                body,
                file_path,
                card,
                carried,
                pack,
                hash_for_row,
                now_for_row
            ],
        )?;
        Ok(())
    })?;
    Ok(id)
}

/// Replace a page's body. Doc 16 section 4's `page.edited.v1`.
///
/// `synced_hash` is deliberately left where it is. It records what the row and
/// the file last agreed on, so an edit here is exactly the event that makes
/// them disagree, and moving it would erase the evidence the mirror reads.
pub fn edit_page(store: &mut Store, page_id: &str, body: &str) -> Result<()> {
    let (id, text, now) = (page_id.to_string(), body.to_string(), now_iso8601());
    let hash = crate::blob::BlobStore::hash(body.as_bytes());
    store.append_with(
        NewEvent::new(
            "page.edited.v1",
            json!({ "page_id": page_id, "body_hash": hash, "length": body.len() }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET body = ?1, updated_at = ?2 WHERE id = ?3",
                params![text, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Record that the row and its file now hold the same text.
///
/// Written by the mirror after it reconciles them, and by nothing else. Doc 16
/// section 7 point 2's conflict rule is decidable only while this is true of
/// the last agreement rather than of the newest write.
pub fn mark_page_synced(store: &Store, page_id: &str, body: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE page SET synced_hash = ?1 WHERE id = ?2",
        params![crate::blob::BlobStore::hash(body.as_bytes()), page_id],
    )?;
    Ok(())
}

/// Take the file's text as the page's, when the mirror finds the file moved and
/// the row did not.
///
/// One statement, because the body and the agreement move together here: the
/// row now says what the file says.
pub fn adopt_page_body(store: &mut Store, page_id: &str, body: &str) -> Result<()> {
    let (id, text, now) = (page_id.to_string(), body.to_string(), now_iso8601());
    let hash = crate::blob::BlobStore::hash(body.as_bytes());
    let hash_for_row = hash.clone();
    store.append_with(
        NewEvent::new(
            "page.edited.v1",
            json!({
                "page_id": page_id,
                "body_hash": hash,
                "length": body.len(),
                // The one edit nobody made in the app. Doc 16 section 3.1: the
                // vault is the person's even without Tessera running.
                "edited_in": "vault",
            }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET body = ?1, synced_hash = ?2, updated_at = ?3 WHERE id = ?4",
                params![text, hash_for_row, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Rename a page, keeping its id. Doc 16 section 3.1.
///
/// The id is what a wikilink resolves to, which is the whole reason a rename is
/// not a delete and a create: doc 16 section 2.2 lists resolution by title
/// string as one of the package's mistakes, because renames silently break the
/// links into it.
pub fn rename_page(store: &mut Store, page_id: &str, title: &str, file_path: &str) -> Result<()> {
    let (id, name, path, now) = (
        page_id.to_string(),
        title.trim().to_string(),
        file_path.to_string(),
        now_iso8601(),
    );
    store.append_with(
        NewEvent::new(
            "page.renamed.v1",
            json!({ "page_id": page_id, "title": title.trim(), "file_path": file_path }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "UPDATE page SET title = ?1, file_path = ?2, updated_at = ?3 WHERE id = ?4",
                params![name, path, now, id],
            )?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Delete a page. Doc 16 section 2.1: a deleted page must not corrupt an answer
/// that cited it, and it cannot, because a citation names a Passage and the
/// passage carries its own verbatim text.
pub fn delete_page(store: &mut Store, page_id: &str) -> Result<()> {
    let id = page_id.to_string();
    store.append_with(
        NewEvent::new(
            "page.deleted.v1",
            json!({ "page_id": page_id }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute("DELETE FROM page WHERE id = ?1", params![id])?;
            Ok(())
        },
    )?;
    Ok(())
}

pub fn read_page(store: &Store, page_id: &str) -> Result<Option<PageRow>> {
    let conn = store.conn();
    Ok(conn
        .query_row(
            &format!("SELECT {PAGE_COLUMNS} FROM page WHERE id = ?1"),
            params![page_id],
            page_row,
        )
        .optional()?)
}

/// The page with this title, case insensitively. Doc 16 section 3.1's
/// uniqueness rule read from the other side: this is what a wikilink resolves
/// through and what a create checks before it collides.
pub fn page_by_title(store: &Store, profile_id: &str, title: &str) -> Result<Option<PageRow>> {
    let conn = store.conn();
    Ok(conn
        .query_row(
            &format!(
                "SELECT {PAGE_COLUMNS} FROM page
                  WHERE profile_id = ?1 AND title = ?2 COLLATE NOCASE"
            ),
            params![profile_id, title.trim()],
            page_row,
        )
        .optional()?)
}

/// Every page in the profile, most recently edited first.
pub fn list_pages(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<PageRow>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(&format!(
        "SELECT {PAGE_COLUMNS} FROM page WHERE profile_id = ?1
          ORDER BY updated_at DESC, title LIMIT ?2"
    ))?;
    Ok(stmt
        .query_map(params![profile_id, limit], page_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One link out of a page, resolved. Doc 16 section 3.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPageLink {
    /// `page`, `concept` or `unresolved`.
    pub target_kind: String,
    /// The row the link points at, or `None` when nothing carries that title
    /// yet. Doc 16 section 3.1: clicking an unresolved link creates the page.
    pub target_id: Option<String>,
    /// The title the link names, which is what an unresolved one is waiting for
    /// and what clicking it would create. `[[Liquidity risk|the rule]]` names
    /// the first and shows the second.
    pub target_title: String,
    pub display_text: String,
    pub position: i64,
}

/// One link into something, with the page it came from. Doc 16 section 2.1's
/// backlinks panel.
#[derive(Debug, Clone)]
pub struct Backlink {
    pub page_id: String,
    pub page_title: String,
    pub display_text: String,
    pub position: i64,
}

/// Replace a page's outbound links with the ones its body now carries.
///
/// Replace rather than merge: the body is the truth about what it links to, and
/// a link the person deleted has to stop appearing in the target's backlinks.
///
/// One event per kind per save rather than one per link. A page with twenty
/// links would otherwise write twenty events on every keystroke-sized edit, and
/// what a person reads the log for is that a save resolved links or left some
/// hanging, not the twenty.
pub fn replace_page_links(store: &mut Store, page_id: &str, links: &[NewPageLink]) -> Result<()> {
    let now = now_iso8601();
    let resolved: Vec<&NewPageLink> = links.iter().filter(|l| l.target_id.is_some()).collect();
    let unresolved: Vec<&str> = links
        .iter()
        .filter(|l| l.target_id.is_none())
        .map(|l| l.target_title.as_str())
        .collect();

    let rows: Vec<(String, String, Option<String>, String, String, i64)> = links
        .iter()
        .map(|l| {
            (
                new_id(),
                l.target_kind.clone(),
                l.target_id.clone(),
                l.target_title.clone(),
                l.display_text.clone(),
                l.position,
            )
        })
        .collect();
    let id = page_id.to_string();
    let when = now.clone();

    let payload = json!({
        "page_id": page_id,
        "resolved": resolved.len(),
        "unresolved": unresolved.len(),
        "titles": unresolved,
    });
    let event_type = if unresolved.is_empty() {
        "page.link_resolved.v1"
    } else {
        // Doc 16 section 3.1: an unresolved link is kept and created on click,
        // so the save that left one is the thing worth recording.
        "page.link_unresolved.v1"
    };

    store.append_with(
        NewEvent::new(event_type, payload, Provenance::user()),
        move |tx| {
            tx.execute("DELETE FROM page_link WHERE from_page_id = ?1", params![id])?;
            for (link_id, kind, target, title, text, position) in rows {
                tx.execute(
                    "INSERT INTO page_link (id, from_page_id, target_kind, target_id,
                         target_title, display_text, position, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![link_id, id, kind, target, title, text, position, when],
                )?;
            }
            Ok(())
        },
    )?;
    Ok(())
}

/// The links out of one page, in body order.
pub fn page_links(store: &Store, page_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT target_kind, target_id, target_title, display_text, position FROM page_link
          WHERE from_page_id = ?1 ORDER BY position",
    )?;
    Ok(stmt
        .query_map(params![page_id], |r| {
            Ok(json!({
                "target_kind": r.get::<_, String>(0)?,
                "target_id": r.get::<_, Option<String>>(1)?,
                "target_title": r.get::<_, String>(2)?,
                "display_text": r.get::<_, String>(3)?,
                "position": r.get::<_, i64>(4)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Every link into a page or a concept. Doc 16 section 2.1.
///
/// A query over `page_link`, never a scan over bodies, which is what the
/// `page_link_target` index is for and what doc 16 phase 12c accepts on.
pub fn backlinks(store: &Store, target_kind: &str, target_id: &str) -> Result<Vec<Backlink>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT l.from_page_id, p.title, l.display_text, l.position
           FROM page_link l JOIN page p ON p.id = l.from_page_id
          WHERE l.target_kind = ?1 AND l.target_id = ?2
          ORDER BY p.title, l.position",
    )?;
    Ok(stmt
        .query_map(params![target_kind, target_id], |r| {
            Ok(Backlink {
                page_id: r.get(0)?,
                page_title: r.get(1)?,
                display_text: r.get(2)?,
                position: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Light up the links that were waiting for this title.
///
/// A person writes `[[Basel III]]` before the page exists, and doc 16 section
/// 3.1 keeps the link rather than dropping it. When the page arrives, by hand
/// or from the vault, the links into it stop being unresolved. Matching is on
/// the title the link named, not on what it displayed, which is why 0007 stores
/// it.
///
/// Returns how many links resolved.
pub fn resolve_pending_links(store: &Store, kind: &str, target_id: &str, title: &str) -> Result<usize> {
    Ok(store.conn().execute(
        "UPDATE page_link SET target_kind = ?1, target_id = ?2
          WHERE target_kind = 'unresolved' AND target_title = ?3 COLLATE NOCASE",
        params![kind, target_id, title.trim()],
    )?)
}

/// Point a card at the page it was saved as. Doc 16 section 4.
pub fn set_card_page(store: &Store, card_id: &str, page_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE card SET page_id = ?1 WHERE id = ?2",
        params![page_id, card_id],
    )?;
    Ok(())
}
