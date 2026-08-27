//! The vault mirror. Doc 16 sections 3.1 and 7 point 2.
//!
//! "Mirrored as `vault/<slug>.md` in the profile folder; the file is the export
//! and the row is the index." Two copies of one page, either of which a person
//! can edit: the app writes the row, a text editor writes the file, and a sync
//! decides what to do about it.
//!
//! The reconciliation is a pure function over what the rows say and what the
//! directory holds, and everything that touches a disk or a database sits
//! outside it. That is the whole design: two way sync is a correctness swamp,
//! and the way out is that the decisions are a table you can write tests
//! against without a filesystem.
//!
//! Comparison is by content, never by mtime. A file copied out of a backup, a
//! folder synced by another tool, a clock that moved: all of them produce an
//! mtime that lies, and none of them changes what the text says.
//!
//! No watcher. `sync` is the unit, called at app start, after a page write and
//! from an RPC. A watcher is additive later and would buy nothing here except
//! timing that fails in CI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tessera_store::blob::BlobStore;
use tessera_store::repo::{self, PageRow};

/// The folder the vault lives in, under the profile.
pub const VAULT: &str = "vault";

/// What a conflict copy is called. Doc 16 section 7 point 2.
const CONFLICT: &str = " (conflict)";

/// One markdown file in the vault, as read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultFile {
    /// Relative to the profile folder, with `/` separators: `vault/a/b.md`.
    pub path: String,
    pub body: String,
}

/// What the mirror decided to do about one page or one file.
///
/// Every variant names both sides, so a caller applying them needs no second
/// look at the row or the file to know what it is writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The row moved and the file did not. Write the file.
    WriteFile {
        page_id: String,
        path: String,
        body: String,
        /// What the plan believed the file held, or `None` when it believed
        /// there was no file. The write re-reads before it overwrites and
        /// stands down when this is not what it finds, which is the one race a
        /// sync without a lock has.
        expect: Option<String>,
    },
    /// The file moved and the row did not. Take the file's text.
    AdoptBody {
        page_id: String,
        path: String,
        body: String,
    },
    /// Both moved, and to different text. Doc 16 section 7 point 2: last write
    /// wins with a conflict copy. The file wins the page, because a person who
    /// edited it outside the app meant to, and the row's text is kept beside it
    /// rather than dropped.
    Conflict {
        page_id: String,
        path: String,
        /// What the page becomes: the file's text.
        body: String,
        /// Where the row's text goes.
        conflict_path: String,
        conflict_title: String,
        conflict_body: String,
    },
    /// A file with no row. Someone wrote a page in their editor.
    Adopt {
        path: String,
        title: String,
        body: String,
    },
    /// The two agree. Recorded so the mirror can write down the agreement, which
    /// is what makes the next conflict decidable.
    Agreed { page_id: String, body: String },
    /// Nothing safe to do, and why. Doc 05 section 11's posture: a thing that
    /// could not be read is named on the page that can fix it rather than
    /// silently skipped.
    Skipped { path: String, reason: &'static str },
}

/// The reason a file was left alone.
pub const TITLE_TAKEN: &str = "a page already has this title";
pub const NO_TITLE: &str = "the file name gives no title";

