//! Walking a folder into the index. Doc 05 section 8.2's index maintenance.
//!
//! One definition, used by the product's watcher, by the eval runner, and by
//! the recall probe. Three copies of "walk, parse, chunk, write" would drift,
//! and the one that drifted would be whichever is not the one being measured.
//!
//! Parse errors are recorded rather than raised. Doc 05 section 10 makes a
//! parse error a per file outcome, and doc 05 section 11 puts those errors on
//! the Profile's Retrievers page, so the caller gets a list rather than a
//! stack trace.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::embed::Embedder;
use crate::parse::ParseError;
use crate::{index, is_supported, parse_file};

/// What one pass over a folder did.
#[derive(Debug, Default, Clone)]
pub struct IngestReport {
    pub indexed: usize,
    pub chunks: usize,
    /// Path, kind, and detail, in the shape the Profile shows them.
    pub errors: Vec<(String, &'static str, String)>,
    /// Paths a hook refused. Doc 05 section 8.2: an excluded folder is never
    /// opened, so these were never read rather than read and discarded.
    pub excluded: usize,
}

/// Index every readable file under `root` into `folder_id`.
///
/// `exclude` are path fragments that must never be opened. The check is on the
/// path rather than on the content because the point is not to read the file at
/// all: doc 01 open question 2 lets a sensitive folder be indexed with text
/// withheld, and that is a different mode from this one, which simply skips.
pub fn index_folder(
    conn: &Connection,
    profile_id: &str,
    folder_id: &str,
    label: &str,
    root: &Path,
    exclude: &[String],
    embedder: Option<&dyn Embedder>,
) -> rusqlite::Result<IngestReport> {
    let now = tessera_store::now_iso8601();
    conn.execute(
        "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (id) DO UPDATE SET root = ?3, label = ?4",
        params![folder_id, profile_id, root.display().to_string(), label, now],
    )?;

    let mut report = IngestReport::default();
    let mut files = Vec::new();
    walk(root, &mut files);
    // Sorted so two runs over one folder index in one order, which keeps the
    // corpus reproducible and two eval runs comparable.
    files.sort();

    for path in files {
        if excluded(&path, exclude) {
            report.excluded += 1;
            continue;
        }

        let reference = relative(root, &path);
        match parse_file(&path) {
            Ok(chunks) => {
                report.chunks += index::write_document(conn, folder_id, &reference, &chunks, embedder, &now)?;
                report.indexed += 1;
            }
            Err(e) => {
                let kind = match &e {
                    ParseError::Io(_) => "io",
                    ParseError::Malformed { .. } => "malformed",
                    ParseError::Protected(_) => "protected",
                    ParseError::NeedsOcr(_) => "needs_ocr",
                    ParseError::UnsupportedFormat(_) => "unsupported",
                };
                record_error(conn, folder_id, &reference, kind, &e.to_string(), &now)?;
                report.errors.push((reference, kind, e.to_string()));
            }
        }
    }

    conn.execute(
        "UPDATE watched_folder SET last_indexed_at = ?1 WHERE id = ?2",
        params![now, folder_id],
    )?;

    Ok(report)
}

/// Whether any exclusion names a component of this path.
///
/// Component-wise rather than substring, so a folder called `Sensitive` is
/// excluded and a file called `insensitive-notes.md` is not.
fn excluded(path: &Path, exclude: &[String]) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        exclude.iter().any(|e| name.eq_ignore_ascii_case(e))
    })
}

