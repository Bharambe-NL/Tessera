//! Reading a board in. Doc 01 section 7.
//!
//! Import never overwrites. Rows keep their ids, which is safe because ids are
//! ULIDs, and the two places a merge cannot be avoided follow the rules doc 01
//! section 7 names by hand: sources merge by `dedupe_key`, concepts by `term`.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use serde_json::{Value, json};
use tessera_store::event::{EmitterType, NewEvent, Provenance};
use tessera_store::{Store, new_id, now_iso8601};

use crate::rows::insert;
use crate::{BundleError, FORMAT_VERSION, Result};

/// What an import did, in the terms doc 01 section 7 cares about.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportOutcome {
    pub bundle_id: String,
    pub board_id: String,
    pub board_title: String,
    /// Rows written, by file.
    pub written: BTreeMap<String, usize>,
    /// Rows already present, which import leaves exactly as they were.
    pub skipped: BTreeMap<String, usize>,
    /// Sources that merged into one this profile already had, by dedupe key.
    pub sources_merged: usize,
    /// Concepts whose term collided: both kept, the incoming one proposed.
    pub concepts_collided: usize,
    /// Blobs whose bytes did not hash to their name, so they were not written.
    pub blobs_rejected: Vec<String>,
}

/// Read a bundle into `store` as a forked board.
///
/// Doc 01 section 7: opening a bundle creates a Board carrying
/// `forked_from_bundle_id`, owned by the importing profile.
pub fn import<R: Read + Seek>(store: &mut Store, profile_id: &str, source: R) -> Result<ImportOutcome> {
    let mut zip = zip::ZipArchive::new(source)?;

    let manifest: Value = serde_json::from_str(&read_text(&mut zip, "manifest.json")?)?;
    if manifest["format_version"].as_str() != Some(FORMAT_VERSION) {
        return Err(BundleError::BadManifest(format!(
            "format_version is {}, this build reads {FORMAT_VERSION}",
            manifest["format_version"]
        )));
    }
    let bundle_id = manifest["bundle_id"]
        .as_str()
        .ok_or_else(|| BundleError::BadManifest("no bundle_id".into()))?
        .to_string();

    let mut board: Value = serde_json::from_str(&read_text(&mut zip, "board.json")?)?;
    let board_id = board["id"]
        .as_str()
        .ok_or_else(|| BundleError::BadManifest("board.json has no id".into()))?
        .to_string();

    let mut out = ImportOutcome {
        bundle_id: bundle_id.clone(),
        board_id: board_id.clone(),
        board_title: board["title"].as_str().unwrap_or_default().to_string(),
        ..Default::default()
    };

    // Read every file first, so a truncated archive is caught before a single
    // row is written. The manifest's counts are what makes that possible, and
    // checking them is the reason they are in the manifest at all.
    let mut loaded: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for name in [
        "cards.jsonl",
        "visuals.jsonl",
        "citations.jsonl",
        "flags.jsonl",
        "reviews.jsonl",
        "ink.jsonl",
        "notes.jsonl",
        "images.jsonl",
        "sources.jsonl",
        "passages.jsonl",
        "concepts.jsonl",
        "concept_links.jsonl",
        "exercises.jsonl",
        "events.jsonl",
    ] {
        let rows = read_jsonl(&mut zip, name)?;
        if let Some(expected) = manifest["counts"][name].as_u64()
            && rows.len() != expected as usize
        {
            return Err(BundleError::Truncated {
                file: name.to_string(),
                found: rows.len(),
                expected: expected as usize,
            });
        }
        loaded.insert(name.to_string(), rows);
    }

    // Blobs before rows, so an image row never points at bytes that failed
    // their hash. Doc 01 section 7: hashes are verified on import.
    for entry in manifest["blobs"].as_array().into_iter().flatten() {
        let Some(digest) = entry["sha256"].as_str() else {
            continue;
        };
        let bytes = read_bytes(&mut zip, &format!("blobs/{digest}"))?;
        if tessera_store::BlobStore::hash(&bytes) != digest {
            out.blobs_rejected.push(digest.to_string());
            continue;
        }
        store.blobs().put(&bytes)?;
    }

    // The doctrine pack the board names. A pack this profile does not have is
    // not invented: the board takes the profile's own pack and the fork is
    // recorded as having done so, because inventing a pack row would put rules
    // in the profile that nobody wrote.
    let pack_id = resolve_pack(store, &manifest, profile_id)?;

    let conn = store.conn();

    // ---- sources merge by dedupe_key, doc 01 section 7 ----
    let mut source_map: BTreeMap<String, String> = BTreeMap::new();
    for mut row in loaded.remove("sources.jsonl").unwrap_or_default() {
        let incoming = row["id"].as_str().unwrap_or_default().to_string();
        let key = row["dedupe_key"].as_str().unwrap_or_default().to_string();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM source WHERE profile_id = ?1 AND dedupe_key = ?2",
                rusqlite::params![profile_id, key],
                |r| r.get(0),
            )
            .ok();
        if let Some(local) = existing {
            // The same page retrieved twice is one Source. The incoming rows
            // that pointed at it are pointed at the local one instead.
            source_map.insert(incoming, local);
            out.sources_merged += 1;
            continue;
        }
        row["profile_id"] = json!(profile_id);
        source_map.insert(incoming.clone(), incoming.clone());
        count(&mut out, "sources.jsonl", insert(conn, "source", "id", &row)?);
    }

    // ---- passages follow their source ----
    for mut row in loaded.remove("passages.jsonl").unwrap_or_default() {
        let Some(source_id) = row["source_id"].as_str().map(str::to_string) else {
            continue;
        };
        let Some(mapped) = source_map.get(&source_id) else {
            // Its source was withheld at export, so there is nothing to hang it
            // from. Dropping it is right: a passage with no source cannot be
            // audited, which is the only reason passages travel.
            continue;
        };
        row["source_id"] = json!(mapped);
        count(&mut out, "passages.jsonl", insert(conn, "passage", "id", &row)?);
    }

    // ---- the board itself ----
    board["profile_id"] = json!(profile_id);
    board["doctrine_pack_id"] = json!(pack_id);
    board["forked_from_bundle_id"] = json!(bundle_id);
    // A fork of someone else's board is not a child of a board this profile
    // has, and a dangling parent id fails the foreign key.
    board["parent_board_id"] = Value::Null;
    count(&mut out, "board.json", insert(conn, "board", "id", &board)?);

    // ---- rows that hang off the board, parents before children ----
    for (file, table) in [
        ("cards.jsonl", "card"),
        ("visuals.jsonl", "visual"),
        ("ink.jsonl", "ink"),
        ("notes.jsonl", "note"),
        ("images.jsonl", "image"),
        ("exercises.jsonl", "exercise"),
        ("reviews.jsonl", "review"),
    ] {
        for row in loaded.remove(file).unwrap_or_default() {
            count(&mut out, file, insert(conn, table, "id", &row)?);
        }
    }

    // Citations after passages, and only those whose passage arrived. A
    // citation pointing at a withheld passage would fail its foreign key, and
    // the card keeps the rest of its markers.
    for row in loaded.remove("citations.jsonl").unwrap_or_default() {
        let known: i64 = conn.query_row(
            "SELECT COUNT(*) FROM passage WHERE id = ?1",
            [row["passage_id"].as_str().unwrap_or_default()],
            |r| r.get(0),
        )?;
        if known == 0 {
            continue;
        }
        count(&mut out, "citations.jsonl", insert(conn, "citation", "id", &row)?);
    }

    for row in loaded.remove("flags.jsonl").unwrap_or_default() {
        count(&mut out, "flags.jsonl", insert(conn, "flag", "id", &row)?);
    }

    // ---- concepts merge by term, doc 01 section 7 ----
    let mut extra_links: Vec<Value> = Vec::new();
    for mut row in loaded.remove("concepts.jsonl").unwrap_or_default() {
        let term = row["term"].as_str().unwrap_or_default().to_string();
        let incoming = row["id"].as_str().unwrap_or_default().to_string();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM concept WHERE profile_id = ?1 AND term = ?2",
                rusqlite::params![profile_id, term],
                |r| r.get(0),
            )
            .ok();
        row["profile_id"] = json!(profile_id);
        row["doctrine_pack_id"] = json!(pack_id);
        if let Some(local) = existing {
            // Doc 01 section 7: keep both, mark the incoming one proposed, and
            // link them `related_to` for the user to reconcile. Merging them
            // silently would assert that two people mean the same thing by one
            // word, which is the assumption a Concept exists to question.
            row["status"] = json!("proposed");
            out.concepts_collided += 1;
            extra_links.push(json!({
                "id": new_id(),
                "concept_id": incoming,
                "target_type": "concept",
                "target_ref": local,
                "relation": "related_to",
                "proposed_by": "import",
                "status": "proposed",
                "created_at": now_iso8601(),
            }));
        }
        count(&mut out, "concepts.jsonl", insert(conn, "concept", "id", &row)?);
    }

    for row in loaded
        .remove("concept_links.jsonl")
        .unwrap_or_default()
        .into_iter()
        .chain(extra_links)
    {
        count(&mut out, "concept_links.jsonl", insert(conn, "concept_link", "id", &row)?);
    }

    // ---- the sender's history ----
    //
    // Replayed rather than appended as this profile's own. The events did not
    // happen here, and doc 01's `source` enum has `replay` for exactly that:
    // the recipient sees how the board was built without the log claiming the
    // recipient built it. The original emitter survives, which is what doc 01
    // section 7 means by "the original author's runs remain readable and marked
    // as imported".
    let history = loaded.remove("events.jsonl").unwrap_or_default();
    let imported_events = history.len();
    for row in history {
        replay(store, &row)?;
    }
    out.written.insert("events.jsonl".into(), imported_events);

    store.append(
        NewEvent::new(
            "board.imported.v1",
            json!({
                "board_id": board_id,
                "bundle_id": bundle_id,
                "sources_merged": out.sources_merged,
                "concepts_collided": out.concepts_collided,
                "blobs_rejected": out.blobs_rejected.len(),
                "events_replayed": imported_events,
            }),
            Provenance::user(),
        )
        .on_board(&board_id),
    )?;

    Ok(out)
}

