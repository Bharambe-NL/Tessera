//! The eval runner. Doc 02 section 10.1.
//!
//! "The harness starts the real pipeline with `provenance.source: test`, points
//! the retrievers at the synthetic corpus, loads the `finance-eu-synthetic`
//! doctrine pack, and submits questions. It records every Run, Step, and Event
//! exactly as production would."
//!
//! The real pipeline is the point. A harness that reimplemented the run would
//! measure the reimplementation, so this links the core and calls the same
//! entry the shell does. Retrievers arrive at M6; until then a question runs
//! through Router, Synthesizer, Visualizer and Verifier with no passages, and
//! the numbers say so honestly rather than being withheld.
//!
//! Scoring lives in the generator, in Python, because the matchers that decide
//! whether an answer states a fact have to be the same ones the corpus was
//! verified with (doc 02 section 11). This binary produces the record; `gen
//! score` turns it into metrics.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use tessera_core::retrieval::RetrieverSet;
use tessera_providers::CompletionRequest;
use tessera_retrievers::IndexedConfig;
use tessera_retrievers::embed::Embedder;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessera_core::Core;
use tessera_providers::{
    AnthropicProvider, KeyStore, MockProvider, MockResponse, ModelPolicy, ModelProvider,
    OpenAiCompatProvider, OsKeychain, endpoint_for,
};
use tessera_store::Source;

#[derive(Parser, Debug)]
#[command(
    name = "tessera-eval",
    about = "Run the synthetic question set through the real pipeline"
)]
struct Args {
    /// The corpus root, for example eval/synthetic/42.
    #[arg(long)]
    corpus: PathBuf,

    /// Where to write the run record.
    #[arg(long, default_value = "eval/results")]
    out: PathBuf,

    /// Stop after this many questions. The whole set costs real money against a
    /// real provider, so a smoke run is the default way in.
    #[arg(long)]
    limit: Option<usize>,

    /// Use the deterministic mock instead of a provider. Doc 12 phase 3's
    /// acceptance runs this way: end to end, every metric 0 or n/a.
    #[arg(long)]
    mock: bool,

    /// A label for the results directory, naming the model policy under test.
    #[arg(long, default_value = "mock")]
    policy: String,

    /// Leave the retrievers unconfigured, which is what every run before M6
    /// did. Kept so a run can be compared against those, and so a failure in
    /// indexing can be isolated from a failure in retrieval.
    #[arg(long)]
    no_retrievers: bool,

    /// With `--mock`, answer from the retrieved passages rather than returning
    /// garbage. Measures retrieval end to end for nothing; measures nothing
    /// about model quality.
    #[arg(long)]
    grounded: bool,

    /// Run the breadth set (questions_breadth.jsonl) instead of the corpus
    /// question set. BN-036: these are pack independent questions with stakes
    /// ground truth, so this switches the default pack to `general` too, since
    /// no doctrine governs paracetamol; an explicit --pack still wins.
    #[arg(long)]
    breadth: bool,

    /// Which snapshot's labels the questions are scored against.
    #[arg(long, default_value = "T1")]
    snapshot: String,

    /// Doc 02 section 10.1 loads `finance-eu-synthetic`: the same rules as the
    /// shipped finance pack with the synthetic issuers substituted in. Breadth
    /// runs default to `general` instead, because nothing in that set is
    /// governed by finance doctrine.
    #[arg(long)]
    pack: Option<String>,

    /// The provider carrying the bulk of the sweep. A 400 question set is a real
    /// cost against a frontier model, so the default is to send most of it
    /// somewhere cheaper and keep a reference sample on the expensive one.
    #[arg(long, default_value = "moonshot")]
    bulk_provider: String,

    /// The keychain entry for the bulk provider.
    #[arg(long, default_value = "moonshot-default")]
    bulk_key_ref: String,

    /// Model ids for the bulk provider's three tiers.
    ///
    /// These defaults came from `tessera-keys check moonshot-default` against a
    /// real account, not from a guess. Run it again if the provider's catalogue
    /// changes; a model id that no longer exists returns a 404 that reads like
    /// an outage.
    #[arg(long, default_value = "kimi-k2.6")]
    bulk_small: String,
    #[arg(long, default_value = "kimi-k2.6")]
    bulk_medium: String,
    #[arg(long, default_value = "kimi-k3")]
    bulk_frontier: String,

    /// How many questions of each depth also go to the reference provider.
    ///
    /// The point is not coverage, it is calibration: a handful of questions
    /// answered on both is what turns "Kimi scored X" into "Kimi scored X where
    /// Anthropic scored Y on the same questions".
    #[arg(long, default_value_t = 3)]
    sample_per_depth: usize,

    /// The reference provider's keychain entry.
    #[arg(long, default_value = "anthropic-default")]
    reference_key_ref: String,

