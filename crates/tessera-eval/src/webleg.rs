//! The web retrieval leg. Doc 05 section 12 and phase 13e.
//!
//! "Recall at k on planted facts by retriever: web 0.80 (the synthetic web is
//! served locally, so this measures extraction and ranking)."
//!
//! Which is the whole point of the number. There is no search API in this
//! build, so what is being measured is not whether the product finds the right
//! site: it is whether, given the sites, the fetch reaches the page, the
//! extraction keeps the sentence, and the ranking puts it above the pages that
//! do not say it. Those three are what decide whether a citation is any good,
//! and they are measurable for nothing against a corpus served on loopback.
//!
//! The corpus plants each fact in named documents at a named fidelity. A web
//! plant is `partial` more often than not, which is the generator being honest
//! about what a plain-language site does to a regulation, and it is why the
//! gate sits at 0.80 rather than at the regulatory retriever's 0.95.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tessera_retrievers::contract::Packet;
use tessera_retrievers::web::{self, HttpFetcher, WebConfig};

/// One planted fact, as the corpus records it.
#[derive(Debug, Clone, Deserialize)]
struct Fact {
    fact_id: String,
    #[serde(default)]
    statement: String,
    #[serde(default)]
    planted_in: Vec<Plant>,
}

#[derive(Debug, Clone, Deserialize)]
struct Plant {
    doc_id: String,
    #[serde(default)]
    fidelity: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Document {
    doc_id: String,
    kind: String,
    #[serde(default)]
    path: String,
}

/// What one query recorded. Written as `web_retrieval.jsonl`.
#[derive(Debug, Clone, Serialize)]
pub struct WebRecord {
    pub fact_id: String,
    pub query: String,
    /// The web documents the corpus planted this fact in.
    pub expected_docs: Vec<String>,
    /// The fidelity of the best web plant, so a run can be split by it before
    /// anything is built to fix a number that surprises.
    pub fidelity: String,
    /// The documents the retriever came back with, best first.
    pub returned_docs: Vec<String>,
    pub passages: usize,
    pub fetch_errors: usize,
}

impl WebRecord {
    /// Doc 05 section 12's recall at k: did the page carrying the fact come
    /// back at all, anywhere in what was returned.
    pub fn recalled(&self) -> bool {
        self.returned_docs.iter().any(|d| self.expected_docs.contains(d))
    }
}

fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<Vec<T>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("{}: {e}", path.display())))
        .collect()
}

/// The sites the corpus serves, as the seeds a profile would name.
///
/// One per domain directory, because that is what a person adds: a site they
/// trust, not a search engine. Discovery walks each site's listing, which is
/// exactly the shape `gen serve` puts on disk.
fn seeds(base: &str, documents: &[Document]) -> Vec<String> {
    let base = base.trim_end_matches('/');
    let mut hosts: BTreeSet<&str> = BTreeSet::new();
    for document in documents.iter().filter(|d| d.kind == "web") {
        if let Some(host) = document.path.split('/').nth(1) {
            hosts.insert(host);
        }
    }
    hosts.iter().map(|h| format!("{base}/{h}/")).collect()
}

/// The query a fact becomes.
///
/// The statement as written, which is what the corpus planted and what a
/// learner asking about the topic would say in their own words. Deliberately
/// not the page's own title: a query built from the answer's headline measures
/// string matching rather than retrieval.
fn query_for(fact: &Fact) -> String {
    fact.statement.trim().to_string()
}