/// Append one of the sender's events, marked as a replay.
fn replay(store: &mut Store, row: &Value) -> Result<()> {
    let event_type = row["event_type"].as_str().unwrap_or_default();
    let mut provenance = Provenance::user();
    if let Some(emitter) = row["provenance"]["emitter_id"]
        .as_str()
        .or_else(|| row["emitter_id"].as_str())
    {
        provenance.emitter_id = emitter.to_string();
    }
    if let Some(kind) = row["provenance"]["emitter_type"]
        .as_str()
        .or_else(|| row["emitter_type"].as_str())
    {
        provenance.emitter_type = match kind {
            "agent" => EmitterType::Agent,
            "harness" => EmitterType::Harness,
            "retriever" => EmitterType::Retriever,
            // An emitter type this build does not know reads as the user rather
            // than failing the import, because the alternative is refusing a
            // whole board over one line of someone else's history.
            _ => EmitterType::User,
        };
    }
    provenance.source = tessera_store::Source::Replay;

    let mut event = NewEvent::new(event_type, row["payload"].clone(), provenance);
    if let Some(board_id) = row["board_id"].as_str() {
        event = event.on_board(board_id);
    }
    if let Some(card_id) = row["card_id"].as_str() {
        event = event.on_card(card_id);
    }
    store.append(event)?;
    Ok(())
}