    /// How many questions to run at once.
    ///
    /// Doc 10 section 6 caps a profile at three runs in flight, and the eval
    /// respects it: a sweep that overran the product's own limit would measure
    /// a configuration the product never runs in. Each worker gets its own
    /// throwaway profile, so nothing shares a store.
    #[arg(long, default_value_t = 3)]
    workers: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Question {
    q_id: String,
    text: String,
    domain: String,
    depth_expected: String,
    #[serde(default)]
    audience_id: Option<String>,
    #[serde(default)]
    required_facts: Vec<String>,
    #[serde(default)]
    required_sources: Vec<String>,
    #[serde(default)]
    forbidden_facts: Vec<String>,
    expected_visual: String,
    #[serde(default)]
    expected_flags: Vec<String>,
    #[serde(default)]
    edge_case_ids: Vec<String>,
    #[serde(default)]
    parent_q_id: Option<String>,
    #[serde(default)]
    anchor_text: Option<String>,
}

/// One question's run, in the shape `gen score` reads.
#[derive(Debug, Serialize)]
struct RunRecord {
    q_id: String,
    text: String,
    domain: String,
    depth_expected: String,
    depth_chosen: Option<String>,
    audience_id: Option<String>,
    required_facts: Vec<String>,
    required_sources: Vec<String>,
    forbidden_facts: Vec<String>,
    expected_visual: String,
    expected_flags: Vec<String>,
    edge_case_ids: Vec<String>,
    parent_q_id: Option<String>,
    anchor_text: Option<String>,

    // What the pipeline produced.
    ok: bool,
    failure: Option<String>,
    card_id: Option<String>,
    /// Which board this ran on. A follow-up shares its parent's board, and the
    /// scorer needs that to tell a real ancestor chain from a coincidence.
    board_id: Option<String>,
    answer: Option<String>,
    findings: Vec<String>,
    visual_type: Option<String>,
    visual_labels: Vec<String>,
    block_index: Vec<Value>,
    citations: Vec<Value>,
    flags: Vec<Value>,
    /// Doc 05 section 8.5. The prior cards the boards retriever recalled, as
    /// "board_id/card_id". Empty until M6 builds it, and the manifest's
    /// `memory_enabled` is what tells the scorer which of those it is.
    prior_cards: Vec<String>,
    /// The Planner's full output, read back from its Step. The card.planned.v1
    /// event carries only the summary doc 04 section 7 declares, and doc 04
    /// section 12 scores fields the summary leaves out: must_exclude, filters,
    /// the assignment per sub-question.
    plan: Option<Value>,
    status: Option<String>,
    confidence: Option<f64>,
    /// Every event the run emitted, so the scorer can check what happened rather
    /// than only what was returned.
    events: Vec<Value>,
    cost: Value,
    latency_ms: u128,
    /// Which provider answered. Doc 02 section 10.1 records the policy under
    /// test with the results; on a split run that has to be per question, or
    /// every metric silently averages two different models.
    provider: String,
    /// `reference` or `bulk`.
    leg: String,
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();

