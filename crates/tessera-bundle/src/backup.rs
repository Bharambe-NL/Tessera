//! Backing up and restoring a profile. Doc 10 section 15.
//!
//! "The profile folder is the unit. Back up, restore, and open profile from
//! folder are the three operations. A corrupted SQLite file is detected on
//! start; the app offers restore from the last backup and keeps the damaged
//! file aside."
//!
//! The database is not copied as a file. SQLite in WAL mode keeps recent
//! commits in a side file, so a byte copy taken while anything is writing is a
//! copy of a database mid transaction: it opens, it passes a shallow look, and
//! it is missing whatever had not been checkpointed. `VACUUM INTO` asks SQLite
//! for a consistent snapshot instead, which is the one operation that means
//! what a person thinks copying the file means.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use tessera_store::Store;
use zip::write::SimpleFileOptions;

use crate::{BundleError, Result};

/// What a backup carries, so a restore can say what it is about to write.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupManifest {
    pub format_version: String,
    pub taken_at: String,
    /// Rows per table at the moment of the snapshot, so a restore that lands
    /// short says so rather than looking complete.
    pub counts: serde_json::Map<String, serde_json::Value>,
    pub blobs: usize,
}

/// Write the profile at `store` to `sink` as a backup zip.
pub fn back_up<W: Write + Seek>(store: &Store, sink: W) -> Result<BackupManifest> {
    let snapshot = store
        .root()
        .join(format!("backup-{}.sqlite", tessera_store::new_id()));
    // Any stale snapshot from a crashed run would be vacuumed into and fail.
    let _ = std::fs::remove_file(&snapshot);

    // The one operation that takes a consistent copy while the database is in
    // use. A byte copy of a WAL database is a copy of a transaction in progress.
    store
        .conn()
        .execute("VACUUM INTO ?1", [snapshot.to_string_lossy().to_string()])?;

    let counts = table_counts(store)?;
    let mut blob_paths = Vec::new();
    let blob_root = store.blobs().root().to_path_buf();
    if blob_root.is_dir() {
        collect(&blob_root, &mut blob_paths)?;
    }

    let manifest = BackupManifest {
        format_version: crate::FORMAT_VERSION.to_string(),
        taken_at: tessera_store::now_iso8601(),
        counts,
        blobs: blob_paths.len(),
    };

    let outcome = write_zip(sink, &snapshot, &blob_root, &blob_paths, &manifest);
    // The snapshot is a second copy of everything the profile holds, so it goes
    // whether the zip succeeded or not.
    let _ = std::fs::remove_file(&snapshot);
    outcome?;

    Ok(manifest)
}

fn write_zip<W: Write + Seek>(
    sink: W,
    snapshot: &Path,
    blob_root: &Path,
    blobs: &[PathBuf],
    manifest: &BackupManifest,
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(sink);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("manifest.json", options)?;
    zip.write_all(serde_json::to_string_pretty(manifest)?.as_bytes())?;

    zip.start_file("tessera.sqlite", options)?;
    zip.write_all(&std::fs::read(snapshot)?)?;

    for path in blobs {
        let Ok(relative) = path.strip_prefix(blob_root) else {
            continue;
        };
        zip.start_file(
            format!("blobs/{}", relative.to_string_lossy().replace('\\', "/")),
            options,
        )?;
        zip.write_all(&std::fs::read(path)?)?;
    }

    zip.finish()?;
    Ok(())
}

/// Read a backup into `root`, which must be empty or absent.
///
/// Never into a folder that already holds a profile. Doc 10 section 15 offers a
/// restore to someone whose database is damaged, and the worst possible reading
/// of that offer is one that overwrites the damaged file before anyone has
/// looked at it. [`quarantine`] is what moves the damaged one aside first.
pub fn restore<R: Read + Seek>(source: R, root: &Path) -> Result<BackupManifest> {
    if root.join("tessera.sqlite").exists() {
        return Err(BundleError::BadManifest(format!(
            "{} already holds a profile; move it aside first",
            root.display()
        )));
    }
    std::fs::create_dir_all(root)?;

    let mut zip = zip::ZipArchive::new(source)?;
    let manifest: BackupManifest = {
        let mut file = zip
            .by_name("manifest.json")
            .map_err(|_| BundleError::Missing("manifest.json"))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        serde_json::from_str(&text)?
    };

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` refuses `..` and absolute paths, which is what stops
        // an archive from writing outside the folder it was pointed at.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        if name.to_string_lossy() == "manifest.json" {
            continue;
        }
        let target = root.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        std::fs::write(&target, bytes)?;
    }

    Ok(manifest)
}

fn table_counts(store: &Store) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut out = serde_json::Map::new();
    for table in [
        "board",
        "card",
        "visual",
        "citation",
        "source",
        "passage",
        "concept",
        "flag",
        "event",
        "run",
        "step",
        "exercise",
        "image",
        "ink",
        "note",
        "learn_session",
    ] {
        let n: i64 = store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0);
        out.insert(table.to_string(), serde_json::json!(n));
    }
    Ok(out)
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, into)?;
        } else {
            into.push(path);
        }
    }
    Ok(())
}
