//! The fan-out. Doc 05 sections 2, 5 and 10.
//!
//! The Planner says which retrievers each sub-question needs; this runs them
//! and hands the Synthesizer one ordered list. Everything interesting here is
//! about what happens when one of them fails, because doc 05 section 10's
//! posture is the whole design: tolerant per assignment, strict on hooks. A
//! retriever may return nothing. It may never return something it was told not
//! to touch.
//!
//! Order matters at the end. Doc 06 section A4 promises the Synthesizer
//! passages "ordered by trust rank then score", and that promise is what lets
//! the Synthesizer prefer the regulation over a blog without knowing which
//! retriever produced either.

use std::borrow::Cow;
use std::sync::Arc;

use serde_json::{Value, json};
use tessera_doctrine::DoctrinePack;
use tessera_harness::hooks::{HookContext, HookSet, Phase};
use tessera_harness::{Admission, Ledger};
use tessera_retrievers::contract::Packet;
use tessera_retrievers::embed::Embedder;
use tessera_retrievers::web::{HttpFetcher, WebConfig};
use tessera_retrievers::{IndexedConfig, indexed, web};
use tessera_store::Store;
use tessera_store::repo::{self, NewPassage, RetrievalRef, WatchedFolder};

/// What this profile can retrieve from. Built once per run from the pack's
/// enabled retrievers and the profile's watched folders.
#[derive(Clone, Default)]
pub struct RetrieverSet {
    /// Retriever id to the folders it reads. Absent means not configured, which
    /// is different from configured and empty: doc 05 section 10's
    /// `connector_unavailable` is a Profile problem the user can fix.
    pub indexed: Vec<(String, IndexedConfig)>,
    /// Doc 05 section 8.1. Present once the pack enables `web` and the profile
    /// has said where it may read from: a web retriever with no seed reaches
    /// nothing, which is the same "configured and empty" the folders have.
    pub web: Option<WebConfig>,
    pub embedder: Option<Arc<dyn Embedder>>,
}

impl RetrieverSet {
    pub fn is_empty(&self) -> bool {
        self.indexed.is_empty() && self.web.is_none()
    }

    /// Whether this profile has told the retriever where to read from.
    ///
    /// Doc 05 section 10 separates "not configured" from "configured and
    /// empty": the first is a Profile problem the user can fix, and the Profile
    /// page is where they see which it is.
    pub fn configured(&self, retriever_id: &str) -> bool {
        if retriever_id == "web" {
            return self.web.is_some();
        }
        self.config(retriever_id).is_some()
    }

    /// The view of this set one run is allowed to use.
    ///
    /// A run narrows the set rather than the fan-out skipping ids as it goes,
    /// because the plan-less fallback in `assignments` reads the set too and
    /// two places deciding what a notebook question may open is how one of them
    /// ends up opening the web. Doc 16 section 4's notebook is vault plus
    /// boards; doc 17 section 5's lesson adds the research profile.
    pub fn restricted(&self, allow: &[&str]) -> RetrieverSet {
        RetrieverSet {
            indexed: self
                .indexed
                .iter()
                .filter(|(id, _)| allow.contains(&id.as_str()))
                .cloned()
                .collect(),
            web: self.web.clone().filter(|_| allow.contains(&"web")),
            embedder: self.embedder.clone(),
        }
    }

    fn config(&self, retriever_id: &str) -> Option<&IndexedConfig> {
        self.indexed
            .iter()
            .find(|(id, _)| id == retriever_id)
            .map(|(_, c)| c)
    }
}