/// Run the leg. Doc 05 section 12.
pub fn run(corpus: &Path, base: &str, limit: usize) -> Result<Vec<WebRecord>, String> {
    let documents: Vec<Document> = read(&corpus.join("corpus").join("documents.jsonl"))?;
    let facts: Vec<Fact> = read(&corpus.join("facts.jsonl"))?;

    let web_docs: BTreeMap<&str, &Document> = documents
        .iter()
        .filter(|d| d.kind == "web")
        .map(|d| (d.doc_id.as_str(), d))
        .collect();
    if web_docs.is_empty() {
        return Err("the corpus has no web documents; rebuild it with `gen build`".into());
    }

    let seeds = seeds(base, &documents);
    let config = WebConfig {
        seeds: seeds.clone(),
        // Every page the corpus serves, and then some. Doc 05 section 12 says
        // this number "measures extraction and ranking", so the crawl must not
        // be part of it: a budget below the page count measures how far the
        // crawl got instead, which is a different question with its own answer.
        max_fetch: web_docs.len().max(1) * 2,
    };
    let fetcher = HttpFetcher::new();

    let mut out = Vec::new();
    for fact in &facts {
        if out.len() >= limit {
            break;
        }
        let planted: Vec<&Plant> = fact
            .planted_in
            .iter()
            .filter(|p| web_docs.contains_key(p.doc_id.as_str()))
            .collect();
        if planted.is_empty() || fact.statement.trim().is_empty() {
            continue;
        }

        let query = query_for(fact);
        let packet: Packet = serde_json::from_value(json!({
            "run_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "retriever_id": "web",
            "query": query,
            // Doc 05 section 12's "recall at k". Ten is what a deep card's
            // Synthesizer would see, so it is the k the number is about.
            "max_passages": 10,
            "doctrine": { "trust_ranks": [{ "class": "web", "rank": 6 }] }
        }))
        .map_err(|e| format!("packet: {e}"))?;

        let retrieved = web::retrieve(&fetcher, &config, &packet);

        // A locator back to the doc id the corpus knows it by, through the file
        // name, which is what `gen serve` serves and what `path` records.
        let by_file: BTreeMap<&str, &str> = web_docs
            .values()
            .filter_map(|d| d.path.rsplit('/').next().map(|f| (f, d.doc_id.as_str())))
            .collect();
        let mut returned: Vec<String> = Vec::new();
        for passage in &retrieved.passages {
            let file = passage.source.locator.rsplit('/').next().unwrap_or_default();
            if let Some(doc_id) = by_file.get(file)
                && !returned.contains(&doc_id.to_string())
            {
                returned.push((*doc_id).to_string());
            }
        }

        out.push(WebRecord {
            fact_id: fact.fact_id.clone(),
            query,
            expected_docs: planted.iter().map(|p| p.doc_id.clone()).collect(),
            // The best fidelity any web plant of this fact has, because that is
            // the most a retriever could have found.
            fidelity: best_fidelity(&planted),
            returned_docs: returned,
            passages: retrieved.passages.len(),
            fetch_errors: retrieved.fetch_errors,
        });
    }

    if out.is_empty() {
        return Err("no fact is planted in a web document".into());
    }
    Ok(out)
}

/// Exact beats paraphrase beats partial. A fact a site only alludes to is a
/// different retrieval problem from one it states, and the report says which.
fn best_fidelity(planted: &[&Plant]) -> String {
    for wanted in ["exact", "paraphrase", "partial"] {
        if planted.iter().any(|p| p.fidelity == wanted) {
            return wanted.to_string();
        }
    }
    planted
        .first()
        .map(|p| p.fidelity.clone())
        .unwrap_or_else(|| "unknown".into())
}

/// The line the run prints. The scorer does the arithmetic; this says what
/// happened, split by the dimension the record carries.
pub fn report(records: &[WebRecord]) -> String {
    let mut by_fidelity: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for record in records {
        let entry = by_fidelity.entry(record.fidelity.as_str()).or_default();
        entry.1 += 1;
        if record.recalled() {
            entry.0 += 1;
        }
    }

    let mut out = String::from("| Fidelity | Recalled | Asked |\n| --- | --- | --- |\n");
    for (fidelity, (hit, total)) in &by_fidelity {
        out.push_str(&format!("| {fidelity} | {hit} | {total} |\n"));
    }
    let hit = records.iter().filter(|r| r.recalled()).count();
    out.push_str(&format!("| all | {hit} | {} |\n", records.len()));

    let errors: usize = records.iter().map(|r| r.fetch_errors).sum();
    if errors > 0 {
        out.push_str(&format!(
            "\n{errors} fetches failed across {} queries\n",
            records.len()
        ));
    }
    out
}