/// Decide what the vault and the rows should do about each other.
///
/// Pure: no clock, no disk, no store. Rows and files in, actions out, in a
/// deterministic order (pages by id, then unmatched files by path) so two runs
/// over the same state produce the same list.
pub fn plan(rows: &[PageRow], files: &[VaultFile]) -> Vec<Action> {
    let by_path: BTreeMap<&str, &VaultFile> = files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut matched: BTreeSet<&str> = BTreeSet::new();
    let mut actions = Vec::new();

    let mut rows: Vec<&PageRow> = rows.iter().collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));

    // Titles already spoken for, so an adopted file cannot collide with a page
    // that exists. Compared case insensitively, because that is what the unique
    // index does.
    let mut taken: BTreeSet<String> = rows.iter().map(|r| r.title.to_lowercase()).collect();
    let mut paths: BTreeSet<String> = rows.iter().map(|r| r.file_path.clone()).collect();

    for row in rows {
        let Some(file) = by_path.get(row.file_path.as_str()) else {
            // The file is gone. Write it back rather than deleting the page:
            // an unmounted folder, a sync tool mid pass and a deliberate
            // deletion all look identical from here, and only one of them wants
            // the page gone. Deleting a page happens in the app, where the
            // person is asked.
            actions.push(Action::WriteFile {
                page_id: row.id.clone(),
                path: row.file_path.clone(),
                body: row.body.clone(),
                expect: None,
            });
            continue;
        };
        matched.insert(row.file_path.as_str());

        let body_hash = BlobStore::hash(row.body.as_bytes());
        let file_hash = BlobStore::hash(file.body.as_bytes());
        if body_hash == file_hash {
            actions.push(Action::Agreed {
                page_id: row.id.clone(),
                body: row.body.clone(),
            });
            continue;
        }

        let row_moved = body_hash != row.synced_hash;
        let file_moved = file_hash != row.synced_hash;
        match (row_moved, file_moved) {
            (true, false) => actions.push(Action::WriteFile {
                page_id: row.id.clone(),
                path: row.file_path.clone(),
                body: row.body.clone(),
                expect: Some(file.body.clone()),
            }),
            (false, true) => actions.push(Action::AdoptBody {
                page_id: row.id.clone(),
                path: row.file_path.clone(),
                body: file.body.clone(),
            }),
            // Both moved. Also the case where neither hash matches the
            // agreement because there never was one, which is the same problem:
            // two texts and no way to tell which is newer.
            _ => {
                let title = format!("{}{CONFLICT}", row.title);
                let path = conflict_path(&row.file_path, &paths);
                paths.insert(path.clone());
                taken.insert(title.to_lowercase());
                actions.push(Action::Conflict {
                    page_id: row.id.clone(),
                    path: row.file_path.clone(),
                    body: file.body.clone(),
                    conflict_path: path,
                    conflict_title: title,
                    conflict_body: row.body.clone(),
                });
            }
        }
    }

    // Files nobody claimed: pages written outside the app.
    for file in files {
        if matched.contains(file.path.as_str()) || paths.contains(&file.path) {
            continue;
        }
        let Some(title) = title_of(&file.path, &file.body) else {
            actions.push(Action::Skipped {
                path: file.path.clone(),
                reason: NO_TITLE,
            });
            continue;
        };
        if !taken.insert(title.to_lowercase()) {
            // Two files whose titles differ only in case, or a file named like
            // a page that lives somewhere else. Doc 16 section 3.1 allows one
            // of them the title, and guessing which would be a coin toss with
            // the person's notes.
            actions.push(Action::Skipped {
                path: file.path.clone(),
                reason: TITLE_TAKEN,
            });
            continue;
        }
        actions.push(Action::Adopt {
            path: file.path.clone(),
            title,
            body: file.body.clone(),
        });
    }

    actions
}

/// `vault/<slug>.md`, or the next free one.
///
/// Doc 17 section 5 writes learning records to `vault/learning/<mission>/<date>.md`,
/// so the folder is a parameter from the start rather than a change later.
pub fn file_path(folder: &str, title: &str, taken: &BTreeSet<String>) -> String {
    let base = slug(title);
    let folder = folder.trim_matches('/');
    let at = |name: &str| {
        if folder.is_empty() {
            format!("{VAULT}/{name}.md")
        } else {
            format!("{VAULT}/{folder}/{name}.md")
        }
    };

    let first = at(&base);
    if !taken.contains(&first) {
        return first;
    }
    // Two titles can slug to one name ("Liquidity risk" and "liquidity/risk"),
    // and the file system has one name to give. The page keeps its title; only
    // the file gets a number.
    for n in 2..1000 {
        let candidate = at(&format!("{base}-{n}"));
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    at(&format!("{base}-{}", tessera_store::new_id()))
}

/// A file name from a title: lower case, words joined by hyphens, nothing that
/// needs escaping on any of the three platforms.
pub fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut hyphen = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            hyphen = false;
        } else if ch.is_alphanumeric() {
            // Keep the letter rather than dropping it: a vault of Dutch notes
            // should not be a vault of empty names.
            out.extend(ch.to_lowercase());
            hyphen = false;
        } else if !hyphen && !out.is_empty() {
            out.push('-');
            hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "page".to_string() } else { out }
}

