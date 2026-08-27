//! Writing a board out. Doc 01 section 7.

use std::collections::BTreeSet;
use std::io::{Seek, Write};

use serde_json::{Value, json};
use tessera_store::event::{NewEvent, Provenance};
use tessera_store::{BlobStore, Store};
use zip::write::SimpleFileOptions;

use crate::rows::query;
use crate::{BundleError, FORMAT_VERSION, Result};

/// What the exporter was told to leave out.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Doc 01 section 7's "export without history": drops `events.jsonl` and
    /// every task packet with it.
    pub with_history: bool,
    /// Doc 01 section 7's checklist answer: source ids the author cleared. A
    /// local document not named here does not travel.
    pub local_documents: BTreeSet<String>,
    /// Display name only. Doc 10 section 8: a bundle carries no other identity.
    pub exported_by: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            with_history: true,
            // Empty rather than everything. A checklist whose default is "send
            // it all" is not a checklist, and this is the one setting where the
            // wrong default leaks a person's own documents to a stranger.
            local_documents: BTreeSet::new(),
            exported_by: None,
        }
    }
}

/// One local document the author has to decide about before export.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalDocument {
    pub source_id: String,
    /// The file name and nothing above it. Doc 01 section 7 forbids the path.
    pub file_name: String,
    /// True for a folder marked sensitive: this one cannot carry its text at
    /// all, whatever the author decides about including it.
    pub text_withheld: bool,
    pub passages: usize,
}

/// What the exporter shows before it writes anything. Doc 01 section 7.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Preflight {
    pub board_id: String,
    pub board_title: String,
    pub cards: usize,
    pub sources: usize,
    pub local_documents: Vec<LocalDocument>,
}

/// Read what an export would carry, without writing it.
///
/// Separate from [`export`] because doc 01 section 7 asks for a checklist "so
/// nothing leaves by accident", and a checklist shown after the file is written
/// is a receipt.
pub fn preflight(store: &Store, board_id: &str) -> Result<Preflight> {
    let board = board_row(store, board_id)?;
    let cards = query(
        store.conn(),
        "SELECT id FROM card WHERE board_id = ?1",
        &[&board_id],
    )?;
    let sources = cited_sources(store, board_id)?;

    let mut local = Vec::new();
    for source in &sources {
        if source["class"].as_str() != Some("local_document") {
            continue;
        }
        let id = source["id"].as_str().unwrap_or_default().to_string();
        let passages = query(
            store.conn(),
            "SELECT id, text_withheld FROM passage WHERE source_id = ?1",
            &[&id],
        )?;
        local.push(LocalDocument {
            file_name: file_name_of(source["locator"].as_str().unwrap_or_default()),
            text_withheld: passages
                .iter()
                .any(|p| p["text_withheld"].as_i64() == Some(1)),
            passages: passages.len(),
            source_id: id,
        });
    }

    Ok(Preflight {
        board_id: board_id.to_string(),
        board_title: board["title"].as_str().unwrap_or_default().to_string(),
        cards: cards.len(),
        sources: sources.len(),
        local_documents: local,
    })
}

