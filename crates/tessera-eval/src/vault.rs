//! The synthetic vault, loaded into a profile. Doc 16 section 5's eval line.
//!
//! `gen build` writes forty markdown files under `vault/` and a `vault.jsonl`
//! beside it saying what each one is. This reads both into page rows, resolves
//! the wikilinks, and indexes the bodies, so a sweep retrieves pages the way the
//! product does rather than through a fixture of its own.
//!
//! The corpus is the source of truth for the files; the rows are written from
//! `vault.jsonl` rather than by running the mirror over the folder, because the
//! ground truth carries what the mirror cannot know: which card a page was saved
//! from, and which passages it carried.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;
use tessera_store::Store;
use tessera_store::repo::{self, NewPage};

#[derive(Debug, Clone, Deserialize)]
pub struct Page {
    pub page_id: String,
    pub title: String,
    pub body: String,
    pub file_path: String,
    #[serde(default)]
    pub source_card_id: Option<String>,
    #[serde(default)]
    pub citations_carried: Vec<serde_json::Value>,
    #[serde(default)]
    pub fact_ids: Vec<String>,
    #[serde(default)]
    pub links_to: Vec<String>,
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Default, Clone)]
pub struct SeedReport {
    pub pages: usize,
    /// How many pages of each kind landed, so the line the sweep prints says
    /// what the vault is rather than only how big it is.
    pub saved_from_cards: usize,
    pub page_only: usize,
    pub links: usize,
    pub unresolved: usize,
    pub indexed: usize,
    /// Pages whose file on disk says something other than the row does. The
    /// corpus writes both, so a difference is a generator bug rather than
    /// something to paper over by trusting one of them.
    pub disagreements: Vec<String>,
}

pub fn load(corpus: &Path) -> Result<Vec<Page>, String> {
    let path = corpus.join("vault.jsonl");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| serde_json::from_str::<Page>(line).map_err(|e| format!("vault.jsonl: {e}")))
        .collect()
}

/// Write the pages, their links and their index entries.
pub fn seed(
    store: &mut Store,
    profile_id: &str,
    pack_id: &str,
    corpus: &Path,
    pages: &[Page],
    embedder: Option<&dyn tessera_retrievers::embed::Embedder>,
) -> Result<SeedReport, String> {
    let mut report = SeedReport::default();
    let titles: BTreeSet<String> = pages.iter().map(|p| p.title.to_lowercase()).collect();
    let mut ids: std::collections::BTreeMap<String, String> = Default::default();

    for page in pages {
        // The file the corpus wrote is what a person's vault holds, so it is
        // the body that is checked against the row.
        if let Ok(on_disk) = std::fs::read_to_string(corpus.join(&page.file_path))
            && on_disk != page.body
        {
            report.disagreements.push(page.file_path.clone());
        }

        let card_present = page.source_card_id.as_ref().filter(|card_id| {
            store
                .conn()
                .query_row(
                    "SELECT 1 FROM card WHERE id = ?1",
                    rusqlite::params![card_id],
                    |_| Ok(()),
                )
                .is_ok()
        });

        let id = repo::create_page(
            store,
            NewPage {
                profile_id,
                title: &page.title,
                body: &page.body,
                file_path: &page.file_path,
                // The card the page was saved from lives on a seeded board, and
                // the boards are seeded first. A page whose card is not there is
                // a page written by hand as far as the row is concerned.
                // The card this page was saved from, when the boards leg
                // seeded it. The eval seeds boards first and keeps the corpus's
                // card ids, so this is the same link doc 16 section 3.2 sets on
                // Save as page rather than a stand in for it.
                source_card_id: card_present.map(String::as_str),
                citations_carried: json!(page.citations_carried),
                doctrine_pack_id: Some(pack_id),
            },
        )
        .map_err(|e| format!("{}: {e}", page.title))?;
        ids.insert(page.title.to_lowercase(), id.clone());
        report.pages += 1;
        match page.kind.as_str() {
            "saved" => report.saved_from_cards += 1,
            "page_only" => report.page_only += 1,
            _ => {}
        }
        // A page-only page states a fact no document does, which is what makes
        // a page-only question worth asking. A page that claims to be one and
        // carries no fact would quietly measure nothing.
        if page.kind == "page_only" && page.fact_ids.is_empty() {
            report
                .disagreements
                .push(format!("{}: page_only with no fact", page.page_id));
        }
    }

    // Links after every page exists, so a link forward resolves as readily as a
    // link back. Doc 16 section 3.1: an unresolved link is kept, and the vault
    // plants a couple on purpose so that state is measured rather than assumed.
    for page in pages {
        let Some(from) = ids.get(&page.title.to_lowercase()) else {
            continue;
        };
        let mut links = Vec::new();
        for target in &page.links_to {
            let key = target.to_lowercase();
            let resolved = titles.contains(&key);
            if resolved {
                report.links += 1;
            } else {
                report.unresolved += 1;
            }
            links.push(repo::NewPageLink {
                target_kind: if resolved { "page" } else { "unresolved" }.to_string(),
                target_id: ids.get(&key).cloned(),
                target_title: target.clone(),
                display_text: target.clone(),
                position: page.body.find(&format!("[[{target}")).unwrap_or(0) as i64,
            });
        }
        repo::replace_page_links(store, from, &links).map_err(|e| format!("{}: {e}", page.title))?;

        report.indexed += tessera_retrievers::pages::index_page(
            store.conn(),
            profile_id,
            from,
            &page.title,
            &strip_links(&page.body),
            embedder,
        )
        .map_err(|e| format!("{}: {e}", page.title))?;
    }

    Ok(report)
}