/// Build the set from the pack's enabled retrievers and the profile's folders.
///
/// This is the answer to "what can this profile actually read", and it is
/// deliberately narrower than the pack's list. Doc 05 section 10 separates a
/// retriever that is not configured from one that is configured and empty: the
/// first is a Profile problem the user can fix, and saying a retriever is
/// configured when nothing has told it where to read would put a
/// `connector_unavailable` at the bottom of a card instead of on the page that
/// can fix it.
///
/// So `local` appears only once a folder is watched, `boards` only while memory
/// is on, and `regulatory`, `web` and `structured` do not appear at all yet:
/// subscriptions and the web retriever are later phases, and until they exist
/// the honest report is that the pack wants them and the profile has not got
/// them.
pub fn assemble(
    pack: &DoctrinePack,
    folders: &[WatchedFolder],
    memory_enabled: bool,
    web_seeds: &[String],
) -> RetrieverSet {
    let mut indexed: Vec<(String, IndexedConfig)> = Vec::new();
    let mut web_config: Option<WebConfig> = None;
    for retriever in &pack.retrievers {
        if !retriever.enabled_by_default {
            continue;
        }
        match retriever.id.as_str() {
            "local" => {
                // Every watched folder, because doc 05 section 8.2's local
                // retriever reads the folders the profile added and the
                // Planner narrows to one with a `folder` filter when it wants
                // one. The boards index shares the table and is not a folder
                // on disk.
                let folder_ids: Vec<String> = folders
                    .iter()
                    .filter(|f| f.id != tessera_retrievers::boards::BOARDS_FOLDER)
                    .map(|f| f.id.clone())
                    .collect();
                if !folder_ids.is_empty() {
                    indexed.push(("local".to_string(), IndexedConfig::local(folder_ids)));
                }
            }
            // Doc 16 section 3.3: the vault is indexed like any folder, under
            // its own class. Always configured when the pack enables it, unlike
            // `local`, because the profile's own pages need nothing pointed at
            // them: an empty vault is a retriever with nothing to find rather
            // than one nobody has set up.
            "vault" => {
                indexed.push((
                    "vault".to_string(),
                    IndexedConfig::pages(vec![tessera_retrievers::VAULT_FOLDER.to_string()]),
                ));
            }
            // Doc 15 section 6: memory is a profile switch, and a set built
            // while it is off must not carry the index a plan-less card would
            // fall back to.
            "boards" if memory_enabled => {
                indexed.push(("boards".to_string(), IndexedConfig::boards()));
            }
            // Doc 05 section 8.1. Configured once the profile has said where it
            // may read from, and not before: a web retriever with no seed
            // reaches nothing, and reporting it as configured would put a
            // `connector_unavailable` at the bottom of a card rather than on
            // the Profile page that can fix it.
            "web" if !web_seeds.is_empty() => {
                web_config = Some(WebConfig::new(web_seeds.to_vec()));
            }
            _ => {}
        }
    }
    RetrieverSet {
        indexed,
        web: web_config,
        // No embedder in the product yet: the local model is a download the
        // app does not ship, so retrieval runs the lexical half. The eval
        // passes one when the machine has it, which is why the number it
        // reports is the better of the two.
        embedder: None,
    }
}

/// The retriever ids a plan-less run falls back to, now that not every one of
/// them is index backed.
fn ids(set: &RetrieverSet) -> Vec<String> {
    let mut out: Vec<String> = set.indexed.iter().map(|(id, _)| id.clone()).collect();
    if set.web.is_some() {
        out.push("web".to_string());
    }
    out
}

/// One assignment as the Planner wrote it.
struct Assignment {
    retriever_id: String,
    query: String,
    sq_id: Option<String>,
    max_passages: usize,
    version_ref: Option<String>,
    folder: Option<String>,
}