    let questions_file = if args.breadth { "questions_breadth.jsonl" } else { "questions.jsonl" };
    let pack = args
        .pack
        .clone()
        .unwrap_or_else(|| if args.breadth { "general" } else { "finance-eu-synthetic" }.to_string());
    let questions = match load_questions(&args.corpus.join(questions_file)) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("could not read the question set: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let total = args.limit.unwrap_or(questions.len()).min(questions.len());
    println!(
        "running {total} of {} questions against the {} provider",
        questions.len(),
        if args.mock {
            "mock"
        } else {
            args.bulk_provider.as_str()
        }
    );

    let plan = match build_plan(&args, &questions, total) {
        Ok(p) => p,
        Err(message) => {
            eprintln!("{message}");
            return std::process::ExitCode::from(2);
        }
    };
    println!("{}", plan.describe());

    let workers = args.workers.max(1).min(total.max(1));
    println!("{workers} workers");

    // The queue hands out families, not questions.
    //
    // Each worker owns its own store, so a follow-up run by a different worker
    // than its parent would look for an ancestor that does not exist there. A
    // family is a root and everything descending from it, and it stays whole on
    // one worker in ancestor-first order. Parallelism is unaffected: there are
    // as many families as root questions.
    let queue = Arc::new(Mutex::new(families(&questions, total)));
    let done = Arc::new(AtomicUsize::new(0));
    let collected: Arc<Mutex<Vec<RunRecord>>> = Arc::new(Mutex::new(Vec::with_capacity(total)));
    let plan = Arc::new(plan);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let done = Arc::clone(&done);
            let collected = Arc::clone(&collected);
            let plan = Arc::clone(&plan);
            let args = &args;
            let pack = &pack;

            scope.spawn(move || {
                // Each worker gets its own profile. Doc 10 section 6's ledger is
                // per profile, and sharing one store would have the workers
                // contend on the very lock the limit exists to avoid.
                let keys: Box<dyn KeyStore> = if args.mock {
                    Box::new(tessera_providers::MemoryKeyStore::with("test-key", "sk-test"))
                } else {
                    Box::new(OsKeychain)
                };
                let first_key_ref = if args.mock {
                    "test-key"
                } else {
                    args.bulk_key_ref.as_str()
                };

                let mut core =
                    match Core::in_memory_with_keys(Arc::clone(&plan.bulk.provider), keys, first_key_ref) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("a worker could not bring a core up: {e}");
                            return;
                        }
                    };
                // Doc 02 section 10.1 and doc 10 section 5: test provenance, so
                // nothing here trips a policy hook meant for live work.
                core.source = Source::Test;
                if let Err(e) = core.use_pack(pack) {
                    eprintln!("a worker could not load the `{pack}` pack: {e}");
                    return;
                }

                // Doc 02 section 10.1 fixes the roots: "local folder retriever
                // at `corpus/internal`, regulatory retriever at
                // `corpus/regulatory`, web retriever at the local static
                // server". Until this was wired the eval measured a pipeline
                // with no retrievers and reported n/a for the half of the
                // product that had just been built.
                if !args.no_retrievers && let Err(e) = configure_retrievers(&mut core, &args.corpus) {
                    eprintln!("a worker could not index the corpus: {e}");
                    return;
                }

                let mut current = String::new();
                let mut local_failures = 0usize;

                while let Some(family) = queue.lock().ok().and_then(|mut q| q.pop_front()) {
                    // Where each question in this family was answered, so its
                    // children can be asked on the same board as follow-ups.
                    let mut answered: HashMap<String, Answered> = HashMap::new();

                    for q in &family {
                        let on_reference = plan.reference_ids.contains(&q.q_id);
                        let leg = if on_reference { &plan.reference } else { &plan.bulk };
                        if leg.name != current {
                            core.use_provider(Arc::clone(&leg.provider), leg.policy.clone());
                            current = leg.name.clone();
                        }

                        let parent = q.parent_q_id.as_deref().and_then(|p| answered.get(p));
                        let mut record = run_one(&mut core, q, parent.cloned().as_ref(), &mut local_failures);
                        record.provider = leg.name.clone();
                        record.leg = if on_reference { "reference" } else { "bulk" }.to_string();

                        if let (Some(card_id), Some(board_id)) =
                            (record.card_id.clone(), record.board_id.clone())
                        {
                            answered.insert(q.q_id.clone(), Answered { board_id, card_id });
                        }

                        let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if finished.is_multiple_of(10) || finished == total {
                            println!("  {finished}/{total}");
                        }
                        if let Ok(mut out) = collected.lock() {
                            out.push(record);
                        }
                    }
                }
            });
        }
    });

    let mut records = collected
        .lock()
        .map(|mut r| std::mem::take(&mut *r))
        .unwrap_or_default();
    // Question order, so two runs of one corpus produce comparable files however
    // the workers happened to interleave.
    records.sort_by(|a, b| a.q_id.cmp(&b.q_id));
    let failures = records.iter().filter(|r| !r.ok).count();

    let dir = args
        .out
        .join(corpus_name(&args.corpus))
        .join(&args.policy)
        .join(stamp());
    if let Err(e) = write_records(&dir, &records, &args, &pack, total, failures) {
        eprintln!("could not write the results: {e}");
        return std::process::ExitCode::from(2);
    }

    println!("\n{total} questions, {failures} failed to produce a card");
    println!("wrote {}", dir.display());
    println!("score it with: gen score --results {}", dir.display());
    std::process::ExitCode::SUCCESS
}

/// Group questions into families, each a root followed by its descendants.
///
/// Ancestor-first within a family, so a follow-up is never asked before the
/// card it follows exists. A question whose parent is not in the set becomes a
/// root of its own rather than being dropped: the corpus is allowed to grow a
/// branch whose parent was filtered out by `--limit`, and losing the question
/// silently would shrink the denominator of every metric without saying so.
fn families(questions: &[Question], total: usize) -> VecDeque<Vec<Question>> {
    let taken: Vec<&Question> = questions.iter().take(total).collect();
    let present: HashSet<&str> = taken.iter().map(|q| q.q_id.as_str()).collect();

    let mut children: HashMap<&str, Vec<&Question>> = HashMap::new();
    let mut roots: Vec<&Question> = Vec::new();
    for q in &taken {
        match q.parent_q_id.as_deref() {
            Some(parent) if present.contains(parent) => {
                children.entry(parent).or_default().push(q);
            }
            _ => roots.push(q),
        }
    }

    let mut out = VecDeque::with_capacity(roots.len());
    for root in roots {
        let mut family = Vec::new();
        let mut frontier = vec![root];
        // A node is appended before its children are explored, so the family
        // comes out ancestor first however deep it goes. A cycle in
        // `parent_q_id` would loop here, and the visited set is what stops a
        // malformed corpus from hanging the sweep.
        let mut visited = HashSet::new();
        while let Some(q) = frontier.pop() {
            if !visited.insert(q.q_id.as_str()) {
                continue;
            }
            family.push((*q).clone());
            if let Some(kids) = children.get(q.q_id.as_str()) {
                frontier.extend(kids.iter().copied());
            }
        }
        out.push_back(family);
    }
    out
}