/// One planted link, and whether the page it names can find it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkRow {
    pub from_title: String,
    pub target_title: String,
    /// `page` for a link the vault can resolve, `unresolved` for one the corpus
    /// planted to name nothing.
    pub target_kind: String,
    /// Whether the target's backlinks list the page the link is written on.
    pub in_backlinks: bool,
}

/// Seed a throwaway profile and check every link from the other end.
///
/// Doc 16 phase 12c gates backlink completeness at 1.00. Measured here rather
/// than inside the sweep's workers because the vault does not depend on the
/// questions: seeding it once in its own store and walking it is the whole
/// check, and it keeps the workers from racing to write one file.
///
/// The scorer re-derives the ratio from these rows and the corpus's own
/// `links_to`, so a run that silently dropped links cannot score 1.00 on the
/// few it kept.
pub fn audit(corpus: &Path, pages: &[Page]) -> Result<Vec<LinkRow>, String> {
    if pages.is_empty() {
        return Ok(Vec::new());
    }
    let mut store = Store::open_in_memory().map_err(|e| format!("vault audit: {e}"))?;
    let now = tessera_store::now_iso8601();
    let (profile_id, pack_id) = (tessera_store::new_id(), tessera_store::new_id());
    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            rusqlite::params![pack_id, now],
        )
        .map_err(|e| format!("vault audit: {e}"))?;
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            rusqlite::params![profile_id, pack_id, now],
        )
        .map_err(|e| format!("vault audit: {e}"))?;

    seed(&mut store, &profile_id, &pack_id, corpus, pages, None)?;

    let mut rows = Vec::new();
    for page in pages {
        for target in &page.links_to {
            let resolved =
                repo::page_by_title(&store, &profile_id, target).map_err(|e| format!("vault audit: {e}"))?;
            let (kind, found) = match resolved {
                Some(target_page) => {
                    let back = repo::backlinks(&store, "page", &target_page.id)
                        .map_err(|e| format!("vault audit: {e}"))?;
                    ("page", back.iter().any(|b| b.page_title == page.title))
                }
                None => ("unresolved", false),
            };
            rows.push(LinkRow {
                from_title: page.title.clone(),
                target_title: target.clone(),
                target_kind: kind.to_string(),
                in_backlinks: found,
            });
        }
    }
    Ok(rows)
}

/// `[[Title|alias]]` reads as "alias". The same rule the core applies before it
/// indexes, kept here rather than depending on the core so the eval's seeding
/// and the product's indexing can be compared rather than assumed identical.
fn strip_links(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        out.push_str(inner.split_once('|').map(|(_, alias)| alias).unwrap_or(inner));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_reads_as_what_it_shows() {
        assert_eq!(
            strip_links("See [[Liquidity risk|the rule]] and [[Buffer]]."),
            "See the rule and Buffer."
        );
        assert_eq!(strip_links("No links."), "No links.");
        assert_eq!(strip_links("Unclosed [[link"), "Unclosed [[link");
    }
}