/// The title a file claims: its first markdown heading, else its file name.
fn title_of(path: &str, body: &str) -> Option<String> {
    for line in body.lines().take(5) {
        if let Some(heading) = line.trim().strip_prefix("# ") {
            let heading = heading.trim();
            if !heading.is_empty() {
                return Some(heading.to_string());
            }
        }
    }
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

fn conflict_path(path: &str, taken: &BTreeSet<String>) -> String {
    let p = Path::new(path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("page");
    let parent = p.parent().and_then(|s| s.to_str()).unwrap_or(VAULT);
    let mut candidate = format!("{parent}/{stem}{CONFLICT}.md");
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{parent}/{stem}{CONFLICT} {n}.md");
        n += 1;
    }
    candidate
}

// ------------------------------------------------------------------ disk ---

/// Read every markdown file under `<root>/vault`, recursively.
///
/// Sorted, so a plan over one vault is the same plan twice. A file that cannot
/// be read is left out rather than failing the sync: the rest of the vault is
/// still worth reconciling, and doc 05 section 11's posture is that an
/// unreadable file is named, not fatal.
pub fn read_vault(root: &Path) -> Vec<VaultFile> {
    let mut out = Vec::new();
    walk(root, &root.join(VAULT), &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<VaultFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(root, &path, out);
            continue;
        }
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(relative) = relative(root, &path) else {
            continue;
        };
        out.push(VaultFile { path: relative, body });
    }
}

/// A path under the profile folder, with `/` separators on every platform, so
/// the string in the row means the same thing on Windows and on Linux.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let mut out = String::new();
    for part in rest.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part.as_os_str().to_str()?);
    }
    Some(out)
}

// ------------------------------------------------------------ the editor --

/// Write a page, new or existing. Doc 16 phase 12c's editor.
///
/// One function for both, because a person editing a title has done the same
/// thing as a person typing one: the row keeps its id, the file follows the
/// slug, and the links are re-read from the body either way.
pub fn write_page(
    store: &mut tessera_store::Store,
    profile_id: &str,
    doctrine_pack_id: Option<&str>,
    page_id: Option<&str>,
    title: &str,
    body: &str,
) -> Result<String, SaveError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(SaveError::Refused(NO_TITLE_GIVEN));
    }

    // Doc 16 section 3.1's uniqueness rule, met before the write rather than as
    // a constraint error a person has to read.
    if let Some(other) = repo::page_by_title(store, profile_id, title).map_err(SaveError::Store)?
        && Some(other.id.as_str()) != page_id
    {
        return Err(SaveError::Refused(TITLE_TAKEN_BY_A_PAGE));
    }

    let root = store.root().to_path_buf();
    let id = match page_id {
        Some(id) => {
            let existing = repo::read_page(store, id)
                .map_err(SaveError::Store)?
                .ok_or(SaveError::Refused(NO_SUCH_PAGE))?;

            if existing.title != title {
                let taken: std::collections::BTreeSet<String> = repo::list_pages(store, profile_id, 10_000)
                    .map_err(SaveError::Store)?
                    .into_iter()
                    .filter(|p| p.id != existing.id)
                    .map(|p| p.file_path)
                    .collect();
                let path = file_path(folder_of(&existing.file_path), title, &taken);
                repo::rename_page(store, id, title, &path).map_err(SaveError::Store)?;
                // The old file goes before the new one arrives. A crash between
                // the two leaves the row with no file, which the mirror writes
                // back; the other order leaves a file with no row, which it
                // adopts as a second page.
                let _ = std::fs::remove_file(root.join(&existing.file_path));
            }
            repo::edit_page(store, id, body).map_err(SaveError::Store)?;
            id.to_string()
        }
        None => {
            let taken: std::collections::BTreeSet<String> = repo::list_pages(store, profile_id, 10_000)
                .map_err(SaveError::Store)?
                .into_iter()
                .map(|p| p.file_path)
                .collect();
            let path = file_path("", title, &taken);
            let id = repo::create_page(
                store,
                repo::NewPage {
                    profile_id,
                    title,
                    body,
                    file_path: &path,
                    source_card_id: None,
                    citations_carried: Value::Array(Vec::new()),
                    doctrine_pack_id,
                },
            )
            .map_err(SaveError::Store)?;
            // Doc 16 section 3.1: a link written before its page existed is
            // kept, and this is the moment it was kept for.
            repo::resolve_pending_links(store, "page", &id, title).map_err(SaveError::Store)?;
            id
        }
    };

    let page = repo::read_page(store, &id)
        .map_err(SaveError::Store)?
        .ok_or(SaveError::Refused(NO_SUCH_PAGE))?;
    save_links(store, profile_id, &id, body).map_err(SaveError::Store)?;
    write_file(&root, &page.file_path, body, None);
    repo::mark_page_synced(store, &id, body).map_err(SaveError::Store)?;
    index_page(store, profile_id, &page).map_err(SaveError::Store)?;
    Ok(id)
}

