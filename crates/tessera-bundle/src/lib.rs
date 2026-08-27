//! The portable bundle. Doc 01 section 7.
//!
//! A board travels as a zip: a manifest, one jsonl file per entity kind, and the
//! blobs those rows point at. The recipient can audit every citation without
//! retrieving anything again, which is the whole point of carrying passages.
//!
//! Two rules shape everything here.
//!
//! **Nothing leaves that the author did not send.** Attempts, profile context,
//! model keys and folder paths are never included, and a local document's
//! locator is reduced to its file name on the way out. Doc 01 section 7 says so
//! in a sentence; [`redact_source`] is where it happens, once, so no export path
//! can miss it.
//!
//! **Import never overwrites.** Rows keep their ids, which is safe because ids
//! are ULIDs. Where a merge is unavoidable, doc 01 section 7 names the rule:
//! sources merge by `dedupe_key`, concepts by `term`, and a term collision keeps
//! both with the incoming one marked `proposed` and linked `related_to`.

mod backup;
mod diagnostics;
mod export;
mod import;
mod rows;

pub use backup::{BackupManifest, back_up, restore};
pub use diagnostics::{DiagnosticsSummary, RECENT_RUNS, diagnostics};
pub use export::{ExportOptions, LocalDocument, Preflight, export, preflight};
pub use import::{ImportOutcome, import};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("store: {0}")]
    Store(#[from] tessera_store::StoreError),
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no board `{0}` in this profile")]
    NoBoard(String),
    #[error("the archive has no {0}")]
    Missing(&'static str),
    #[error("the manifest is not a bundle manifest: {0}")]
    BadManifest(String),
    #[error("{file} carries {found} rows and the manifest promised {expected}")]
    Truncated {
        file: String,
        found: usize,
        expected: usize,
    },
    #[error("blob {digest} does not hash to its name")]
    BlobCorrupt { digest: String },
}

pub type Result<T> = std::result::Result<T, BundleError>;

/// The format this build writes and the only one it reads.
pub const FORMAT_VERSION: &str = "1.0";
