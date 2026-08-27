//! The web retriever. Doc 05 section 8.1.
//!
//! The first component in this build that opens a socket. Everything else reads
//! a file the user pointed at or a row the product wrote, so this one carries
//! the rules that matter when the thing on the other end is not yours:
//!
//! - **Nothing is reached that was not pointed at.** A profile configures seeds,
//!   discovery never leaves a seed's host, and a page linking somewhere else is
//!   a link this retriever does not follow. In evaluation every seed is
//!   loopback, so the whole leg is structurally incapable of leaving the
//!   machine, and that is a property of the design rather than of the test.
//! - **The hooks run per URL, not per assignment.** Doc 05 section 15 puts the
//!   domain denylist before anything is opened. The fan-out checks the
//!   assignment; only this module knows the URLs, so it runs the same
//!   `HookSet` again on each one rather than keeping a second denylist.
//! - **The same bytes give the same answer.** The content hash is of the body
//!   as fetched, and ranking is deterministic, so two sweeps over one corpus
//!   produce identical rows. A retriever whose output moved between runs would
//!   make every number downstream of it unreadable.
//!
//! What is deliberately absent is search. Doc 05 section 8.1 opens with a
//! search API and the user's key, which is a live, paid dependency; the rest of
//! the pipeline (fetch, extract, chunk, rank, persist) is the part that decides
//! whether a citation is any good, and it is measurable for nothing against the
//! synthetic web. Discovery here walks the seeds, and the day a search key is
//! added it becomes one more way to produce candidate URLs.

use std::collections::BTreeSet;

use scraper::{Html, Selector};
use serde_json::json;
use sha2::{Digest, Sha256};
use tessera_harness::hooks::{HookContext, HookSet, Phase};

use crate::chunking::Chunk;
use crate::contract::{Coverage, Packet, Passage, Retrieved, Source, cap, dedupe_key};
use crate::parse::html;

/// Doc 05 section 8.1: "Fetch: top eight results".
pub const MAX_FETCH: usize = 8;

/// How many pages discovery will look at per seed before it stops.
///
/// A listing with a thousand links is a listing this retriever reads the head
/// of. The cap is on candidates rather than on fetches so that the denylist and
/// the ranking still see a spread, and the run says how many it looked at.
const MAX_CANDIDATES: usize = 64;

/// What this profile may reach. Doc 05 section 8.1.
#[derive(Debug, Clone, Default)]
pub struct WebConfig {
    /// The bases discovery starts from. A page outside every seed's host is
    /// never fetched, whatever links to it.
    pub seeds: Vec<String>,
    /// Doc 05 section 8.1's eight, as a setting so a lesson can ask for fewer.
    pub max_fetch: usize,
}

impl WebConfig {
    pub fn new(seeds: Vec<String>) -> Self {
        Self {
            seeds,
            max_fetch: MAX_FETCH,
        }
    }
}

/// One page as it came off the wire.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The host said no, or said something this retriever does not read.
    Refused(String),
    /// Nothing answered.
    Unreachable(String),
}

/// How pages are read. A trait so the ranking and the extraction can be tested
/// without a socket, and so the eval can point the same code at a loopback
/// server rather than at a second implementation.
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Fetched, FetchError>;
}

/// The real fetcher, over HTTP.
///
/// Deliberately thin: a GET with a timeout, a body size cap and a content type
/// check. Everything about what may be reached is decided before this is
/// called, by the seeds and by the hooks, so this does not need an opinion.
pub struct HttpFetcher {
    agent: ureq::Agent,
    /// Bodies larger than this are refused rather than read. A retriever that
    /// pulled a hundred megabyte page into memory to find one sentence would be
    /// a denial of service the user aimed at themselves.
    max_bytes: usize,
}

/// Doc 05's per assignment budget, applied per fetch.
pub const FETCH_TIMEOUT_SECS: u64 = 10;
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpFetcher {
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
                .timeout_read(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
                .build(),
            max_bytes: MAX_BODY_BYTES,
        }
    }
}

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str) -> Result<Fetched, FetchError> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;

        // Doc 05 section 8.1 reads pages. Anything else is refused before it is
        // read, because a retriever that ran html5ever over a pdf would produce
        // a passage of nothing and cite it.
        let content_type = response.content_type().to_string();
        if !content_type.starts_with("text/html") && !content_type.starts_with("text/plain") {
            return Err(FetchError::Refused(content_type));
        }

        use std::io::Read;
        let mut body = String::new();
        response
            .into_reader()
            .take(self.max_bytes as u64)
            .read_to_string(&mut body)
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;

        Ok(Fetched {
            url: url.to_string(),
            body,
        })
    }
}