/// Remove a page, its file and its index entries.
pub fn delete_page(store: &mut tessera_store::Store, page_id: &str) -> Result<(), tessera_store::StoreError> {
    let root = store.root().to_path_buf();
    if let Some(page) = repo::read_page(store, page_id)? {
        let _ = std::fs::remove_file(root.join(&page.file_path));
    }
    // The index first: a page whose row is gone but whose chunks remain would
    // be retrieved and cited as a source nobody can open.
    tessera_retrievers::pages::forget_page(store.conn(), page_id)?;
    repo::delete_page(store, page_id)
}

/// The folder part of a page's path, so a rename keeps a page where it lives.
fn folder_of(file_path: &str) -> &str {
    let rest = file_path.strip_prefix(&format!("{VAULT}/")).unwrap_or(file_path);
    match rest.rfind('/') {
        Some(at) => &rest[..at],
        None => "",
    }
}

pub const NO_TITLE_GIVEN: &str = "a page needs a title";
pub const TITLE_TAKEN_BY_A_PAGE: &str = "another page has this title";
pub const NO_SUCH_PAGE: &str = "no such page";

// --------------------------------------------------------- save as a page --

/// Why a card could not be saved. Doc 16 section 3.2.
pub const NOT_SETTLED: &str = "a card is saved once it has an answer";
pub const BLOCKED: &str = "a blocked card is not saved";

/// Turn a card into a page. Doc 16 section 3.2.
///
/// "Creates a Page from question, answer, findings, and a markdown rendering of
/// the visual; `source_card_id` and `citations_carried` set."
///
/// The citations are copied rather than re-derived. Doc 16 section 2.2 is the
/// reason: the assessed package pointed the next answer's citation at the note,
/// so two hops later the regulation was out of reach and possibly stale. A page
/// is context and the passages it carries are the evidence, which only holds
/// while they are the same passages the card cited.
pub fn save_card_as_page(
    store: &mut tessera_store::Store,
    profile_id: &str,
    doctrine_pack_id: Option<&str>,
    card: &repo::CardView,
) -> Result<String, SaveError> {
    // Doc 16 section 3.2: "Only done or warn flagged cards can be saved."
    if card.status != "done" && card.status != "flagged" {
        return Err(SaveError::Refused(NOT_SETTLED));
    }
    if card.flags.iter().any(|f| f["severity"] == "block") {
        return Err(SaveError::Refused(BLOCKED));
    }

    let taken: std::collections::BTreeSet<String> = repo::list_pages(store, profile_id, 10_000)
        .map_err(SaveError::Store)?
        .into_iter()
        .map(|p| p.file_path)
        .collect();
    let title = title_for(store, profile_id, &card.question).map_err(SaveError::Store)?;
    let path = file_path("", &title, &taken);
    let body = page_body(&title, card);

    // Doc 16 section 3.2: blocked content is excluded. A block flag hides a
    // visual block rather than the answer, and a page that carried the hidden
    // tile would put back exactly what the Verifier took out.
    let carried: Vec<Value> = card
        .citations
        .iter()
        .filter_map(|c| {
            Some(serde_json::json!({
                "ordinal": c["ordinal"].as_i64()?,
                "passage_id": c["passage_id"].as_str().unwrap_or_default(),
            }))
        })
        .collect();

    let page_id = repo::create_page(
        store,
        repo::NewPage {
            profile_id,
            title: &title,
            body: &body,
            file_path: &path,
            source_card_id: Some(&card.id),
            citations_carried: Value::Array(carried),
            doctrine_pack_id,
        },
    )
    .map_err(SaveError::Store)?;

    repo::set_card_page(store, &card.id, &page_id).map_err(SaveError::Store)?;
    save_links(store, profile_id, &page_id, &body).map_err(SaveError::Store)?;
    // The link somebody wrote before this page existed. Doc 16 section 3.1.
    repo::resolve_pending_links(store, "page", &page_id, &title).map_err(SaveError::Store)?;

    let root = store.root().to_path_buf();
    write_file(&root, &path, &body, None);
    repo::mark_page_synced(store, &page_id, &body).map_err(SaveError::Store)?;

    if let Some(page) = repo::read_page(store, &page_id).map_err(SaveError::Store)? {
        index_page(store, profile_id, &page).map_err(SaveError::Store)?;
    }
    Ok(page_id)
}