fn record_error(
    conn: &Connection,
    folder_id: &str,
    path: &str,
    kind: &str,
    detail: &str,
    now: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO index_error (folder_id, path, kind, detail, noticed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (folder_id, path) DO UPDATE SET kind = ?3, detail = ?4, noticed_at = ?5",
        params![folder_id, path, kind, detail, now],
    )?;
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if is_supported(&path) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> tessera_store::Store {
        let s = tessera_store::Store::open_in_memory().expect("store");
        s.conn()
            .execute(
                "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                     retriever_config, created_at, updated_at)
                 VALUES ('p', 'deep', 'pack', '{}', '{}', 'now', 'now')",
                [],
            )
            .expect("profile");
        s
    }

    fn tree() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-ingest-{}", tessera_store::new_id()));
        std::fs::create_dir_all(dir.join("Sensitive")).expect("dir");
        std::fs::create_dir_all(dir.join("Open")).expect("dir");
        std::fs::write(
            dir.join("Open/policy.md"),
            "# Policy\n\nThe buffer is 2.5 percent.\n",
        )
        .expect("write");
        std::fs::write(
            dir.join("Open/insensitive-notes.md"),
            "# Notes\n\nA note about buffers.\n",
        )
        .expect("write");
        std::fs::write(dir.join("Sensitive/minutes.md"), "# Minutes\n\nConfidential.\n").expect("write");
        std::fs::write(dir.join("Open/broken.pdf"), b"%PDF-1.4\nnot really").expect("write");
        std::fs::write(dir.join("Open/photo.jpeg"), b"\xff\xd8\xff").expect("write");
        dir
    }

    #[test]
    fn a_folder_is_walked_parsed_and_indexed() {
        let s = store();
        let root = tree();
        let report = index_folder(s.conn(), "p", "local", "Risk", &root, &[], None).expect("ingest");

        assert_eq!(report.indexed, 3, "{report:?}");
        assert!(report.chunks >= 3);
        // The jpeg is not a failure: it is a format this build does not read,
        // and the walker never offered it.
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].1, "malformed");

        let hits = index::search(s.conn(), &["local".into()], "buffer percent", None, 10).expect("search");
        assert!(!hits.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_excluded_folder_is_never_opened() {
        // Doc 05 section 12's exclusion compliance. The file is not read, not
        // read and dropped: the difference is the whole point of the rule.
        let s = store();
        let root = tree();
        let report =
            index_folder(s.conn(), "p", "local", "Risk", &root, &["Sensitive".into()], None).expect("ingest");

        assert_eq!(report.excluded, 1);
        let hits = index::search(s.conn(), &["local".into()], "Confidential", None, 10).expect("search");
        assert!(hits.is_empty(), "an excluded file reached the index");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exclusion_matches_a_path_component_and_not_a_substring() {
        // `insensitive-notes.md` contains "Sensitive" as a substring. A
        // substring check would silently drop a file nobody excluded.
        let s = store();
        let root = tree();
        index_folder(s.conn(), "p", "local", "Risk", &root, &["Sensitive".into()], None).expect("ingest");

        let hits =
            index::search(s.conn(), &["local".into()], "note about buffers", None, 10).expect("search");
        assert!(!hits.is_empty(), "a file was excluded by a substring match");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_parse_error_is_recorded_where_the_profile_can_show_it() {
        // Doc 05 section 11: the Retrievers page lists parse errors. A failure
        // nobody can see is a document that silently vanished.
        let s = store();
        let root = tree();
        index_folder(s.conn(), "p", "local", "Risk", &root, &[], None).expect("ingest");

        let (path, kind): (String, String) = s
            .conn()
            .query_row("SELECT path, kind FROM index_error LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("error row");
        assert!(path.ends_with("broken.pdf"), "{path}");
        assert_eq!(kind, "malformed");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn indexing_twice_replaces_rather_than_duplicates() {
        let s = store();
        let root = tree();
        let first = index_folder(s.conn(), "p", "local", "Risk", &root, &[], None).expect("one");
        let second = index_folder(s.conn(), "p", "local", "Risk", &root, &[], None).expect("two");
        assert_eq!(first.chunks, second.chunks);

        let total: i64 = s
            .conn()
            .query_row("SELECT count(*) FROM index_entry", [], |r| r.get(0))
            .expect("count");
        assert_eq!(total as usize, first.chunks, "a second pass duplicated the index");
        std::fs::remove_dir_all(&root).ok();
    }
}