/// Run one assignment against the web. Doc 05 section 8.1.
pub fn retrieve(fetcher: &dyn Fetcher, config: &WebConfig, packet: &Packet) -> Retrieved {
    let mut out = Retrieved::default();
    if config.seeds.is_empty() {
        return out;
    }

    let hooks = HookSet::retriever_defaults();
    let allowed = |url: &str| {
        let context = HookContext {
            retriever_id: "web",
            run_id: &packet.run_id,
            query: Some(&packet.query),
            target: Some(url),
            excluded_paths: &packet.must_exclude,
            denied_domains: &packet.doctrine.denied_domains,
        };
        hooks.run(Phase::Pre, &context)
    };

    let mut denied: BTreeSet<String> = BTreeSet::new();
    let mut candidates: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for seed in &config.seeds {
        // The seed itself is checked before it is opened, which is the whole
        // point of a pre hook: a denied domain is never reached, not fetched
        // and then discarded.
        if let Some(denial) = allowed(seed) {
            denied.insert(denial.category);
            continue;
        }
        let Ok(listing) = fetcher.get(seed) else {
            out.fetch_errors += 1;
            continue;
        };
        // A seed that is itself a page is a candidate; a seed that is a listing
        // contributes its links. Both are true of the same fetch, so the body
        // is used twice rather than fetched twice.
        for url in links_within(&listing) {
            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
            if seen.insert(dedupe_key(&url)) {
                candidates.push(url);
            }
        }
        if has_prose(&listing.body) && seen.insert(dedupe_key(&listing.url)) {
            candidates.push(listing.url.clone());
        }
    }

    let max_fetch = if config.max_fetch == 0 {
        MAX_FETCH
    } else {
        config.max_fetch
    };

    let mut pages: Vec<(Fetched, Vec<Chunk>, html::HtmlMeta)> = Vec::new();
    for url in candidates {
        if pages.len() >= max_fetch {
            break;
        }
        if let Some(denial) = allowed(&url) {
            denied.insert(denial.category);
            continue;
        }
        let Ok(page) = fetcher.get(&url) else {
            out.fetch_errors += 1;
            continue;
        };
        let chunks = html::parse(&page.body);
        if chunks.is_empty() {
            continue;
        }
        let meta = html::meta(&page.body);
        pages.push((page, chunks, meta));
    }

    out.caveats.extend(denied);

    // Doc 05 section 8.1: "BM25 over chunks against the query". Over what was
    // just fetched rather than over an index, because there is no index: a web
    // page is read once for this question and the ranking is a fact about this
    // fetch.
    let corpus: Vec<&str> = pages
        .iter()
        .flat_map(|(_, chunks, _)| chunks.iter().map(|c| c.text.as_str()))
        .collect();
    let scores = bm25(&packet.query, &corpus);

    let mut scored: Vec<(f64, usize, usize)> = Vec::new();
    let mut at = 0usize;
    for (page_index, (_, chunks, _)) in pages.iter().enumerate() {
        for chunk_index in 0..chunks.len() {
            scored.push((scores[at], page_index, chunk_index));
            at += 1;
        }
    }
    // Sorted by score, then by where it sat in its page, so two runs over the
    // same bytes order ties the same way.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    scored.retain(|(score, _, _)| *score > 0.0);
    scored.truncate(packet.max_passages);

    let mut sources: BTreeSet<String> = BTreeSet::new();
    for (score, page_index, chunk_index) in scored {
        let (page, chunks, meta) = &pages[page_index];
        let chunk = &chunks[chunk_index];
        let locator = page.url.clone();
        let issuer = meta.issuer.clone().or_else(|| host_of(&locator));
        // Doc 05 section 5: the rank is doctrine's, never the retriever's own
        // opinion of the page it just read.
        let trust_rank = packet.doctrine.rank_for("web", issuer.as_deref());

        // Doc 05 section 8.1: "Source per page (dedupe by normalised URL)". The
        // mirror of one page under two URLs is one source.
        if sources.insert(dedupe_key(&locator)) {
            out.sources_created += 1;
        } else {
            out.sources_deduplicated += 1;
        }

        let content_hash = hash(&page.body);
        out.passages.push(Passage {
            passage_id: format!("{}#{}", content_hash, chunk.sequence),
            source_id: locator.clone(),
            text: cap(&chunk.text),
            location: json!(chunk.location),
            score,
            source: Source {
                class: "web".into(),
                title: meta.title.clone().unwrap_or_else(|| title_of(&locator)),
                locator,
                issuer,
                published_at: meta.published_at.clone(),
                trust_rank,
                // A web page ages like a web page. Doctrine decides how long
                // that is; naming the class here is what lets it.
                freshness_class: "web_page".into(),
                version_ref: packet.filters.version_ref.clone(),
                content_hash,
            },
        });
    }

    out.coverage = if out.passages.is_empty() {
        Coverage::None
    } else if out.passages.len() >= packet.max_passages {
        Coverage::Full
    } else {
        Coverage::Partial
    };
    out
}