/// Where a question's parent was answered, if it had one.
///
/// Half the corpus is follow-ups, and a follow-up asked on a board of its own
/// is a question with no subject: "which article says so?" names nothing to
/// retrieve. Measured that way, retrieval recall on standalone questions was
/// 1.000 and on follow-ups 0.485, and every point of that gap was the harness
/// asking the question wrong rather than the retriever answering it wrong.
#[derive(Clone)]
struct Answered {
    board_id: String,
    card_id: String,
}

fn run_one(
    core: &mut Core,
    q: &Question,
    parent: Option<&Answered>,
    failures: &mut usize,
) -> RunRecord {
    let started = std::time::Instant::now();

    // A follow-up belongs on its parent's board, which is what makes the
    // ancestor chain walkable. A root question gets a board of its own.
    //
    // Always `fast`, never the expected depth. Seeding the board default with
    // the label hands the Router part of the answer, because the default is the
    // baseline its recommendation starts from: every earlier sweep's route
    // accuracy was measured with that leak (BN-036), so those numbers are not
    // comparable with what this measures.
    let board_id = match parent {
        Some(p) => p.board_id.clone(),
        None => match core.create_board(&q.text, "fast") {
            Ok(id) => id,
            Err(e) => {
                *failures += 1;
                return empty_record(q, format!("could not create a board: {e}"), started);
            }
        },
    };

    let outcome = core.ask_on(
        &board_id,
        &q.text,
        Some(&q.depth_expected),
        parent.map(|p| p.card_id.as_str()),
    );

    let plan: Option<Value> = core
        .store
        .conn()
        .query_row(
            "SELECT s.output FROM step s
             JOIN run r ON r.id = s.run_id
             WHERE r.board_id = ?1 AND s.agent_id = 'planner' AND s.output IS NOT NULL
             ORDER BY s.started_at DESC LIMIT 1",
            [&board_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());

    // Only this card's events. A follow-up shares its parent's board, so an
    // unfiltered read would hand the scorer the parent's flags and citations as
    // though this card had raised them.
    let this_card = outcome.as_ref().ok().map(|o| o.card_id.clone());
    let events: Vec<Value> = core
        .store
        .events(Some(&board_id))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| match (&this_card, &e.card_id) {
            (Some(want), Some(got)) => want == got,
            // A board level event belongs to whichever card provoked it, and
            // for a root question that is this one.
            (_, None) => parent.is_none(),
            _ => true,
        })
        .map(|e| {
            json!({
                "type": e.event_type,
                "payload": e.payload,
                "actor": e.provenance.emitter_id,
                "source": e.provenance.source,
            })
        })
        .collect();

    let mut record = match outcome {
        Ok(o) => {
            let board = tessera_store::repo::read_board(&core.store, &board_id)
                .ok()
                .flatten();
            // By id, not the board's first card: a follow-up sits on its
            // parent's board, and `first()` would report the parent's answer as
            // this question's.
            let card = board.as_ref().and_then(|b| b.cards.iter().find(|c| c.id == o.card_id));

            RunRecord {
                ok: true,
                failure: None,
                card_id: Some(o.card_id),
                answer: card.and_then(|c| c.answer.clone()),
                findings: card
                    .map(|c| {
                        c.findings
                            .iter()
                            .filter_map(|f| f["text"].as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                visual_type: card
                    .and_then(|c| c.visual.as_ref())
                    .and_then(|v| v["type"].as_str().map(str::to_string)),
                visual_labels: card
                    .and_then(|c| c.visual.as_ref())
                    .and_then(|v| v["block_index"].as_array().cloned())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|b| b["label"].as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                block_index: card
                    .and_then(|c| c.visual.as_ref())
                    .and_then(|v| v["block_index"].as_array().cloned())
                    .unwrap_or_default(),
                citations: card.map(|c| c.citations.clone()).unwrap_or_default(),
                prior_cards: Vec::new(),
                plan: plan.clone(),
                flags: card.map(|c| c.flags.clone()).unwrap_or_default(),
                status: Some(o.status),
                confidence: Some(o.confidence),
                depth_chosen: card.map(|c| c.depth.clone()),
                ..empty_record(q, String::new(), started)
            }
        }
        Err(e) => {
            *failures += 1;
            RunRecord {
                failure: Some(e.to_string()),
                ..empty_record(q, e.to_string(), started)
            }
        }
    };

    record.board_id = Some(board_id.clone());
    record.cost = run_cost(core, &board_id);
    record.events = events;
    record.latency_ms = started.elapsed().as_millis();
    record
}

fn empty_record(q: &Question, failure: String, started: std::time::Instant) -> RunRecord {
    RunRecord {
        q_id: q.q_id.clone(),
        text: q.text.clone(),
        domain: q.domain.clone(),
        depth_expected: q.depth_expected.clone(),
        depth_chosen: None,
        audience_id: q.audience_id.clone(),
        required_facts: q.required_facts.clone(),
        required_sources: q.required_sources.clone(),
        forbidden_facts: q.forbidden_facts.clone(),
        expected_visual: q.expected_visual.clone(),
        expected_flags: q.expected_flags.clone(),
        edge_case_ids: q.edge_case_ids.clone(),
        parent_q_id: q.parent_q_id.clone(),
        anchor_text: q.anchor_text.clone(),
        ok: false,
        failure: if failure.is_empty() { None } else { Some(failure) },
        card_id: None,
        board_id: None,
        answer: None,
        findings: Vec::new(),
        visual_type: None,
        visual_labels: Vec::new(),
        block_index: Vec::new(),
        citations: Vec::new(),
        prior_cards: Vec::new(),
        plan: None,
        flags: Vec::new(),
        status: None,
        confidence: None,
        events: Vec::new(),
        cost: Value::Null,
        latency_ms: started.elapsed().as_millis(),
        provider: String::new(),
        leg: String::new(),
    }
}

/// One provider and the policy naming its models.
struct Leg {
    name: String,
    provider: Arc<dyn ModelProvider>,
    policy: ModelPolicy,
}

/// Which questions go where.
struct Plan {
    bulk: Leg,
    reference: Leg,
    /// The questions the reference provider answers as well.
    reference_ids: std::collections::BTreeSet<String>,
}

impl Plan {
    fn describe(&self) -> String {
        if self.reference_ids.is_empty() {
            return format!("every question on {}", self.bulk.name);
        }
        format!(
            "{} questions on {} as a reference sample, the rest on {}",
            self.reference_ids.len(),
            self.reference.name,
            self.bulk.name
        )
    }
}

/// A mock that answers from the passages it was handed.
///
/// The garbage mock proves the pipeline fails closed, which is worth proving
/// and is all it proves: no card is produced, so nothing downstream of routing
/// is exercised and every metric reports n/a. That left the whole retrieval
/// half of the product measurable only by spending money.
///
/// This one reads the passages out of the prompt and writes an answer that
/// quotes them. It invents nothing: if retrieval found the fact, the answer
/// states it, and if it did not, the answer says so. So `fact_recall` measured
/// this way is a measurement of retrieval rather than of a model, which is
/// exactly the thing that could not be measured before, and it costs nothing.
///
/// What it cannot measure is anything about model quality: phrasing, judgment,
/// conflict resolution, or whether a real model would have cited the passage it
/// was given. Those need a real provider and a real sweep.
fn grounded_mock() -> Arc<dyn ModelProvider> {
    let provider = MockProvider::new()
        .with_default(MockResponse::Scripted(Arc::new(|request| {
            match request.stage.as_str() {
                "route" => MockResponse::Json(routed()),
                "plan" => MockResponse::Json(planned(request)),
                "synthesize" => MockResponse::Json(synthesized(request)),
                "visualize" => MockResponse::Json(visualised(request)),
                // Anything else is not scripted, and failing closed is the
                // right default for a stage nobody thought about.
                _ => MockResponse::Garbage,
            }
        })));
    Arc::new(provider)
}

fn routed() -> Value {
    json!({
        "classification": {
            "question_type": "factual",
            "regulatory_stakes": true,
            "audience_id": null,
            "language": "en",
            "needs_current_information": false,
            "needs_internal_documents": true,
            "needs_structured_data": false,
            "entities": [],
            "is_follow_up_of_context": false
        }
    })
}

fn planned(request: &CompletionRequest) -> Value {
    // One sub-question. The Planner's own deterministic half assigns the
    // retrievers, so the plan only has to be well formed and retrievable.
    //
    // Retrievable is the part that takes work. Half the corpus is follow-ups,
    // and "which article says so?" names nothing to look for, so a plan that
    // repeats the request back sends the retriever a query with no subject.
    // Prefixing the nearest ancestor question is the crudest resolution there
    // is: it invents nothing, and it puts the subject into the query the way a
    // real Planner would while writing something far better. What it measures
    // is therefore a floor on retrieval, not a ceiling.
    let prompt = prompt_of(request);
    let text = match ancestor_question(&prompt) {
        Some(parent) => format!("{parent} {}", first_line(&prompt)),
        None => first_line(&prompt),
    };
    json!({
        "sub_questions": [{
            "sq_id": "sq-1",
            "text": text,
            "purpose": "answer the question as asked",
            "priority": 1
        }],
        "scope_limits": []
    })
}

/// The nearest ancestor question the Planner's prompt was given, if any.
fn ancestor_question(prompt: &str) -> Option<String> {
    prompt
        .lines()
        .find(|l| l.starts_with("Ancestor question: "))
        .map(|l| l.trim_start_matches("Ancestor question: ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Pull the passages back out of the prompt the Synthesizer built.
///
/// `prompts::passage_block` fences each one as `<passage n="1" ...>text</passage>`,
/// and that fence exists so the model can tell quoted data from instruction. It
/// serves here for the same reason: it is the one part of the prompt whose
/// shape is guaranteed.
fn passages_in(prompt: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut rest = prompt;
    while let Some(start) = rest.find("<passage n=\"") {
        let after = &rest[start + "<passage n=\"".len()..];
        let Some(quote) = after.find('"') else { break };
        let Ok(ordinal) = after[..quote].parse::<usize>() else {
            rest = after;
            continue;
        };
        let Some(open_end) = after.find('>') else { break };
        let body = &after[open_end + 1..];
        let Some(close) = body.find("</passage>") else { break };
        out.push((ordinal, body[..close].trim().to_string()));
        rest = &body[close..];
    }
    out
}

fn synthesized(request: &CompletionRequest) -> Value {
    let prompt = prompt_of(request);
    let passages = passages_in(&prompt);

    if passages.is_empty() {
        // Doc 06 section A10. The honest answer, and the same one the real
        // Synthesizer gives, so the no-sources path stays measured.
        return json!({
            "answer": "No sources were found for this question.",
            "findings": [],
            "citations": [],
            "structured_summary": { "values": [], "steps": [], "groups": [], "relations": [] },
            "scope_statement": "No sources found for this question.",
            "confidence": 0.0,
            "caveats": []
        });
    }

    // Quote each passage behind its own marker. This is what makes the answer
    // carry whatever retrieval found: if the figure is in a passage it is in
    // the answer, and if it is not, no amount of phrasing puts it there.
    let mut answer = String::new();
    let mut citations = Vec::new();
    let mut findings = Vec::new();
    for (ordinal, text) in passages.iter().take(6) {
        let sentence = text.split_whitespace().collect::<Vec<_>>().join(" ");
        answer.push_str(&sentence);
        answer.push_str(&format!(" [{ordinal}]"));
        answer.push(' ');
        citations.push(json!({ "ordinal": ordinal, "binding": "answer" }));
        if findings.len() < 3 {
            findings.push(json!({
                "text": sentence.chars().take(200).collect::<String>(),
                "citations": [ordinal]
            }));
        }
    }

    json!({
        "answer": answer.trim(),
        "findings": findings,
        "citations": citations,
        "structured_summary": {
            "values": [],
            "steps": [],
            "groups": [],
            "relations": []
        },
        "scope_statement": "Answered from the retrieved passages.",
        "confidence": 0.6,
        "caveats": []
    })
}

fn visualised(_request: &CompletionRequest) -> Value {
    // An empty summary declines a visual, which doc 06 section B10 allows and
    // which keeps the visual metrics honest: this mock has no structure to
    // render, so claiming one would be inventing a number.
    json!({
        "declined": true,
        "reason": "no_structure",
        "visual": null
    })
}

fn prompt_of(request: &CompletionRequest) -> String {
    let mut text = request.system.clone().unwrap_or_default();
    for message in &request.messages {
        for block in &message.content {
            if let tessera_providers::ContentBlock::Text { text: t } = block {
                text.push('\n');
                text.push_str(t);
            }
        }
    }
    text
}

fn first_line(prompt: &str) -> String {
    prompt
        .lines()
        .find(|l| l.starts_with("Request: "))
        .map(|l| l.trim_start_matches("Request: ").to_string())
        .unwrap_or_else(|| "the question".to_string())
}

fn build_plan(args: &Args, questions: &[Question], total: usize) -> Result<Plan, String> {
    if args.mock {
        // Garbage by default, which proves the pipeline fails closed at scale.
        // `--grounded` swaps in a mock that answers from its passages, which is
        // the only free way to measure whether retrieval found anything.
        let mock: Arc<dyn ModelProvider> = if args.grounded {
            grounded_mock()
        } else {
            Arc::new(MockProvider::new().with_default(MockResponse::Garbage))
        };
        let leg = |provider: Arc<dyn ModelProvider>| Leg {
            name: "mock".to_string(),
            provider,
            policy: ModelPolicy::default_anthropic("test-key"),
        };
        return Ok(Plan {
            bulk: leg(Arc::clone(&mock)),
            reference: leg(mock),
            reference_ids: Default::default(),
        });
    }

    let keys = OsKeychain;

    // Fail before spending anything. Doc 03 section 8.3's posture: a missing key
    // stops the run rather than being discovered halfway through it.
    let bulk_secret = keys.get(&args.bulk_key_ref).map_err(|_| {
        format!(
            "No key stored under `{}`. Set one with: tessera-keys set {}",
            args.bulk_key_ref, args.bulk_key_ref
        )
    })?;

    let endpoint = endpoint_for(&args.bulk_provider)
        .ok_or_else(|| format!("No adapter for `{}`.", args.bulk_provider))?;
    let bulk = Leg {
        name: args.bulk_provider.clone(),
        provider: Arc::new(
            OpenAiCompatProvider::new(endpoint, bulk_secret)
                .map_err(|e| format!("Could not build the {} client: {e}", args.bulk_provider))?,
        ),
        policy: ModelPolicy::single_provider(
            &args.bulk_provider,
            &args.bulk_key_ref,
            &args.bulk_small,
            &args.bulk_medium,
            &args.bulk_frontier,
        ),
    };

    if args.sample_per_depth == 0 {
        return Ok(Plan {
            reference: Leg {
                name: bulk.name.clone(),
                provider: Arc::clone(&bulk.provider),
                policy: bulk.policy.clone(),
            },
            bulk,
            reference_ids: Default::default(),
        });
    }

    let reference_secret = keys.get(&args.reference_key_ref).map_err(|_| {
        format!(
            "No key stored under `{}`. Set one with: tessera-keys set {}",
            args.reference_key_ref, args.reference_key_ref
        )
    })?;
    let reference = Leg {
        name: "anthropic".to_string(),
        provider: Arc::new(
            AnthropicProvider::new(reference_secret)
                .map_err(|e| format!("Could not build the Anthropic client: {e}"))?,
        ),
        policy: ModelPolicy::default_anthropic(&args.reference_key_ref),
    };

    // The first N of each depth, in question order, so the sample is the same
    // set on every run and two runs stay comparable.
    let mut reference_ids = std::collections::BTreeSet::new();
    for depth in ["fast", "deep", "research"] {
        for q in questions
            .iter()
            .take(total)
            .filter(|q| q.depth_expected == depth)
            .take(args.sample_per_depth)
        {
            reference_ids.insert(q.q_id.clone());
        }
    }

    Ok(Plan {
        bulk,
        reference,
        reference_ids,
    })
}

/// Doc 02 section 10.2's cost and latency, read off the Run the way the
/// Profile's spend page does.
fn run_cost(core: &Core, board_id: &str) -> Value {
    core.store
        .conn()
        .query_row(
            "SELECT cost FROM run WHERE board_id = ?1 ORDER BY started_at DESC LIMIT 1",
            rusqlite::params![board_id],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null)
}

fn load_questions(path: &Path) -> std::io::Result<Vec<Question>> {
    let body = std::fs::read_to_string(path)?;
    Ok(body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

fn corpus_name(corpus: &Path) -> String {
    corpus
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// A sortable directory name. Doc 02 section 10.4 keeps one directory per run.
fn stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{secs}")
}

/// The union of the pack's per retriever exclusions, doc 04 section 4's
/// `doctrine.must_exclude`.
fn doctrine_must_exclude(pack_code: &str) -> Vec<String> {
    let Ok(registry) = tessera_schema::Registry::load() else {
        return Vec::new();
    };
    let Ok(packs) = tessera_doctrine::PackLibrary::load_built_in(&registry) else {
        return Vec::new();
    };
    let Ok(pack) = packs.get(pack_code) else {
        return Vec::new();
    };
    let mut out: Vec<String> = pack
        .retrievers
        .iter()
        .flat_map(|r| r.must_exclude.iter().cloned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Point this core's retrievers at the synthetic corpus.
///
/// Doc 02 section 10.1: "points the retrievers at the synthetic corpus (local
/// folder retriever at `corpus/internal`, regulatory retriever at
/// `corpus/regulatory`, web retriever at the local static server)".
///
/// The web tree is indexed from disk rather than fetched from `gen serve`,
/// because what the gate measures is extraction and ranking and doc 02 section
/// 7 says as much: "the synthetic web is served locally, so this measures
/// extraction and ranking". Fetching localhost to read files that are already
/// on disk would add a moving part and measure the same thing.
///
/// The Sensitive folder is excluded here rather than filtered later. Doc 05
/// section 8.2 is exact: "Excluded folders are never opened." A folder that is
/// read and then discarded has already been read.
fn configure_retrievers(core: &mut Core, corpus: &Path) -> Result<(), String> {
    let profile_id = core.profile_id.clone();
    let exclude = doctrine_exclusions(core);
    let mut embedder: Option<Arc<dyn Embedder>> = None;

    // The embedder is optional and its absence is reported rather than fatal.
    // A machine without the model can still measure the lexical half, which is
    // a real number, and failing the whole sweep over a download would be a
    // poor trade.
    match tessera_retrievers::embed::LocalEmbedder::multilingual() {
        Ok(e) => embedder = Some(Arc::new(e)),
        Err(e) => eprintln!("no embedding model ({e}); indexing the lexical half only"),
    }

    let roots = [
        ("regulatory", "Central Authority for Prudential Oversight", corpus.join("corpus/regulatory")),
        ("local", "Internal documents", corpus.join("corpus/internal")),
        ("web", "The synthetic web", corpus.join("corpus/web")),
    ];

    let mut configured: Vec<(String, IndexedConfig)> = Vec::new();
    for (id, label, root) in roots {
        if !root.is_dir() {
            continue;
        }
        let report = tessera_retrievers::index_folder(
            core.store.conn(),
            &profile_id,
            id,
            label,
            &root,
            &exclude,
            embedder.as_deref(),
        )
        .map_err(|e| format!("{id}: {e}"))?;

        // Doc 05 section 11 puts parse errors on the Profile's Retrievers
        // page. Here they go to stderr, because a run that quietly indexed
        // ninety of a hundred documents would produce a recall number nobody
        // could explain.
        if !report.errors.is_empty() {
            eprintln!(
                "  {id}: {} indexed, {} excluded, {} unreadable ({})",
                report.indexed,
                report.excluded,
                report.errors.len(),
                report
                    .errors
                    .iter()
                    .map(|(p, k, _)| format!("{p} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        let config = match id {
            "regulatory" => IndexedConfig::regulatory(id),
            _ => IndexedConfig::local(vec![id.to_string()]),
        };
        configured.push((id.to_string(), config));
    }

    // Doc 05 section 8.5: memory is a retriever like any other, over an index
    // that fills as cards are answered rather than from disk.
    tessera_retrievers::boards::ensure_folder(core.store.conn(), &profile_id)
        .map_err(|e| format!("boards: {e}"))?;
    configured.push(("boards".to_string(), IndexedConfig::boards()));

    core.retrievers = RetrieverSet { indexed: configured, embedder };
    Ok(())
}

/// The folder names doctrine says never to open.
fn doctrine_exclusions(core: &Core) -> Vec<String> {
    core.packs
        .get(&core.pack_code)
        .map(|p| p.must_exclude())
        .unwrap_or_default()
}

fn write_records(
    dir: &Path,
    records: &[RunRecord],
    args: &Args,
    pack: &str,
    total: usize,
    failures: usize,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;

    let mut file = std::fs::File::create(dir.join("runs.jsonl"))?;
    for r in records {
        writeln!(file, "{}", serde_json::to_string(r).unwrap_or_default())?;
    }

    let manifest = json!({
        "corpus": corpus_name(&args.corpus),
        "policy": args.policy,
        "pack": pack,
        "snapshot": args.snapshot,
        "provider": if args.mock { "mock" } else { args.bulk_provider.as_str() },
        "bulk_provider": args.bulk_provider,
        "bulk_models": {
            "small": args.bulk_small,
            "medium": args.bulk_medium,
            "frontier": args.bulk_frontier
        },
        "reference_provider": if args.mock { "mock" } else { "anthropic" },
        "sample_per_depth": args.sample_per_depth,
        "questions_run": total,
        "cards_failed": failures,
        // Doc 07 section B9: the support check is not enabled until its
        // agreement is measured, so every verdict in this run is `unchecked`.
        // A scorer that read them as `supported` would report a number the
        // product has not earned.
        "support_check_enabled": false,
        "retrievers_enabled": !args.no_retrievers,
        // Doc 15 section 5's four metrics report n/a until this is true.
        // Reporting a clean zero for own_card sole support while no card has
        // ever been offered a prior card would say the rule holds when nothing
        // has tested it. The boards retriever is configured with the others,
        // so this follows them.
        "memory_enabled": !args.no_retrievers,
        // Doc 04 section 5: the plan's must_exclude may add to the doctrine's
        // list and never remove from it. The scorer needs the floor to check
        // compliance, and the pack is loaded here, not there.
        "doctrine_must_exclude": doctrine_must_exclude(pack),
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )?;
    Ok(())
}