/// What went wrong saving a card.
#[derive(Debug)]
pub enum SaveError {
    /// The card is not one a person may save, and why.
    Refused(&'static str),
    Store(tessera_store::StoreError),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Refused(why) => write!(f, "{why}"),
            SaveError::Store(e) => write!(f, "{e}"),
        }
    }
}

/// A title from the question, free for this profile to take.
fn title_for(
    store: &tessera_store::Store,
    profile_id: &str,
    question: &str,
) -> Result<String, tessera_store::StoreError> {
    let trimmed = question.trim().trim_end_matches(['?', '.', '!']).trim();
    let base = if trimmed.is_empty() { "A card" } else { trimmed };
    let base = base[..base.len().min(120)].trim().to_string();
    let base = base[..1].to_uppercase() + &base[1..];

    let mut title = base.clone();
    let mut n = 2;
    while repo::page_by_title(store, profile_id, &title)?.is_some() {
        title = format!("{base} {n}");
        n += 1;
    }
    Ok(title)
}

/// The markdown a saved card becomes. Doc 16 section 3.2.
fn page_body(title: &str, card: &repo::CardView) -> String {
    let mut out = format!("# {title}\n\n");
    if let Some(answer) = card.answer.as_deref() {
        out.push_str(answer.trim());
        out.push('\n');
    }

    let findings: Vec<&str> = card.findings.iter().filter_map(|f| f["text"].as_str()).collect();
    if !findings.is_empty() {
        out.push_str("\n## What it said\n\n");
        for finding in findings {
            out.push_str(&format!("- {finding}\n"));
        }
    }

    if let Some(visual) = card.visual.as_ref()
        && let Some(rendered) = visual_markdown(visual)
    {
        out.push_str("\n## The diagram, as text\n\n");
        out.push_str(&rendered);
    }

    if !card.citations.is_empty() {
        out.push_str("\n## Sources\n\n");
        for citation in &card.citations {
            let ordinal = citation["ordinal"].as_i64().unwrap_or(0);
            let title = citation["source_title"].as_str().unwrap_or("A source");
            let locator = citation["locator"].as_str().unwrap_or("");
            out.push_str(&format!("{ordinal}. {title} ({locator})\n"));
        }
    }
    out
}