/// Whether a fetched body reads as a page rather than as an index of pages.
///
/// A directory listing is links and nothing else, and indexing one puts a
/// source in the store whose whole content is the names of other sources. The
/// test is text that is not a link: a listing that happened to carry a sentence
/// is a page with links on it and reading it costs nothing, while one that is
/// only `<li><a>` is the index it looks like.
fn has_prose(body: &str) -> bool {
    let document = Html::parse_document(body);
    let Ok(blocks) = Selector::parse("p, li, td, blockquote, pre, h1, h2, h3, h4, h5, h6") else {
        return false;
    };
    let Ok(anchors) = Selector::parse("a") else {
        return false;
    };
    for element in document.select(&blocks) {
        let whole: String = element.text().collect();
        let linked: String = element.select(&anchors).flat_map(|a| a.text()).collect();
        let remaining = whole.replace(linked.trim(), "");
        if remaining.split_whitespace().count() >= 3 {
            return true;
        }
    }
    false
}

/// Every link on a page that stays on its own host.
///
/// The host is the boundary, so a synthetic corpus on loopback cannot reach the
/// internet and a profile pointed at one site cannot be walked onto another by
/// a page that links there. Doc 05 section 8.1's denylist is the second gate;
/// this is the first, and it is structural.
fn links_within(page: &Fetched) -> Vec<String> {
    let Some(host) = host_of(&page.url) else {
        return Vec::new();
    };
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let document = Html::parse_document(&page.body);
    let mut out = Vec::new();
    for element in document.select(&selector) {
        let href = element.value().attr("href").unwrap_or_default().trim();
        if href.is_empty() || href.starts_with('#') || href.starts_with("mailto:") {
            continue;
        }
        let Some(url) = resolve(&page.url, href) else {
            continue;
        };
        if host_of(&url).as_deref() != Some(host.as_str()) {
            continue;
        }
        // A listing links back to its own parent; following that is a walk up
        // and then down again over pages already seen.
        if href == ".." || href == "../" {
            continue;
        }
        out.push(url);
    }
    out
}