/// Write the board to `sink` as a bundle zip, and return its manifest.
pub fn export<W: Write + Seek>(
    store: &mut Store,
    registry: &tessera_schema::Registry,
    board_id: &str,
    options: &ExportOptions,
    sink: W,
) -> Result<Value> {
    let board = board_row(store, board_id)?;
    let conn = store.conn();

    // Everything hanging off the board's cards, in one place so the joins are
    // written once. Every one of these filters through `card.board_id`: a
    // bundle carries this board and nothing beside it.
    let cards = query(conn, "SELECT * FROM card WHERE board_id = ?1", &[&board_id])?;
    let visuals = query(
        conn,
        "SELECT v.* FROM visual v JOIN card c ON c.id = v.card_id WHERE c.board_id = ?1",
        &[&board_id],
    )?;
    let citations = query(
        conn,
        "SELECT ci.* FROM citation ci JOIN card c ON c.id = ci.card_id WHERE c.board_id = ?1",
        &[&board_id],
    )?;
    let flags = query(
        conn,
        "SELECT f.* FROM flag f JOIN card c ON c.id = f.card_id WHERE c.board_id = ?1",
        &[&board_id],
    )?;
    let reviews = query(
        conn,
        "SELECT DISTINCT r.* FROM review r
         JOIN flag f ON f.review_id = r.id
         JOIN card c ON c.id = f.card_id WHERE c.board_id = ?1",
        &[&board_id],
    )?;
    let ink = query(conn, "SELECT * FROM ink WHERE board_id = ?1", &[&board_id])?;
    let notes = query(conn, "SELECT * FROM note WHERE board_id = ?1", &[&board_id])?;
    let images = query(conn, "SELECT * FROM image WHERE board_id = ?1", &[&board_id])?;
    let exercises = query(
        conn,
        "SELECT * FROM exercise WHERE board_id = ?1",
        &[&board_id],
    )?;

    // Concepts linked from this board's cards, and the links themselves. Doc 01
    // section 4.7 keeps concepts on the profile, so the filter is the link.
    let concepts = query(
        conn,
        "SELECT DISTINCT co.* FROM concept co
         JOIN concept_link l ON l.concept_id = co.id
         JOIN card c ON c.id = l.target_ref
         WHERE l.target_type = 'card' AND c.board_id = ?1",
        &[&board_id],
    )?;
    let concept_links = query(
        conn,
        "SELECT DISTINCT l.* FROM concept_link l
         JOIN card c ON c.id = l.target_ref
         WHERE l.target_type = 'card' AND c.board_id = ?1",
        &[&board_id],
    )?;

    // Sources and passages: only what this board cites, and only what the
    // author cleared. Doc 01 section 7.
    let mut sources = Vec::new();
    let mut withheld: BTreeSet<String> = BTreeSet::new();
    for source in cited_sources(store, board_id)? {
        let id = source["id"].as_str().unwrap_or_default().to_string();
        if source["class"].as_str() == Some("local_document")
            && !options.local_documents.contains(&id)
        {
            withheld.insert(id);
            continue;
        }
        sources.push(redact_source(source));
    }
    let kept: BTreeSet<String> = sources
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect();
    let passages: Vec<Value> = cited_passages(store, board_id)?
        .into_iter()
        .filter(|p| {
            p["source_id"]
                .as_str()
                .is_some_and(|id| kept.contains(id))
        })
        .map(redact_passage)
        .collect();

    // A citation whose source the author withheld would point at nothing. It
    // travels anyway, because dropping it would quietly change the card's
    // answer: the marker is rendered from the citation, and a card that cited
    // four things and arrives citing three reads as a card that claimed less.
    // The importer resolves it as a citation with no passage, which is visibly
    // missing rather than invisibly absent.

    let events = if options.with_history {
        store
            .events(Some(board_id))?
            .into_iter()
            .map(|e| serde_json::to_value(&e))
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    // Blobs: the bytes the rows point at, by hash. Doc 01 section 4.6 stores
    // images once by hash so a forked board never duplicates them.
    let digests: BTreeSet<String> = images
        .iter()
        .filter_map(|i| i["blob_ref"].as_str().map(str::to_string))
        .collect();

    let bundle_id = tessera_store::new_id();
    let exported_at = tessera_store::now_iso8601();
    let pack = query(
        conn,
        "SELECT code, version FROM doctrine_pack WHERE id = ?1",
        &[&board["doctrine_pack_id"].as_str().unwrap_or_default()],
    )?
    .into_iter()
    .next()
    .unwrap_or_else(|| json!({ "code": "general", "version": "unknown" }));

    let files: Vec<(&str, &Vec<Value>)> = vec![
        ("cards.jsonl", &cards),
        ("visuals.jsonl", &visuals),
        ("citations.jsonl", &citations),
        ("flags.jsonl", &flags),
        ("reviews.jsonl", &reviews),
        ("ink.jsonl", &ink),
        ("notes.jsonl", &notes),
        ("images.jsonl", &images),
        ("sources.jsonl", &sources),
        ("passages.jsonl", &passages),
        ("concepts.jsonl", &concepts),
        ("concept_links.jsonl", &concept_links),
        ("exercises.jsonl", &exercises),
        ("events.jsonl", &events),
    ];

    let mut counts = serde_json::Map::new();
    for (name, rows) in &files {
        counts.insert((*name).to_string(), json!(rows.len()));
    }
    counts.insert("blobs".into(), json!(digests.len()));

    let preflight = preflight(store, board_id)?;
    let mut manifest = json!({
        "bundle_id": bundle_id,
        "format_version": FORMAT_VERSION,
        "exported_at": exported_at,
        // Absent, not null. The schema types this as a string, and a null here
        // would say the author has a display name and it is nothing.
        "board_id": board_id,
        "board_title": board["title"],
        "doctrine_pack": { "code": pack["code"], "version": pack["version"] },
        "includes": {
            "cards": true, "visuals": true, "citations": true, "flags": true,
            "reviews": true, "ink": true, "notes": true, "images": true,
            "sources": true, "passages": true, "concepts": true,
            "exercises": true, "events": options.with_history
        },
        "counts": Value::Object(counts),
        "local_documents": preflight.local_documents.iter().map(|d| json!({
            "source_id": d.source_id,
            "file_name": d.file_name,
            "included": !withheld.contains(&d.source_id),
            "text_withheld": d.text_withheld
        })).collect::<Vec<_>>(),
        "blobs": digests.iter().map(|digest| {
            let bytes = store.blobs().get(digest).map(|b| b.len()).unwrap_or(0);
            json!({ "sha256": digest, "bytes": bytes })
        }).collect::<Vec<_>>(),
    });
    if let Some(name) = &options.exported_by {
        manifest["exported_by"] = json!(name);
    }

    // Doc 12 operating principle 1: validate at every boundary. A bundle is a
    // boundary in the strongest sense, because the thing on the far side of it
    // is a stranger's build, and the manifest is the only part of the archive
    // that tells them what the rest should be.
    registry
        .validate(tessera_schema::ids::BUNDLE_MANIFEST, &manifest)
        .map_err(|e| BundleError::BadManifest(e.to_string()))?;

    write_archive(sink, &manifest, &board, &files, &digests, store.blobs())?;

    store.append(
        NewEvent::new(
            "board.exported.v1",
            json!({
                "board_id": board_id,
                "bundle_id": bundle_id,
                "with_history": options.with_history,
                "local_documents_withheld": withheld.len(),
            }),
            Provenance::user(),
        )
        .on_board(board_id),
    )?;

    Ok(manifest)
}

fn write_archive<W: Write + Seek>(
    sink: W,
    manifest: &Value,
    board: &Value,
    files: &[(&str, &Vec<Value>)],
    digests: &BTreeSet<String>,
    blobs: &BlobStore,
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(sink);
    // Deflate rather than stored: passages are the bulk of a bundle and they
    // are prose.
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("manifest.json", options)?;
    zip.write_all(serde_json::to_string_pretty(manifest)?.as_bytes())?;

    zip.start_file("board.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&redact_board(board.clone()))?.as_bytes())?;

    for (name, rows) in files {
        zip.start_file(*name, options)?;
        for row in *rows {
            zip.write_all(serde_json::to_string(row)?.as_bytes())?;
            zip.write_all(b"\n")?;
        }
    }

    for digest in digests {
        let bytes = blobs.get(digest)?;
        zip.start_file(format!("blobs/{digest}"), options)?;
        zip.write_all(&bytes)?;
    }

    zip.finish()?;
    Ok(())
}

fn board_row(store: &Store, board_id: &str) -> Result<Value> {
    query(
        store.conn(),
        "SELECT * FROM board WHERE id = ?1",
        &[&board_id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| BundleError::NoBoard(board_id.to_string()))
}

/// Sources this board cites, through citation and passage.
fn cited_sources(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    query(
        store.conn(),
        "SELECT DISTINCT s.* FROM source s
         JOIN passage p ON p.source_id = s.id
         JOIN citation ci ON ci.passage_id = p.id
         JOIN card c ON c.id = ci.card_id
         WHERE c.board_id = ?1",
        &[&board_id],
    )
}

fn cited_passages(store: &Store, board_id: &str) -> Result<Vec<Value>> {
    query(
        store.conn(),
        "SELECT DISTINCT p.* FROM passage p
         JOIN citation ci ON ci.passage_id = p.id
         JOIN card c ON c.id = ci.card_id
         WHERE c.board_id = ?1",
        &[&board_id],
    )
}

/// Drop what a bundle never carries about the board itself.
///
/// `profile_id` is the sender's profile row, which means nothing on the
/// recipient's machine and names a person's install if it travels.
fn redact_board(mut board: Value) -> Value {
    if let Some(object) = board.as_object_mut() {
        object.remove("profile_id");
    }
    board
}

/// Doc 01 section 7: a local document's path never leaves, only its file name.
///
/// The whole rule lives here, in one function every export path calls, because
/// a redaction applied in two places is a redaction that will one day be
/// applied in one.
fn redact_source(mut source: Value) -> Value {
    let Some(object) = source.as_object_mut() else {
        return source;
    };
    object.remove("profile_id");
    if object.get("class").and_then(Value::as_str) == Some("local_document") {
        let name = file_name_of(object.get("locator").and_then(Value::as_str).unwrap_or_default());
        object.insert("locator".into(), json!(name.clone()));
        // The dedupe key is a normalised locator, so it carries the path too.
        // Rewriting it to the file name keeps the merge rule working on the
        // recipient's side without carrying the folder it came from.
        object.insert("dedupe_key".into(), json!(format!("file:{name}")));
    }
    source
}

/// Doc 01 open question 2: a passage from a folder marked sensitive carries its
/// offsets and never its text, and the flag travels with it so the recipient can
/// see that something was withheld rather than that nothing was there.
fn redact_passage(mut passage: Value) -> Value {
    let Some(object) = passage.as_object_mut() else {
        return passage;
    };
    if object.get("text_withheld").and_then(Value::as_i64) == Some(1) {
        object.insert("text".into(), Value::Null);
    }
    // The embedding is a local index reference and means nothing elsewhere.
    object.insert("embedding_ref".into(), Value::Null);
    passage
}

/// The last path component, for either separator.
///
/// Both separators, because a bundle written on Windows is read on macOS and a
/// backslash is an ordinary character in a POSIX file name.
fn file_name_of(locator: &str) -> String {
    locator
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(locator)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_document_travels_as_a_file_name() {
        let source = json!({
            "id": "s1", "profile_id": "p1", "class": "local_document",
            "locator": "/home/someone/Private/Risk/model-risk-09.pdf",
            "dedupe_key": "file:/home/someone/Private/Risk/model-risk-09.pdf"
        });
        let out = redact_source(source);
        assert_eq!(out["locator"], "model-risk-09.pdf");
        assert_eq!(out["dedupe_key"], "file:model-risk-09.pdf");
        assert!(out.get("profile_id").is_none());
        // The whole point: the folder the file sat in does not appear anywhere.
        assert!(!out.to_string().contains("Private"));
    }

    #[test]
    fn a_web_source_keeps_its_locator() {
        let source = json!({
            "id": "s1", "profile_id": "p1", "class": "web",
            "locator": "https://example.test/rules", "dedupe_key": "example.test/rules"
        });
        let out = redact_source(source);
        assert_eq!(out["locator"], "https://example.test/rules");
        assert_eq!(out["dedupe_key"], "example.test/rules");
    }

    #[test]
    fn a_windows_path_loses_its_folders_too() {
        assert_eq!(file_name_of(r"C:\Users\someone\Risk\note.pdf"), "note.pdf");
        assert_eq!(file_name_of("plain.pdf"), "plain.pdf");
    }

    #[test]
    fn a_withheld_passage_carries_no_text() {
        let passage = json!({
            "id": "p1", "source_id": "s1", "text": "the secret",
            "text_withheld": 1, "location": "{\"page\":2}"
        });
        let out = redact_passage(passage);
        assert!(out["text"].is_null());
        // And the offsets stay, which is what doc 01 open question 2 asks for.
        assert_eq!(out["location"], "{\"page\":2}");
        assert_eq!(out["text_withheld"], 1);
    }

    #[test]
    fn the_checklist_defaults_to_sending_nothing() {
        // The one default where being wrong sends someone's own documents to a
        // stranger, so it is asserted rather than assumed.
        assert!(ExportOptions::default().local_documents.is_empty());
    }
}