/// Read the Planner's assignments, or invent the one a plan-less deep card
/// needs.
///
/// A deep card without a plan still has to retrieve, and the question is the
/// only query there is. Skipping retrieval because no plan named a
/// sub-question would make the presence of a plan decide whether the card has
/// sources, which is not a distinction anybody asked for.
fn assignments(plan: Option<&Value>, question: &str, set: &RetrieverSet) -> Vec<Assignment> {
    if let Some(plan) = plan
        && let Some(sub_questions) = plan["sub_questions"].as_array()
        && !sub_questions.is_empty()
    {
        let mut out = Vec::new();
        for sq in sub_questions {
            let sq_id = sq["sq_id"].as_str().map(str::to_string);
            for r in sq["retrievers"].as_array().into_iter().flatten() {
                let Some(retriever_id) = r["id"].as_str() else {
                    continue;
                };
                out.push(Assignment {
                    retriever_id: retriever_id.to_string(),
                    query: r["query"]
                        .as_str()
                        .or_else(|| sq["text"].as_str())
                        .unwrap_or(question)
                        .to_string(),
                    sq_id: sq_id.clone(),
                    max_passages: r["max_passages"].as_u64().unwrap_or(12) as usize,
                    version_ref: r["filters"]["version_ref"].as_str().map(str::to_string),
                    folder: r["filters"]["folder"].as_str().map(str::to_string),
                });
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    ids(set)
        .into_iter()
        .map(|id| Assignment {
            retriever_id: id,
            query: question.to_string(),
            sq_id: None,
            max_passages: 12,
            version_ref: None,
            folder: None,
        })
        .collect()
}

pub struct FanOut {
    /// Ordered as doc 06 section A4 promises: trust rank, then score.
    pub passages: Vec<Value>,
    /// Doc 05 section 8.5's `builds_on`, collected here rather than derived
    /// downstream. The Synthesizer's packet carries a trimmed source per doc 06
    /// section A4 and has no locator in it, so the only place that knows which
    /// prior card a passage came from is the place that fetched it.
    pub builds_on: Vec<Value>,
    /// Doc 05 section 10: the card caveat names the exclusion category and
    /// never the excluded item.
    pub caveats: Vec<String>,
    pub assignments_run: usize,
    pub assignments_failed: usize,
}

/// Run every assignment and collect what came back.
///
/// Sequential today. The ledger slot is taken and released around each one
/// anyway, because the accounting is what doc 10 section 6's limit is for and
/// because the shape of the loop is where parallelism goes when the web
/// connector arrives and the wait becomes a network round trip rather than a
/// SQLite query measured in microseconds.
#[allow(clippy::too_many_arguments)]
pub fn run(
    store: &mut Store,
    ledger: &Ledger,
    set: &RetrieverSet,
    profile_id: &str,
    at: RetrievalRef<'_>,
    plan: Option<&Value>,
    question: &str,
    doctrine: &Value,
    must_exclude: &[String],
    posture: Posture<'_>,
) -> FanOut {
    let mut out = FanOut {
        passages: Vec::new(),
        builds_on: Vec::new(),
        caveats: Vec::new(),
        assignments_run: 0,
        assignments_failed: 0,
    };
    let set: Cow<'_, RetrieverSet> = match posture.allow {
        Some(allow) => Cow::Owned(set.restricted(allow)),
        None => Cow::Borrowed(set),
    };
    // Doc 17 section 5's larger fetch budget, applied to the connector that has
    // one. A lesson reads more widely than a card because it is building
    // somebody's understanding of a topic rather than answering one question.
    let set: Cow<'_, RetrieverSet> = match (posture.fetch_budget, &set.web) {
        (Some(budget), Some(_)) => {
            let mut widened = set.into_owned();
            if let Some(web) = widened.web.as_mut() {
                web.max_fetch = budget;
            }
            Cow::Owned(widened)
        }
        _ => set,
    };
    let set = set.as_ref();
    if set.is_empty() {
        return out;
    }

    let hooks = HookSet::retriever_defaults();
    let denied_domains: Vec<String> = doctrine["denied_domains"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    for assignment in assignments(plan, question, set) {
        let web_config = (assignment.retriever_id == "web")
            .then_some(set.web.as_ref())
            .flatten();
        let indexed_config = set.config(&assignment.retriever_id);
        if indexed_config.is_none() && web_config.is_none() {
            // Doc 05 section 10 `connector_unavailable`: coverage none, the
            // other retrievers carry on, and the user is told in the Profile
            // rather than in the card.
            continue;
        }

        let here = RetrievalRef {
            retriever_id: &assignment.retriever_id,
            ..at
        };

        // Doc 05 section 15's pre hooks, before anything is opened. A denial is
        // a hard stop for this assignment and nothing else.
        let context = HookContext {
            retriever_id: &assignment.retriever_id,
            run_id: at.run_id,
            query: Some(&assignment.query),
            target: assignment.folder.as_deref(),
            excluded_paths: must_exclude,
            denied_domains: &denied_domains,
        };
        if let Some(denial) = hooks.run(Phase::Pre, &context) {
            let _ = repo::record_hook_denial(store, here, &denial.hook_id, &denial.category);
            if !out.caveats.contains(&denial.category) {
                out.caveats.push(denial.category);
            }
            out.assignments_failed += 1;
            continue;
        }

        let admitted = matches!(ledger.try_take_retriever_slot(), Admission::Admitted);
        if !admitted {
            // Over the limit rather than broken. Doc 10 section 6 caps
            // assignments in flight, and the honest response is to run fewer,
            // not to queue forever inside one card.
            out.assignments_failed += 1;
            continue;
        }

        let started = std::time::Instant::now();
        let packet = build_packet(&assignment, doctrine, must_exclude, posture.must_include, at);
        let _ = repo::start_retrieval(store, here, &assignment.query);

        // Doc 05 section 8.1's web leg and sections 8.2 to 8.5's index legs
        // reach the same contract by different roads. The tolerant posture is
        // the same for both: an assignment that produced nothing leaves the
        // card standing on whatever the others found.
        let result = match (indexed_config, web_config) {
            (Some(config), _) => {
                indexed::retrieve(store.conn(), config, &packet, set.embedder.as_deref()).ok()
            }
            (None, Some(config)) => Some(web::retrieve(&HttpFetcher::new(), config, &packet)),
            (None, None) => None,
        };
        ledger.release_retriever_slot();

        let retrieved = match result {
            Some(r) => r,
            None => {
                // Tolerant: this assignment produced nothing and the card
                // continues on whatever the others found.
                out.assignments_failed += 1;
                continue;
            }
        };

        // Doc 05 section 10: a denial the connector reported is the same
        // caveat the fan-out writes for one it caught itself. Only the web
        // retriever can report one, because only it knows the URLs.
        for caveat in &retrieved.caveats {
            if !out.caveats.contains(caveat) {
                out.caveats.push(caveat.clone());
            }
        }

        let rows: Vec<NewPassage<'_>> = retrieved
            .passages
            .iter()
            .map(|p| NewPassage {
                class: &p.source.class,
                title: &p.source.title,
                locator: &p.source.locator,
                issuer: p.source.issuer.as_deref(),
                published_at: p.source.published_at.as_deref(),
                freshness_class: &p.source.freshness_class,
                trust_rank: p.source.trust_rank,
                version_ref: p.source.version_ref.as_deref(),
                content_hash: &p.source.content_hash,
                text: &p.text,
                location: p.location.clone(),
                // Doc 01 open question 2. A folder marked sensitive would set
                // this; the exclusion hook means such a folder is never read at
                // all in this build, so nothing reaches here withheld yet.
                text_withheld: false,
            })
            .collect();

        let coverage = match retrieved.coverage {
            tessera_retrievers::Coverage::Full => "full",
            tessera_retrievers::Coverage::Partial => "partial",
            tessera_retrievers::Coverage::None => "none",
        };

        let retained = repo::record_retrieval(
            store,
            profile_id,
            here,
            &rows,
            coverage,
            started.elapsed().as_millis(),
        );

        // The stored passage id is what a citation will point at, so the
        // Synthesizer has to see that one and not the index entry id it was
        // found by.
        let (ids, stale) = retained.map(|r| (r.passage_ids, r.stale)).unwrap_or_default();
        for (i, passage) in retrieved.passages.iter().enumerate() {
            // A prior card's locator is `board_id/card_id`, which is exactly
            // what doc 01 section 4.4 records and what doc 15's ground truth
            // names a prior card by.
            if passage.source.class == "own_card"
                && let Some((board, card)) = passage.source.locator.split_once('/')
            {
                let entry = json!({
                    "board_id": board,
                    "card_id": card,
                    "verified_at": tessera_store::now_iso8601(),
                });
                if !out.builds_on.contains(&entry) {
                    out.builds_on.push(entry);
                }
            }

            out.passages.push(json!({
                "passage_id": ids.get(i).cloned().unwrap_or_else(|| passage.passage_id.clone()),
                "sq_id": assignment.sq_id,
                "text": passage.text,
                "score": passage.score,
                "source": {
                    "title": passage.source.title,
                    "class": passage.source.class,
                    "issuer": passage.source.issuer,
                    "trust_rank": passage.source.trust_rank,
                    "published_at": passage.source.published_at,
                    "version_ref": passage.source.version_ref,
                    // Doc 07 section B8.4's freshness check reads these. A source
                    // a re-verification already marked stale is still stale when
                    // it is reached again, so the state comes from the row rather
                    // than from the fact that this run just read it.
                    "stale": stale.get(i).map(Option::is_some).unwrap_or(false),
                    "stale_reason": stale.get(i).cloned().flatten(),
                    "locator": passage.source.locator,
                },
            }));
        }
        out.assignments_run += 1;
    }

    order(&mut out.passages);
    out
}

/// How one run reads, beyond what it is asking. Doc 16 section 3.4 and doc 17
/// section 5 are the two that narrow or widen it; every other run takes the
/// default, which is every configured retriever at the ordinary budget.
#[derive(Debug, Clone, Copy, Default)]
pub struct Posture<'a> {
    /// The retrievers this run may use, or every configured one. A narrowed run
    /// is a policy choice rather than a missing connector, so what it leaves
    /// out is left out silently and no caveat names it.
    pub allow: Option<&'a [&'a str]>,
    /// Doc 17 section 5: a path's `sources_hint` locators, read first.
    pub must_include: &'a [String],
    /// Doc 17 section 5: how many pages a lesson may fetch, when more than the
    /// connector's own default.
    pub fetch_budget: Option<usize>,
}