/// An absolute URL from a base and an href, for the two forms a listing writes.
fn resolve(base: &str, href: &str) -> Option<String> {
    if href.contains("://") {
        return Some(href.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if let Some(path) = href.strip_prefix('/') {
        return Some(format!("{scheme}://{authority}/{path}"));
    }
    // Relative to the directory the base names.
    let path = rest.strip_prefix(authority).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    let directory = match path.rfind('/') {
        Some(cut) => &path[..=cut],
        None => "/",
    };
    Some(format!("{scheme}://{authority}{directory}{href}"))
}

pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    (!authority.is_empty()).then(|| authority.to_lowercase())
}

fn title_of(locator: &str) -> String {
    locator
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(locator)
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(locator)
        .replace(['-', '_'], " ")
}

fn hash(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    hex::encode(hasher.finalize())
}

// ------------------------------------------------------------------ bm25 ---

/// Okapi BM25 over the chunks one fetch produced. Doc 05 section 8.1.
///
/// Written here rather than reused from the index because the index is SQLite's
/// FTS over rows that persist, and a web page is read once for one question. A
/// chunk that matches nothing scores zero and is dropped by the caller, so a
/// page about something else contributes nothing rather than contributing a
/// weak match that outranks silence.
fn bm25(query: &str, documents: &[&str]) -> Vec<f64> {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;

    let terms: Vec<String> = tokens(query);
    let docs: Vec<Vec<String>> = documents.iter().map(|d| tokens(d)).collect();
    if docs.is_empty() {
        return Vec::new();
    }
    let total = docs.len() as f64;
    let average = docs.iter().map(|d| d.len() as f64).sum::<f64>() / total;

    let mut scores = vec![0.0; docs.len()];
    for term in &terms {
        let containing = docs.iter().filter(|d| d.contains(term)).count() as f64;
        if containing == 0.0 {
            continue;
        }
        // The usual smoothed idf, floored at zero so a term in every document
        // adds nothing rather than subtracting.
        let idf = (((total - containing + 0.5) / (containing + 0.5)) + 1.0)
            .ln()
            .max(0.0);
        for (i, document) in docs.iter().enumerate() {
            let frequency = document.iter().filter(|t| *t == term).count() as f64;
            if frequency == 0.0 {
                continue;
            }
            let length = document.len() as f64;
            let denominator = frequency + K1 * (1.0 - B + B * length / average.max(1.0));
            scores[i] += idf * (frequency * (K1 + 1.0)) / denominator;
        }
    }
    scores
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A fetcher over a map, so extraction and ranking are tested without a
    /// socket. The socket has its own test, over loopback.
    struct Fixture(BTreeMap<String, String>);

    impl Fetcher for Fixture {
        fn get(&self, url: &str) -> Result<Fetched, FetchError> {
            self.0
                .get(url)
                .map(|body| Fetched {
                    url: url.to_string(),
                    body: body.clone(),
                })
                .ok_or_else(|| FetchError::Unreachable(url.to_string()))
        }
    }

    fn page(title: &str, body: &str) -> String {
        format!(
            "<!doctype html><html><head><title>{title}</title>\
             <meta name=\"issuer\" content=\"ledgerline.invalid\">\
             <meta name=\"published\" content=\"2025-05-22\"></head>\
             <body><h1>{title}</h1><p>{body}</p></body></html>"
        )
    }

    fn listing(names: &[&str]) -> String {
        let links: String = names
            .iter()
            .map(|n| format!("<li><a href=\"{n}\">{n}</a></li>"))
            .collect();
        format!("<html><body><ul>{links}</ul></body></html>", links = links)
    }

    fn corpus() -> Fixture {
        let mut map = BTreeMap::new();
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/".to_string(),
            listing(&["buffers.html", "outsourcing.html"]),
        );
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/buffers.html".to_string(),
            page(
                "Capital buffers explained",
                "The capital conservation buffer is 2.5 per cent of risk weighted assets.",
            ),
        );
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/outsourcing.html".to_string(),
            page(
                "Outsourcing notification",
                "The notification period before an outsourcing starts comes to 117 days.",
            ),
        );
        Fixture(map)
    }

    fn packet(query: &str) -> Packet {
        serde_json::from_value(json!({
            "run_id": "r",
            "retriever_id": "web",
            "query": query,
            "max_passages": 4,
            "doctrine": { "trust_ranks": [{ "class": "web", "rank": 6 }] }
        }))
        .expect("packet")
    }

    fn config() -> WebConfig {
        WebConfig::new(vec!["http://127.0.0.1:9/ledgerline.invalid/".into()])
    }

    #[test]
    fn a_listing_is_walked_and_the_page_that_answers_ranks_first() {
        let out = retrieve(&corpus(), &config(), &packet("capital conservation buffer"));
        assert!(!out.passages.is_empty(), "nothing was retrieved");
        assert!(
            out.passages[0].text.contains("2.5 per cent"),
            "the buffer page did not rank first: {:?}",
            out.passages[0].text
        );
        assert_eq!(out.passages[0].source.class, "web");
        assert_eq!(out.passages[0].source.trust_rank, 6, "doctrine sets the rank");
        assert_eq!(
            out.passages[0].source.published_at.as_deref(),
            Some("2025-05-22"),
            "doc 05 section 8.1's post hook reads published_at from the page"
        );
    }

    #[test]
    fn a_chunk_matching_nothing_is_dropped_rather_than_ranked_weakly() {
        // The failure this prevents: a page about outsourcing coming back for a
        // question about buffers, at a low score, and being cited anyway.
        let out = retrieve(&corpus(), &config(), &packet("capital conservation buffer"));
        assert!(
            out.passages.iter().all(|p| !p.text.contains("117 days")),
            "a page matching nothing was returned: {:?}",
            out.passages.iter().map(|p| &p.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_same_bytes_give_the_same_rows_twice() {
        // Doc 05's whole staleness story rests on a content hash that means
        // something, and a sweep that moved between runs would make every
        // number downstream of it unreadable.
        let first = retrieve(&corpus(), &config(), &packet("capital buffer"));
        let second = retrieve(&corpus(), &config(), &packet("capital buffer"));
        let ids = |r: &Retrieved| {
            r.passages
                .iter()
                .map(|p| (p.passage_id.clone(), p.source.content_hash.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second));
        assert!(!ids(&first).is_empty());
    }

    #[test]
    fn a_denied_domain_is_never_opened() {
        // Doc 05 section 15: the hook runs before anything is opened, and the
        // fan out cannot run it per URL because only this module has them.
        let mut p = packet("capital buffer");
        p.doctrine.denied_domains = vec!["127.0.0.1".into()];
        let out = retrieve(&corpus(), &config(), &p);
        assert!(out.passages.is_empty(), "a denied domain answered");
        assert!(
            out.caveats.iter().any(|c| c.contains("denied")),
            "the denial was not reported: {:?}",
            out.caveats
        );
    }

    #[test]
    fn discovery_never_leaves_the_hosts_it_was_pointed_at() {
        // The structural half of the rule. A page linking elsewhere is a link
        // this retriever does not follow, whatever doctrine says about it.
        let mut map = corpus().0;
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/".to_string(),
            "<html><body><a href=\"https://elsewhere.invalid/buffers.html\">buffers</a>\
             <a href=\"buffers.html\">ours</a></body></html>"
                .to_string(),
        );
        map.insert(
            "https://elsewhere.invalid/buffers.html".to_string(),
            page("Elsewhere", "The capital conservation buffer is 9 per cent."),
        );
        let out = retrieve(&Fixture(map), &config(), &packet("capital conservation buffer"));
        assert!(
            out.passages
                .iter()
                .all(|p| p.source.locator.contains("127.0.0.1")),
            "discovery left the host it was pointed at: {:?}",
            out.passages.iter().map(|p| &p.source.locator).collect::<Vec<_>>()
        );
        assert!(!out.passages.is_empty(), "the same host page was lost too");
    }

    #[test]
    fn one_page_under_two_urls_is_one_source() {
        // Doc 05 section 8.1: "dedupe by normalised URL".
        let mut map = corpus().0;
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/".to_string(),
            "<html><body><a href=\"buffers.html\">a</a>\
             <a href=\"buffers.html?utm_source=news\">b</a></body></html>"
                .to_string(),
        );
        map.insert(
            "http://127.0.0.1:9/ledgerline.invalid/buffers.html?utm_source=news".to_string(),
            page("Capital buffers explained", "The buffer is 2.5 per cent."),
        );
        let out = retrieve(&Fixture(map), &config(), &packet("capital buffer"));
        assert_eq!(out.sources_created, 1, "the mirror became a second source");
    }

    #[test]
    fn a_page_that_cannot_be_fetched_is_counted_and_the_rest_carry_on() {
        let mut map = corpus().0;
        map.remove("http://127.0.0.1:9/ledgerline.invalid/outsourcing.html");
        let out = retrieve(&Fixture(map), &config(), &packet("capital conservation buffer"));
        assert_eq!(out.fetch_errors, 1);
        assert!(!out.passages.is_empty(), "one bad page took the whole run down");
        // Doc 05 section 9: a fetch error costs two tenths of the confidence.
        assert!(out.confidence() < 1.0);
    }

    #[test]
    fn a_relative_href_resolves_against_the_directory_it_sits_in() {
        assert_eq!(
            resolve("http://h/a/b/index.html", "c.html").as_deref(),
            Some("http://h/a/b/c.html")
        );
        assert_eq!(
            resolve("http://h/a/b/", "c.html").as_deref(),
            Some("http://h/a/b/c.html")
        );
        assert_eq!(
            resolve("http://h/a/b/", "/c.html").as_deref(),
            Some("http://h/c.html")
        );
        assert_eq!(
            resolve("http://h/a/", "https://x/c.html").as_deref(),
            Some("https://x/c.html")
        );
    }

    #[test]
    fn a_directory_listing_is_walked_and_never_indexed_as_a_page() {
        // A listing indexed as a page is a source whose whole content is the
        // names of other sources, and it would rank against a question that
        // happened to share a word with a file name.
        let out = retrieve(&corpus(), &config(), &packet("capital conservation buffer"));
        assert!(
            out.passages.iter().all(|p| !p.source.locator.ends_with('/')),
            "the listing itself was indexed: {:?}",
            out.passages.iter().map(|p| &p.source.locator).collect::<Vec<_>>()
        );
        // And a page that carries links as well as prose is still a page.
        assert!(has_prose(
            "<html><body><p>The buffer is 2.5 per cent of risk weighted assets, \
             see <a href=\"x\">the note</a>.</p></body></html>"
        ));
        assert!(!has_prose(&listing(&["a.html", "b.html"])));
    }

    #[test]
    fn bm25_ranks_the_document_that_says_it_over_the_one_that_mentions_it() {
        let scores = bm25(
            "capital buffer",
            &[
                "The capital conservation buffer is 2.5 per cent of risk weighted assets.",
                "Outsourcing has nothing to do with any of this at all whatsoever.",
            ],
        );
        assert!(scores[0] > scores[1]);
        assert_eq!(scores[1], 0.0, "a document matching nothing scored above zero");
    }
}