/// The visual's labels as a list, blocks the Verifier hid left out.
///
/// A rendering rather than a copy of the payload: doc 16 section 3.2 wants the
/// page readable in any editor, and a JSON blob in a markdown file is neither
/// readable nor a diagram.
fn visual_markdown(visual: &Value) -> Option<String> {
    let blocks = visual["block_index"].as_array()?;
    let mut lines = Vec::new();
    for block in blocks {
        // Doc 07 section B8: a hidden block was hidden for a reason, and a page
        // that carried it would put back what the Verifier took out.
        if block["hidden"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(label) = block["label"].as_str() else {
            continue;
        };
        let ordinals: Vec<String> = block["citation_ordinals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o.as_i64().map(|n| format!("[{n}]")))
            .collect();
        lines.push(format!(
            "- {label}{}",
            if ordinals.is_empty() {
                String::new()
            } else {
                format!(" {}", ordinals.join(""))
            }
        ));
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n") + "\n")
}

// ------------------------------------------------------------------ links --

/// Index a page so the vault retriever can find it. Doc 16 section 3.3.
///
/// The wikilinks are stripped first: `[[Liquidity risk]]` indexed verbatim
/// matches a query for brackets and misses the sentence the link is part of.
pub fn index_page(
    store: &tessera_store::Store,
    profile_id: &str,
    page: &PageRow,
) -> Result<(), tessera_store::StoreError> {
    tessera_retrievers::pages::index_page(
        store.conn(),
        profile_id,
        &page.id,
        &page.title,
        &crate::wikilink::strip(&page.body),
        // No embedder in the product yet, so this is the lexical half. BN-107.
        None,
    )?;
    Ok(())
}

/// Re-index every page in the profile.
///
/// Cheap enough to do whole after a sync: a vault is hundreds of pages of
/// markdown, and the alternative is a set of dirty flags that can disagree with
/// the rows they track.
pub fn reindex(store: &tessera_store::Store, profile_id: &str) -> Result<usize, tessera_store::StoreError> {
    let pages = repo::list_pages(store, profile_id, 10_000)?;
    for page in &pages {
        index_page(store, profile_id, page)?;
    }
    Ok(pages.len())
}

/// Parse a page's body and store what its wikilinks point at.
///
/// Doc 16 sections 2.2 and 3.1. Resolution order is page, then concept, then
/// nothing: a page is a document a person can open and a concept is a term in
/// the glossary, so when both carry the title the page is what they meant to
/// follow. The concept detail lists pages beside cards either way.
///
/// Called wherever a body is written, which is why it takes the body rather
/// than reading it back: the caller has just decided what the body is.
pub fn save_links(
    store: &mut tessera_store::Store,
    profile_id: &str,
    page_id: &str,
    body: &str,
) -> Result<(), tessera_store::StoreError> {
    let parsed = crate::wikilink::parse(body);
    let mut links = Vec::with_capacity(parsed.len());
    for link in parsed {
        let (kind, target) = resolve(store, profile_id, &link.target_title)?;
        links.push(repo::NewPageLink {
            target_kind: kind.to_string(),
            target_id: target,
            target_title: link.target_title,
            display_text: link.display_text,
            position: link.position as i64,
        });
    }
    repo::replace_page_links(store, page_id, &links)
}

fn resolve(
    store: &tessera_store::Store,
    profile_id: &str,
    title: &str,
) -> Result<(&'static str, Option<String>), tessera_store::StoreError> {
    if let Some(page) = repo::page_by_title(store, profile_id, title)? {
        return Ok(("page", Some(page.id)));
    }
    if let Some(concept) = repo::concept_by_term_or_alias(store, profile_id, title)? {
        return Ok(("concept", Some(concept)));
    }
    Ok(("unresolved", None))
}

/// What one pass over the vault did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub written: usize,
    pub adopted: usize,
    pub created: usize,
    pub conflicts: usize,
    pub agreed: usize,
    /// Path and reason, in the shape the Pages view shows them.
    pub skipped: Vec<(String, &'static str)>,
}

/// Reconcile the vault folder with the page rows.
///
/// The only impure part: it reads the directory, runs [`plan`], and applies
/// what it decided. Applying re-reads each file it is about to overwrite and
/// leaves it alone if it moved since the listing, because a person typing in
/// their editor while this runs must not lose the sentence they were on. The
/// next sync sees the new text and decides again.
pub fn sync(
    store: &mut tessera_store::Store,
    profile_id: &str,
    doctrine_pack_id: Option<&str>,
) -> Result<SyncReport, tessera_store::StoreError> {
    let root = store.root().to_path_buf();
    let rows = repo::list_pages(store, profile_id, 10_000)?;
    let files = read_vault(&root);
    let mut report = SyncReport::default();

    let mut touched = false;
    for action in plan(&rows, &files) {
        match action {
            Action::Agreed { page_id, body } => {
                repo::mark_page_synced(store, &page_id, &body)?;
                report.agreed += 1;
            }
            Action::WriteFile {
                page_id,
                path,
                body,
                expect,
            } => {
                if write_file(&root, &path, &body, expect.as_deref()) {
                    repo::mark_page_synced(store, &page_id, &body)?;
                    report.written += 1;
                }
            }
            Action::AdoptBody { page_id, body, .. } => {
                repo::adopt_page_body(store, &page_id, &body)?;
                save_links(store, profile_id, &page_id, &body)?;
                report.adopted += 1;
            }
            Action::Conflict {
                page_id,
                body,
                conflict_path,
                conflict_title,
                conflict_body,
                ..
            } => {
                // The copy first. If this is where it stops, the page still
                // holds the row's text and the file still holds its own, which
                // is where it started; losing the copy after taking the file's
                // text is the order that loses work.
                if !write_file(&root, &conflict_path, &conflict_body, None) {
                    continue;
                }
                let copy = repo::create_page(
                    store,
                    repo::NewPage {
                        profile_id,
                        title: &conflict_title,
                        body: &conflict_body,
                        file_path: &conflict_path,
                        source_card_id: None,
                        citations_carried: serde_json::json!([]),
                        doctrine_pack_id,
                    },
                )?;
                save_links(store, profile_id, &copy, &conflict_body)?;
                repo::adopt_page_body(store, &page_id, &body)?;
                save_links(store, profile_id, &page_id, &body)?;
                report.conflicts += 1;
            }
            Action::Adopt { path, title, body } => {
                let page_id = repo::create_page(
                    store,
                    repo::NewPage {
                        profile_id,
                        title: &title,
                        body: &body,
                        file_path: &path,
                        source_card_id: None,
                        citations_carried: serde_json::json!([]),
                        doctrine_pack_id,
                    },
                )?;
                save_links(store, profile_id, &page_id, &body)?;
                // A page that arrives is a title somebody may already have
                // linked to. Doc 16 section 3.1 keeps an unresolved link rather
                // than dropping it, and this is the moment it was kept for.
                repo::resolve_pending_links(store, "page", &page_id, &title)?;
                report.created += 1;
            }
            Action::Skipped { path, reason } => report.skipped.push((path, reason)),
        }
        touched = true;
    }

    // One pass at the end rather than one per page: a sync that adopted forty
    // files would otherwise re-read and re-chunk the same vault forty times.
    if touched {
        reindex(store, profile_id)?;
    }

    Ok(report)
}

/// Write a file, unless it changed since the listing was taken.
///
/// `expect` is the text the plan believed was there. `None` means the plan
/// believed there was no file at all, which a fresh write and a rewritten
/// missing file both are.
fn write_file(root: &Path, path: &str, body: &str, expect: Option<&str>) -> bool {
    let full = root.join(path);
    if let Ok(current) = std::fs::read_to_string(&full) {
        match expect {
            Some(text) if current != text => return false,
            // The plan said this file was missing and it is not. Something
            // wrote it between the listing and now, and the next sync is where
            // that gets decided rather than here.
            None if current != body => return false,
            _ => {}
        }
    }
    if let Some(parent) = full.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&full, body).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_stands_down_when_the_file_moved_under_it() {
        // The one race a sync without a lock has: the listing was taken, the
        // plan was made, and between the two a person typed in their editor.
        // Losing the sentence they were on is worse than deciding again next
        // time, so the write re-reads first.
        let root = std::env::temp_dir().join(format!("tessera-vault-{}", tessera_store::new_id()));
        std::fs::create_dir_all(root.join(VAULT)).expect("dir");
        let path = "vault/a.md";

        // The plan believed there was no file. There is, and it says something
        // else, so this is not ours to overwrite.
        std::fs::write(root.join(path), "what the person just typed").expect("file");
        assert!(!write_file(&root, path, "what the row says", None));
        assert_eq!(
            std::fs::read_to_string(root.join(path)).expect("read"),
            "what the person just typed"
        );

        // The plan believed the file said one thing; it does, so the write is
        // the one that was decided.
        assert!(write_file(
            &root,
            path,
            "the new text",
            Some("what the person just typed")
        ));
        assert_eq!(
            std::fs::read_to_string(root.join(path)).expect("read"),
            "the new text"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_file_is_written_from_the_row() {
        let root = std::env::temp_dir().join(format!("tessera-vault-{}", tessera_store::new_id()));
        assert!(write_file(
            &root,
            "vault/learning/a-mission/2026-08-27.md",
            "text",
            None
        ));
        assert_eq!(
            std::fs::read_to_string(root.join("vault/learning/a-mission/2026-08-27.md")).expect("read"),
            "text",
            "a subpath is created rather than refused"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