fn build_packet(
    assignment: &Assignment,
    doctrine: &Value,
    must_exclude: &[String],
    must_include: &[String],
    at: RetrievalRef<'_>,
) -> Packet {
    serde_json::from_value(json!({
        "schema_version": "1.0",
        "run_id": at.run_id,
        "card_id": at.card_id,
        "sq_id": assignment.sq_id,
        "retriever_id": assignment.retriever_id,
        "query": assignment.query,
        "filters": {
            "version_ref": assignment.version_ref,
            "folder": assignment.folder,
        },
        "max_passages": assignment.max_passages,
        "must_exclude": must_exclude,
        "must_include": must_include,
        "doctrine": doctrine,
    }))
    .unwrap_or_else(|_| Packet {
        run_id: at.run_id.to_string(),
        card_id: None,
        sq_id: None,
        retriever_id: assignment.retriever_id.clone(),
        query: assignment.query.clone(),
        filters: Default::default(),
        max_passages: assignment.max_passages,
        must_exclude: Vec::new(),
        must_include: Vec::new(),
        doctrine: Default::default(),
    })
}

/// Doc 06 section A4: trust rank, then score.
///
/// Rank ascends because rank 1 is the regulation and rank 8 is something
/// somebody typed. Score descends within a rank. Ties break on the passage id
/// so two runs over one corpus hand the Synthesizer the same list, which every
/// comparison between two eval runs depends on.
fn order(passages: &mut [Value]) {
    passages.sort_by(|a, b| {
        let rank = |v: &Value| v["source"]["trust_rank"].as_i64().unwrap_or(9);
        let score = |v: &Value| v["score"].as_f64().unwrap_or(0.0);
        let id = |v: &Value| v["passage_id"].as_str().unwrap_or_default().to_string();
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| id(a).cmp(&id(b)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str) -> WatchedFolder {
        WatchedFolder {
            id: id.to_string(),
            root: format!("/tmp/{id}"),
            label: id.to_string(),
            sensitive: false,
            embeddings: "local".to_string(),
            last_indexed_at: None,
        }
    }

    /// The smallest pack that parses, carrying only the field under test.
    fn pack(retrievers: &[(&str, bool)]) -> DoctrinePack {
        let retrievers: Vec<Value> = retrievers
            .iter()
            .map(|(id, enabled)| json!({ "id": id, "enabled_by_default": enabled }))
            .collect();
        serde_json::from_value(json!({
            "code": "test",
            "version": "0.1.0",
            "audiences": [],
            "source_hierarchy": [],
            "freshness_classes": {},
            "flag_rules": [],
            "retrievers": retrievers,
            "exercise_templates": [],
        }))
        .expect("the fixture pack parses")
    }

    #[test]
    fn local_is_configured_once_a_folder_is_watched() {
        let pack = pack(&[("local", true), ("boards", true)]);
        let empty = assemble(&pack, &[], true, &[]);
        assert!(
            !empty.configured("local"),
            "a local retriever with nowhere to read is not configured"
        );

        let watched = assemble(&pack, &[folder("f1"), folder("f2")], true, &[]);
        assert!(watched.configured("local"));
        assert_eq!(
            watched.config("local").expect("local").folder_ids,
            vec!["f1".to_string(), "f2".to_string()],
            "every watched folder, because the Planner narrows with a filter"
        );
    }

    #[test]
    fn the_boards_index_is_not_a_folder_the_local_retriever_reads() {
        // It shares the table with folders on disk, and a local assignment that
        // reached it would return prior cards as local documents, which is the
        // one class doc 15 section 2 will not let stand as evidence.
        let set = assemble(
            &pack(&[("local", true)]),
            &[folder(tessera_retrievers::boards::BOARDS_FOLDER)],
            true,
            &[],
        );
        assert!(!set.configured("local"));
    }

    #[test]
    fn memory_off_leaves_the_boards_index_out_of_the_set() {
        let pack = pack(&[("boards", true)]);
        assert!(assemble(&pack, &[], true, &[]).configured("boards"));
        assert!(!assemble(&pack, &[], false, &[]).configured("boards"));
    }

    #[test]
    fn the_vault_is_configured_without_anybody_pointing_at_it() {
        // Unlike `local`, which waits for a folder. The profile's own pages are
        // already where the app can read them, so an empty vault is a retriever
        // with nothing to find rather than one nobody has set up.
        let set = assemble(&pack(&[("vault", true)]), &[], true, &[]);
        assert!(set.configured("vault"));
        assert_eq!(
            set.config("vault").expect("vault").source_class,
            "page",
            "a page must not enter as a local document: doc 16 section 3.3"
        );

        // And a pack that turns it off gets no vault.
        assert!(!assemble(&pack(&[("vault", false)]), &[], true, &[]).configured("vault"));
    }

    #[test]
    fn a_retriever_the_product_has_not_built_is_reported_unconfigured() {
        // Doc 05 section 10's `connector_unavailable`. The finance pack enables
        // regulatory, web and structured; none of them has anywhere to read
        // until subscriptions exist and the profile names a seed, and claiming
        // otherwise would put the failure at the bottom of a card instead of on
        // the page that can fix it.
        let set = assemble(
            &pack(&[("regulatory", true), ("web", true), ("structured", true)]),
            &[folder("f1")],
            true,
            &[],
        );
        assert!(set.is_empty());
        for id in ["regulatory", "web", "structured"] {
            assert!(!set.configured(id), "{id} claimed a connector it has not got");
        }
    }

    #[test]
    fn the_web_retriever_arrives_with_the_seed_and_not_before() {
        // Doc 05 section 8.1: a web retriever with nowhere to read reaches
        // nothing, and a profile that has named no seed is not pointed at the
        // internet by default. The seed is what turns it on.
        let pack = pack(&[("web", true)]);
        assert!(!assemble(&pack, &[], true, &[]).configured("web"));

        let seeded = assemble(&pack, &[], true, &["http://127.0.0.1:9/".to_string()]);
        assert!(seeded.configured("web"));
        assert!(!seeded.is_empty());
        // And a plan-less run falls back to it, which is the only way a card
        // with no plan reaches the web at all.
        assert_eq!(assignments(None, "q", &seeded).len(), 1);

        // A restricted run leaves it out unless it was allowed, which is what
        // keeps doc 16 section 4's notebook off the web.
        assert!(!seeded.restricted(&["vault", "boards"]).configured("web"));
        assert!(seeded.restricted(&["web"]).configured("web"));
    }

    #[test]
    fn a_restricted_run_sees_only_what_it_was_allowed() {
        let set = assemble(
            &pack(&[("local", true), ("boards", true)]),
            &[folder("f1")],
            true,
            &[],
        );
        let narrowed = set.restricted(&["boards"]);
        assert!(narrowed.configured("boards"));
        assert!(!narrowed.configured("local"));
        // And the fallback reads the narrowed set, so a plan-less run cannot
        // reach round it.
        assert_eq!(assignments(None, "q", &narrowed).len(), 1);
    }

    fn passage(rank: i64, score: f64, id: &str) -> Value {
        json!({ "passage_id": id, "score": score, "source": { "trust_rank": rank } })
    }

    #[test]
    fn trust_rank_beats_score() {
        // The promise the Synthesizer relies on: a regulation with a mediocre
        // match outranks a blog post that matched beautifully.
        let mut p = vec![passage(7, 0.99, "blog"), passage(1, 0.10, "regulation")];
        order(&mut p);
        assert_eq!(p[0]["passage_id"], "regulation");
    }

    #[test]
    fn score_orders_within_one_rank() {
        let mut p = vec![passage(4, 0.20, "b"), passage(4, 0.80, "a")];
        order(&mut p);
        assert_eq!(p[0]["passage_id"], "a");
    }

    #[test]
    fn an_unranked_passage_sorts_last() {
        let mut p = vec![
            json!({ "passage_id": "x", "score": 1.0, "source": {} }),
            passage(4, 0.1, "a"),
        ];
        order(&mut p);
        assert_eq!(p[0]["passage_id"], "a");
    }

    #[test]
    fn ordering_is_stable_across_runs() {
        let build = || vec![passage(4, 0.5, "b"), passage(4, 0.5, "a"), passage(4, 0.5, "c")];
        let (mut first, mut second) = (build(), build());
        order(&mut first);
        order(&mut second);
        assert_eq!(first, second);
        assert_eq!(first[0]["passage_id"], "a");
    }

    #[test]
    fn a_plan_less_deep_card_still_gets_one_assignment_per_retriever() {
        // Otherwise the presence of a plan would decide whether a card has
        // sources, which is not a distinction anybody asked for.
        let set = RetrieverSet {
            indexed: vec![
                ("regulatory".into(), IndexedConfig::regulatory("reg")),
                ("local".into(), IndexedConfig::local(vec!["local".into()])),
            ],
            web: None,
            embedder: None,
        };
        let out = assignments(None, "what applies?", &set);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|a| a.query == "what applies?"));
    }

    #[test]
    fn a_plan_with_sub_questions_wins_over_the_default() {
        let set = RetrieverSet {
            indexed: vec![("regulatory".into(), IndexedConfig::regulatory("reg"))],
            web: None,
            embedder: None,
        };
        let plan = json!({
            "sub_questions": [{
                "sq_id": "sq-1",
                "text": "what is the buffer",
                "retrievers": [{ "id": "regulatory", "query": "buffer", "max_passages": 4 }]
            }]
        });
        let out = assignments(Some(&plan), "the original question", &set);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].query, "buffer");
        assert_eq!(out[0].sq_id.as_deref(), Some("sq-1"));
        assert_eq!(out[0].max_passages, 4);
    }

    #[test]
    fn an_empty_plan_falls_back_rather_than_retrieving_nothing() {
        let set = RetrieverSet {
            indexed: vec![("local".into(), IndexedConfig::local(vec!["local".into()]))],
            web: None,
            embedder: None,
        };
        let out = assignments(Some(&json!({ "sub_questions": [] })), "q", &set);
        assert_eq!(out.len(), 1, "an empty plan silenced retrieval");
    }
}
