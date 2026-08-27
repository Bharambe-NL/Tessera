//! Re-verify cited sources against the corpus as it stands now.
//!
//! Doc 05 section 3: "web runs re-verification of cited locators weekly
//! (content hash comparison) and emits `source.stale.v1` on change", and "on
//! demand re-verification: when a board is opened and any citation is older
//! than its freshness class". Doc 05 section 7 names the three reasons a source
//! goes stale, and this produces all three.
//!
//! Everything here reads the corpus the way a retriever would: the file on
//! disk, its bytes, and the other files in its folder. Nothing reads the
//! generator's own manifests. A pass that compared `snapshots/T3.json` against
//! `snapshots/T1.json` would be copying the answer rather than finding it, and
//! staleness detection would measure the copy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tessera_store::Store;

/// What one pass found, so a run that could compare nothing says so rather than
/// reporting that nothing was stale.
#[derive(Debug, Default)]
pub struct StaleReport {
    pub checked: usize,
    pub stale: usize,
    pub by_reason: BTreeMap<String, usize>,
    /// Sources whose locator points outside the corpus, so neither tree holds a
    /// file to compare. Counted rather than assumed fresh.
    pub unresolvable: usize,
    /// True when no baseline tree was given, which leaves `content_changed`
    /// undetectable. The caller reports this rather than letting a silent zero
    /// read as "nothing changed".
    pub content_comparison_skipped: bool,
    /// The locators that went stale. A caller asking a fresh question that
    /// reaches one of these gets a card the freshness check will flag, which is
    /// how the Planner's stale ancestor rule becomes measurable.
    pub locators: Vec<String>,
}

/// Mark every cited source that the corpus no longer supports as it did.
///
/// `corpus` is the tree as it stands now. `baseline` is the tree as it stood
/// when the cards were written; without it a document that was quietly edited
/// cannot be told from one that was not, and the report says so.
pub fn mark(
    store: &mut Store,
    corpus: &Path,
    baseline: Option<&Path>,
    run_id: &str,
) -> Result<StaleReport, String> {
    let mut report = StaleReport {
        content_comparison_skipped: baseline.is_none(),
        ..Default::default()
    };

    let sources: Vec<(String, String)> = {
        let conn = store.conn();
        // A prior card is not a file to re-read. Doc 15 section 2 makes it
        // context and never evidence, so its freshness is the origin card's
        // problem. Excluded by class rather than by the shape of the locator,
        // because a regulatory locator is a bare file name with no folder in it.
        let mut stmt = conn
            .prepare(
                "SELECT id, locator FROM source
                  WHERE stale = 0 AND class != 'own_card' ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };

    for (source_id, locator) in sources {
        report.checked += 1;
        let Some(relative) = resolve(corpus, baseline, &locator) else {
            // The locator names nothing either tree holds, so neither its
            // absence nor its content says anything. Counted rather than read
            // as gone, because a locator this pass cannot resolve is a gap in
            // the pass and not a fact about the source.
            report.unresolvable += 1;
            continue;
        };
        let now = corpus.join("corpus").join(&relative);

        let reason = if !now.exists() {
            // Deleted, or a page that stopped resolving. Doc 05 section 7's
            // `locator_gone`.
            Some("locator_gone")
        } else if superseded_by_a_later_version(&now) {
            // The file is untouched and still says what it said. What changed is
            // that a later version of the same regulation now sits beside it,
            // which is doc 07 section B8.4's "version_ref equals the version in
            // force" read from the folder rather than from a filter.
            Some("superseded_version")
        } else {
            match baseline {
                Some(root) => {
                    let then = root.join("corpus").join(&relative);
                    // A file absent from the baseline was added after the cards
                    // were written, so nothing cited it then and it is not stale.
                    if then.exists() && digest(&then) != digest(&now) {
                        Some("content_changed")
                    } else {
                        None
                    }
                }
                None => None,
            }
        };

        if let Some(reason) = reason {
            tessera_store::repo::mark_source_stale(store, &source_id, reason, run_id)
                .map_err(|e| format!("{source_id}: {e}"))?;
            report.stale += 1;
            *report.by_reason.entry(reason.to_string()).or_default() += 1;
            report.locators.push(locator.clone());
        }
    }

    Ok(report)
}

/// Where a locator sits under `corpus/`, as a path relative to it.
///
/// A retriever records a locator relative to the folder it indexes, so
/// `reg-car3-v1.md` rather than `regulatory/reg-car3-v1.md`, and which folder
/// that was is not in the string. The folders are tried in turn against both
/// trees, so a document deleted by now still resolves through the baseline and
/// is read as gone rather than as a locator this pass could not place.
fn resolve(corpus: &Path, baseline: Option<&Path>, locator: &str) -> Option<PathBuf> {
    const FOLDERS: [&str; 3] = ["regulatory", "internal", "web"];
    let candidates =
        std::iter::once(PathBuf::from(locator)).chain(FOLDERS.iter().map(|f| Path::new(f).join(locator)));

    for candidate in candidates {
        let in_either = corpus.join("corpus").join(&candidate).exists()
            || baseline.is_some_and(|b| b.join("corpus").join(&candidate).exists());
        if in_either {
            return Some(candidate);
        }
    }
    None
}

/// Whether a later version of the same document sits beside this one.
///
/// The corpus names versions in the file stem, `reg-car3-v1` and `reg-car3-v2`,
/// and states them in the title line too. Reading the folder is what a
/// regulatory retriever does anyway, so this needs no index and no filter.
fn superseded_by_a_later_version(path: &Path) -> bool {
    let Some((stem, version)) = path.file_stem().and_then(|s| s.to_str()).and_then(split_version) else {
        return false;
    };
    let Some(folder) = path.parent() else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let sibling: PathBuf = entry.path();
        sibling
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(split_version)
            .is_some_and(|(other_stem, other_version)| other_stem == stem && other_version > version)
    })
}

/// Split `reg-car3-v1` into `reg-car3` and 1. A stem with no `-v<number>` tail
/// carries no version, so nothing supersedes it.
fn split_version(stem: &str) -> Option<(&str, u32)> {
    let (head, tail) = stem.rsplit_once("-v")?;
    tail.parse::<u32>().ok().map(|n| (head, n))
}

fn digest(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(hex::encode(Sha256::digest(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_tail_splits_off_the_stem() {
        assert_eq!(split_version("reg-car3-v1"), Some(("reg-car3", 1)));
        assert_eq!(split_version("reg-car3-v12"), Some(("reg-car3", 12)));
        assert_eq!(split_version("int-policy"), None);
        assert_eq!(split_version("int-rev-vNext"), None);
    }

    #[test]
    fn a_later_version_beside_a_file_supersedes_it() {
        let dir = std::env::temp_dir().join(format!("tessera-reverify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let v1 = dir.join("reg-car3-v1.md");
        let v2 = dir.join("reg-car3-v2.md");
        let solo = dir.join("reg-psd-s.md");
        std::fs::write(&v1, "one").expect("write");
        std::fs::write(&solo, "solo").expect("write");

        assert!(!superseded_by_a_later_version(&v1), "v2 does not exist yet");
        std::fs::write(&v2, "two").expect("write");
        assert!(superseded_by_a_later_version(&v1));
        assert!(
            !superseded_by_a_later_version(&v2),
            "nothing supersedes the latest"
        );
        assert!(
            !superseded_by_a_later_version(&solo),
            "no version, nothing to supersede"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