/// The local pack row for the bundle's pack, or the profile's own.
fn resolve_pack(store: &Store, manifest: &Value, profile_id: &str) -> Result<String> {
    let code = manifest["doctrine_pack"]["code"].as_str().unwrap_or("general");
    let version = manifest["doctrine_pack"]["version"].as_str().unwrap_or("");

    let exact: Option<String> = store
        .conn()
        .query_row(
            "SELECT id FROM doctrine_pack WHERE code = ?1 AND version = ?2",
            rusqlite::params![code, version],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = exact {
        return Ok(id);
    }
    let by_code: Option<String> = store
        .conn()
        .query_row(
            "SELECT id FROM doctrine_pack WHERE code = ?1 ORDER BY version DESC LIMIT 1",
            [code],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = by_code {
        return Ok(id);
    }
    store
        .conn()
        .query_row(
            "SELECT default_doctrine_pack_id FROM profile WHERE id = ?1",
            [profile_id],
            |r| r.get(0),
        )
        .map_err(|_| BundleError::BadManifest(format!("no pack `{code}` and no profile pack")))
}

fn count(out: &mut ImportOutcome, file: &str, written: bool) {
    let bucket = if written {
        &mut out.written
    } else {
        &mut out.skipped
    };
    *bucket.entry(file.to_string()).or_insert(0) += 1;
}

fn read_text<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &'static str) -> Result<String> {
    let mut file = zip.by_name(name).map_err(|_| BundleError::Missing(name))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

fn read_bytes<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let mut file = zip
        .by_name(name)
        .map_err(|_| BundleError::Missing("a blob the manifest names"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// One json object per line, blank lines ignored.
///
/// A file the archive does not carry reads as empty rather than as an error:
/// doc 01 section 7's "export without history" writes a bundle with no
/// `events.jsonl`, and that is a valid bundle.
fn read_jsonl<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<Value>> {
    let text = match zip.by_name(name) {
        Ok(mut file) => {
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            text
        }
        Err(_) => return Ok(Vec::new()),
    };
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line)?);
    }
    Ok(rows)
}
