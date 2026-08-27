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

use std::sync::Arc;

use serde_json::{Value, json};
use tessera_harness::hooks::{HookContext, HookSet, Phase};
use tessera_harness::{Admission, Ledger};
use tessera_retrievers::contract::Packet;
use tessera_retrievers::embed::Embedder;
use tessera_retrievers::{IndexedConfig, indexed};
use tessera_store::Store;
use tessera_store::repo::{self, NewPassage, RetrievalRef};

/// What this profile can retrieve from. Built once per run from the pack's
/// enabled retrievers and the profile's watched folders.
#[derive(Clone, Default)]
pub struct RetrieverSet {
    /// Retriever id to the folders it reads. Absent means not configured, which
    /// is different from configured and empty: doc 05 section 10's
    /// `connector_unavailable` is a Profile problem the user can fix.
    pub indexed: Vec<(String, IndexedConfig)>,
    pub embedder: Option<Arc<dyn Embedder>>,
}

impl RetrieverSet {
    pub fn is_empty(&self) -> bool {
        self.indexed.is_empty()
    }

    /// Whether this profile has told the retriever where to read from.
    ///
    /// Doc 05 section 10 separates "not configured" from "configured and
    /// empty": the first is a Profile problem the user can fix, and the Profile
    /// page is where they see which it is.
    pub fn configured(&self, retriever_id: &str) -> bool {
        self.config(retriever_id).is_some()
    }

    fn config(&self, retriever_id: &str) -> Option<&IndexedConfig> {
        self.indexed
            .iter()
            .find(|(id, _)| id == retriever_id)
            .map(|(_, c)| c)
    }
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

    set.indexed
        .iter()
        .map(|(id, _)| Assignment {
            retriever_id: id.clone(),
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
) -> FanOut {
    let mut out = FanOut {
        passages: Vec::new(),
        builds_on: Vec::new(),
        caveats: Vec::new(),
        assignments_run: 0,
        assignments_failed: 0,
    };
    if set.is_empty() {
        return out;
    }

    let hooks = HookSet::retriever_defaults();
    let denied_domains: Vec<String> = doctrine["denied_domains"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    for assignment in assignments(plan, question, set) {
        let Some(config) = set.config(&assignment.retriever_id) else {
            // Doc 05 section 10 `connector_unavailable`: coverage none, the
            // other retrievers carry on, and the user is told in the Profile
            // rather than in the card.
            continue;
        };

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
        let packet = build_packet(&assignment, doctrine, must_exclude, at);
        let _ = repo::start_retrieval(store, here, &assignment.query);

        let result = indexed::retrieve(store.conn(), config, &packet, set.embedder.as_deref());
        ledger.release_retriever_slot();

        let retrieved = match result {
            Ok(r) => r,
            Err(_) => {
                // Tolerant: this assignment produced nothing and the card
                // continues on whatever the others found.
                out.assignments_failed += 1;
                continue;
            }
        };

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

fn build_packet(
    assignment: &Assignment,
    doctrine: &Value,
    must_exclude: &[String],
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
            embedder: None,
        };
        let out = assignments(Some(&json!({ "sub_questions": [] })), "q", &set);
        assert_eq!(out.len(), 1, "an empty plan silenced retrieval");
    }
}
