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

mod boards;
mod bundles;
mod reverify;
mod vault;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tessera_core::retrieval::RetrieverSet;
use tessera_providers::CompletionRequest;
use tessera_retrievers::IndexedConfig;
use tessera_retrievers::embed::Embedder;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessera_core::{Anchor, Core};
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

    /// Re-verify the corpus's own boards instead of asking questions.
    ///
    /// Doc 07 section B3's batch: every card the corpus shipped is read back
    /// against the tree this run points at, and a citation whose source has
    /// since changed, gone, or been superseded flips the card to flagged. This
    /// is what doc 02 section 5.4 means by a board written at T1 and reopened at
    /// T3, and it is what the three staleness gates are scored on.
    ///
    /// Runs on one worker against one store, because every card has to see the
    /// same imported boards.
    #[arg(long)]
    verify_only: bool,

    /// Generate an exercise on each imported board after the sweep. Doc 08.
    ///
    /// The Exercise agent reads cards that exist and never retrieves, so it
    /// costs one call per board and measures for nothing on the grounded mock.
    /// It also writes `exercises.jsonl` beside the runs, which is what the
    /// scorer re-checks doc 08 section 5's two rules against: measuring the
    /// agent's output with the agent's own check would report 1.00 whatever the
    /// check did.
    #[arg(long)]
    exercise: bool,

    /// The corpus as it stood when those boards were written, usually the T1
    /// tree. Without it a document that was quietly edited cannot be told from
    /// one that was not, and the run reports that it could not tell rather than
    /// reporting that nothing changed.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Round trip every corpus board through export and import. Doc 12 phase
    /// 10's acceptance.
    ///
    /// Costs nothing: no provider is called, because a bundle carries what the
    /// board already holds and asks no model anything.
    #[arg(long)]
    bundles: bool,
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
    /// `card` for a question this run asked, `verify_only` for a card it read
    /// back. The scorer keeps the two apart: a re-verification answers nothing,
    /// so counting it among the answers would dilute every recall metric with
    /// rows that were never asked a question.
    kind: String,
    /// The corpus's own name for the card being re-verified, `board_id/card_id`.
    ///
    /// Doc 15's ground truth names prior cards this way. Matching on `card_id`
    /// alone would compare a pipeline ulid against a synthetic card id and
    /// silently never match, which is why the chain is matched on this.
    card_ref: Option<String>,
}

/// One exercise a run generated, with the cards its items may draw from.
///
/// The cards travel with it so the scorer can re-check doc 08 section 5's two
/// rules independently. Measuring the agent's output with the agent's own check
/// would report 1.00 whatever the check did.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExerciseRecord {
    board_id: String,
    exercise_id: Option<String>,
    items: Value,
    cards: Value,
    dropped: usize,
}

/// How many boards one worker generates an exercise on.
///
/// Every board would be a call per board and the sweep has hundreds. Five is
/// enough for the two gates to have a denominator worth reading, and the run
/// says out loud when it sampled rather than covered.
const EXERCISE_BOARDS_PER_WORKER: usize = 5;

/// Read back what the exercise wrote, so the scorer measures the stored rows.
fn exercise_record(core: &Core, board_id: &str, outcome: &tessera_core::ExerciseOutcome) -> ExerciseRecord {
    let items = outcome
        .exercise_id
        .as_deref()
        .and_then(|id| {
            core.store
                .conn()
                .query_row(
                    "SELECT items FROM exercise WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
        })
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(|| json!([]));

    ExerciseRecord {
        board_id: board_id.to_string(),
        exercise_id: outcome.exercise_id.clone(),
        items,
        cards: json!(tessera_store::repo::cards_for_exercise(&core.store, board_id, 8).unwrap_or_default()),
        dropped: outcome.dropped,
    }
}

/// Where this run reads its keys.
///
/// A mock run needs none, so it gets a keystore holding one fake. A live run on
/// a person's machine reads the OS keychain, which is doc 01 section 4.16's rule
/// and the only place a key belongs on a machine that has one.
///
/// A headless runner has no keychain at all: on Linux `keyring` wants a Secret
/// Service over D-Bus and a CI runner has no session for one, so the choice is
/// between reading the environment and having no nightly eval. The environment
/// is chosen only when `TESSERA_CI` says so, rather than by asking whether the
/// keychain happens to answer: a keychain that is merely locked would look the
/// same, and falling back then would train a person to expect a prompt that
/// never comes.
fn keystore(mock: bool) -> Box<dyn KeyStore> {
    if mock {
        return Box::new(tessera_providers::MemoryKeyStore::with("test-key", "sk-test"));
    }
    if std::env::var("TESSERA_CI").is_ok_and(|v| !v.trim().is_empty()) {
        eprintln!("reading keys from the environment, which is what TESSERA_CI asks for");
        return Box::new(tessera_providers::EnvKeyStore);
    }
    Box::new(OsKeychain)
}

/// Doc 12 phase 10's acceptance: every corpus board out and back in.
fn round_trip_bundles(args: &Args) -> std::process::ExitCode {
    let trips = match bundles::run(&args.corpus, &args.snapshot) {
        Ok(t) => t,
        Err(message) => {
            eprintln!("{message}");
            return std::process::ExitCode::from(2);
        }
    };

    println!("{}", bundles::report(&trips));
    let lost: Vec<&bundles::Trip> = trips.iter().filter(|t| !t.whole()).collect();
    let marked = trips.iter().filter(|t| t.marked_for_export).count();
    let collided: usize = trips.iter().map(|t| t.concepts_collided).sum();
    let titles: usize = trips.iter().map(|t| t.pages_collided).sum();
    let pages: usize = trips.iter().map(|t| t.arrived.pages).sum();
    let dropped: usize = trips.iter().map(|t| t.carried_evidence_dropped).sum();

    println!(
        "{} boards round tripped, {marked} of them marked for export by the corpus, \
         {collided} concept {} and {titles} page {} handled",
        trips.len(),
        if collided == 1 {
            "term collision"
        } else {
            "term collisions"
        },
        if titles == 1 {
            "title collision"
        } else {
            "title collisions"
        }
    );
    // Named rather than implied: doc 16 section 2.2 makes carried evidence the
    // reason a page can support a claim at all, so a page that arrives without
    // it arrives weaker than it left.
    println!("{pages} pages carried, {dropped} carried citations dropped for evidence that stayed behind");
    if lost.is_empty() {
        println!("every board arrived whole");
        return std::process::ExitCode::SUCCESS;
    }
    for trip in &lost {
        eprintln!(
            "{} lost rows: {:?} sent, {:?} arrived. {}",
            trip.board_id, trip.sent, trip.arrived, trip.note
        );
    }
    std::process::ExitCode::from(1)
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();

    // Before the question set is read, because a bundle round trip asks no
    // questions and a corpus with no `questions.jsonl` can still ship boards.
    if args.bundles {
        return round_trip_bundles(&args);
    }

    let questions_file = if args.breadth {
        "questions_breadth.jsonl"
    } else {
        "questions.jsonl"
    };
    let pack = args.pack.clone().unwrap_or_else(|| {
        if args.breadth {
            "general"
        } else {
            "finance-eu-synthetic"
        }
        .to_string()
    });
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

    if args.verify_only {
        let mut records = run_verify_only(&args, &pack, &plan, &questions);
        records.sort_by(|a, b| a.q_id.cmp(&b.q_id));
        let failures = records.iter().filter(|r| !r.ok).count();
        let dir = args
            .out
            .join(corpus_name(&args.corpus))
            .join(&args.policy)
            .join(stamp());
        if let Err(e) = write_records(&dir, &records, &[], &[], &args, &pack, records.len(), failures) {
            eprintln!("could not write the results: {e}");
            return std::process::ExitCode::from(2);
        }
        println!("\n{} cards re-verified, {failures} failed", records.len());
        println!("wrote {}", dir.display());
        println!("score it with: gen score --results {}", dir.display());
        return std::process::ExitCode::SUCCESS;
    }

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
    let exercises: Arc<Mutex<Vec<ExerciseRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let plan = Arc::new(plan);

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let done = Arc::clone(&done);
            let collected = Arc::clone(&collected);
            let exercises = Arc::clone(&exercises);
            let plan = Arc::clone(&plan);
            let args = &args;
            let pack = &pack;

            scope.spawn(move || {
                // Each worker gets its own profile. Doc 10 section 6's ledger is
                // per profile, and sharing one store would have the workers
                // contend on the very lock the limit exists to avoid.
                let keys = keystore(args.mock);
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
                if !args.no_retrievers
                    && let Err(e) = configure_retrievers(&mut core, &args.corpus, &args.snapshot)
                {
                    eprintln!("a worker could not index the corpus: {e}");
                    return;
                }

                let mut current = String::new();
                let mut local_failures = 0usize;
                let mut boards_seen: std::collections::BTreeSet<String> = Default::default();

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
                            boards_seen.insert(board_id.clone());
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

                // Doc 08 section 3: on demand from a board. Every board this
                // worker filled is a board with cards worth testing, so this is
                // the widest sample the run can take for free.
                if args.exercise {
                    let taken: Vec<String> = boards_seen
                        .iter()
                        .take(EXERCISE_BOARDS_PER_WORKER)
                        .cloned()
                        .collect();
                    if boards_seen.len() > taken.len() {
                        // No silent caps: a run that sampled 5 of 40 boards
                        // says so, because "traceability 1.00" over five boards
                        // and over forty are different claims.
                        println!(
                            "  exercise: sampling {} of this worker's {} boards",
                            taken.len(),
                            boards_seen.len()
                        );
                    }
                    for board_id in taken {
                        match core.make_exercise(&board_id, None) {
                            Ok(outcome) => {
                                let record = exercise_record(&core, &board_id, &outcome);
                                if let Ok(mut out) = exercises.lock() {
                                    out.push(record);
                                }
                            }
                            Err(e) => eprintln!("  exercise on {board_id} failed: {e}"),
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
    let exercises = exercises
        .lock()
        .map(|mut e| std::mem::take(&mut *e))
        .unwrap_or_default();
    // The vault's own check, run once outside the workers.
    let vault_links = match vault::load(&args.corpus).and_then(|p| vault::audit(&args.corpus, &p)) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("the vault could not be audited: {e}");
            Vec::new()
        }
    };
    if let Err(e) = write_records(
        &dir,
        &records,
        &exercises,
        &vault_links,
        &args,
        &pack,
        total,
        failures,
    ) {
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

fn run_one(core: &mut Core, q: &Question, parent: Option<&Answered>, failures: &mut usize) -> RunRecord {
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
        parent.map_or_else(Anchor::default, |p| Anchor::on(&p.card_id)),
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
            let card = board
                .as_ref()
                .and_then(|b| b.cards.iter().find(|c| c.id == o.card_id));

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
                // Doc 05 section 8.5's `builds_on`, in the "board_id/card_id"
                // shape doc 15's ground truth names a prior card by. This was
                // hardcoded empty while the metric that reads it carried a
                // threshold, so prior card recall reported 0.000 for a
                // capability nothing had exercised.
                prior_cards: card
                    .map(|c| {
                        c.builds_on
                            .iter()
                            .filter_map(|b| {
                                Some(format!("{}/{}", b["board_id"].as_str()?, b["card_id"].as_str()?))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
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

/// Bring one core up, import the corpus's boards, re-verify their sources, and
/// read every card back.
///
/// One store rather than the sweep's three, because a re-verification reads
/// cards another worker would not have.
fn run_verify_only(args: &Args, pack: &str, plan: &Plan, questions: &[Question]) -> Vec<RunRecord> {
    let keys = keystore(args.mock);
    let first_key_ref = if args.mock {
        "test-key"
    } else {
        args.bulk_key_ref.as_str()
    };

    let mut core = match Core::in_memory_with_keys(Arc::clone(&plan.bulk.provider), keys, first_key_ref) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not bring a core up: {e}");
            return Vec::new();
        }
    };
    core.source = Source::Test;
    if let Err(e) = core.use_pack(pack) {
        eprintln!("could not load the `{pack}` pack: {e}");
        return Vec::new();
    }
    // The boards are imported by the same call that indexes the corpus, so the
    // cards and the tree they are judged against arrive together.
    if let Err(e) = configure_retrievers(&mut core, &args.corpus, &args.snapshot) {
        eprintln!("could not index the corpus: {e}");
        return Vec::new();
    }

    let stale = match reverify::mark(
        &mut core.store,
        &args.corpus,
        args.baseline.as_deref(),
        "reverify",
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("could not re-verify the cited sources: {e}");
            return Vec::new();
        }
    };

    verify_sweep(&mut core, &args.corpus, questions, &stale)
}

/// Read every card the corpus shipped back against the tree this run points at.
///
/// Doc 07 section B3's batch, and what doc 02 section 5.4 means by a board
/// written at T1 and reopened at T3. One store and one worker, because every
/// card has to see the same imported boards and doc 10 section 6 allows one
/// verifier per board anyway.
fn verify_sweep(
    core: &mut Core,
    corpus: &Path,
    questions: &[Question],
    stale: &reverify::StaleReport,
) -> Vec<RunRecord> {
    let mut records = Vec::new();
    let boards = match boards::load(corpus) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("could not read the corpus boards: {e}");
            return records;
        }
    };

    println!(
        "re-verifying {} boards against {}",
        boards.len(),
        corpus.display()
    );
    println!(
        "  {} of {} cited sources went stale{}",
        stale.stale,
        stale.checked,
        if stale.by_reason.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                stale
                    .by_reason
                    .iter()
                    .map(|(reason, n)| format!("{n} {reason}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    if stale.content_comparison_skipped {
        // BN-019. A run that could not compare says so, rather than letting a
        // reader take the absence of `content_changed` for evidence of none.
        println!(
            "  no baseline tree was given, so a quietly edited document reads as unchanged. \
             Pass --baseline to measure that."
        );
    }
    if stale.unresolvable > 0 {
        println!("  {} cited sources point outside the corpus", stale.unresolvable);
    }

    for board in &boards {
        for card in &board.cards {
            // A card that cited nothing has no source to have gone stale, so
            // re-verifying it would add a row that measures nothing.
            if card.citations.is_empty() {
                continue;
            }
            let started = std::time::Instant::now();
            let card_ref = format!("{}/{}", board.board_id, card.card_id);
            let mut record = empty_verify_record(board, card, started);

            match core.verify_card(&board.board_id, &card.card_id) {
                Ok(outcome) => {
                    record.ok = true;
                    record.card_id = Some(outcome.card_id.clone());
                    record.status = Some(outcome.status);
                    record.confidence = Some(outcome.confidence);
                    let view = tessera_store::repo::read_board(&core.store, &board.board_id)
                        .ok()
                        .flatten();
                    if let Some(view) = view
                        && let Some(found) = view.cards.iter().find(|c| c.id == card.card_id)
                    {
                        record.citations = found.citations.clone();
                        record.flags = found.flags.clone();
                        record.prior_cards = found
                            .builds_on
                            .iter()
                            .filter_map(|b| {
                                Some(format!("{}/{}", b["board_id"].as_str()?, b["card_id"].as_str()?))
                            })
                            .collect();
                    }
                }
                Err(e) => record.failure = Some(e.to_string()),
            }
            record.latency_ms = started.elapsed().as_millis();
            record.card_ref = Some(card_ref);
            records.push(record);
        }
    }

    let flagged = records
        .iter()
        .filter(|r| r.flags.iter().any(|f| f["rule_id"] == json!("stale_source")))
        .count();
    println!(
        "  {} cards re-verified, {flagged} carry a stale citation",
        records.len()
    );

    records.extend(follow_up_on_stale(core, questions, &stale.locators));
    records
}

/// Ask a question whose sources have gone stale, then follow up on the answer.
///
/// Doc 04 section 12 scores the Planner on whether a request whose ancestor
/// carries a stale citation earns a sub-question that re-checks it. That needs
/// such a request to exist, and this is where it comes from.
///
/// The pair is asked through the ordinary pipeline rather than on the corpus's
/// own boards. Those boards keep the generator's card ids so doc 15's ground
/// truth can name them, and doc 01 section 3 makes every id the product mints a
/// ulid, so asking a follow-up on an imported card would put a fixture id where
/// the Router's packet requires a real one. The sources are already marked
/// stale, so a card that reaches one is flagged exactly as an old card would be,
/// and its follow-up sees a genuinely stale ancestor.
///
/// The follow-up says nothing about verifying or currency on purpose. The
/// grounded mock echoes the request into its one sub-question, so a question
/// using those words would score the wording rather than the Planner.
fn follow_up_on_stale(core: &mut Core, questions: &[Question], stale_locators: &[String]) -> Vec<RunRecord> {
    const FOLLOW_UP: &str = "What does this mean for the position today?";
    let mut out = Vec::new();

    // Questions whose own required sources are among the ones that went stale.
    // A question that never reaches a stale source would produce a fresh card
    // and measure nothing.
    let stale_docs: HashSet<&str> = stale_locators
        .iter()
        .filter_map(|l| Path::new(l).file_stem().and_then(|s| s.to_str()))
        .collect();
    let touching: Vec<&Question> = questions
        .iter()
        .filter(|q| q.required_sources.iter().any(|s| stale_docs.contains(s.as_str())))
        .take(12)
        .collect();

    if touching.is_empty() {
        println!("  no question in the set reaches a stale source, so no follow-up was asked");
        return out;
    }

    let mut failures = 0usize;
    for q in touching {
        let mut root = run_one(core, q, None, &mut failures);
        root.provider = "mock".to_string();
        root.leg = "verify".to_string();
        let (Some(board_id), Some(card_id)) = (root.board_id.clone(), root.card_id.clone()) else {
            out.push(root);
            continue;
        };
        let asked_stale = root.citations.iter().any(|c| c["stale"] == json!(true));
        out.push(root);
        if !asked_stale {
            continue;
        }

        let started = std::time::Instant::now();
        let mut record = empty_record(q, String::new(), started);
        record.q_id = format!("FU-{}", q.q_id);
        record.text = FOLLOW_UP.to_string();
        record.depth_expected = "deep".to_string();
        record.parent_q_id = Some(q.q_id.clone());
        record.board_id = Some(board_id.clone());
        // A follow-up asks something the question set never asked, so it carries
        // no required facts of its own to be scored against.
        record.required_facts = Vec::new();
        record.required_sources = Vec::new();
        record.provider = "mock".to_string();
        record.leg = "verify".to_string();

        match core.ask_on(&board_id, FOLLOW_UP, Some("deep"), Anchor::on(&card_id)) {
            Ok(outcome) => {
                record.ok = true;
                record.card_id = Some(outcome.card_id);
                record.status = Some(outcome.status);
                record.confidence = Some(outcome.confidence);
                record.depth_chosen = Some("deep".to_string());
            }
            Err(e) => record.failure = Some(e.to_string()),
        }

        record.plan = core
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
        record.latency_ms = started.elapsed().as_millis();
        out.push(record);
    }

    let planned = out.iter().filter(|r| r.plan.is_some()).count();
    let with_stale = out
        .iter()
        .filter(|r| {
            r.plan
                .as_ref()
                .and_then(|p| p["constraints"]["stale_ancestor_citations"].as_array())
                .is_some_and(|a| !a.is_empty())
        })
        .count();
    println!(
        "  {} questions asked over stale sources, {planned} carried a plan, \
         {with_stale} planned against a stale ancestor",
        out.len()
    );
    out
}

fn empty_verify_record(board: &boards::Board, card: &boards::Card, started: std::time::Instant) -> RunRecord {
    RunRecord {
        q_id: format!("VO-{}-{}", board.board_id, card.card_id),
        text: card.question.clone(),
        domain: String::new(),
        depth_expected: card.depth.clone(),
        depth_chosen: Some(card.depth.clone()),
        audience_id: None,
        // What the card states. Doc 02 section 10.2 scores staleness detection
        // against the cards whose facts were superseded, and this is where that
        // denominator comes from.
        required_facts: card.fact_ids.clone(),
        required_sources: Vec::new(),
        forbidden_facts: Vec::new(),
        expected_visual: String::new(),
        expected_flags: Vec::new(),
        edge_case_ids: Vec::new(),
        parent_q_id: card.parent_card_id.clone(),
        anchor_text: card.anchor_text.clone(),
        ok: false,
        failure: None,
        card_id: None,
        board_id: Some(board.board_id.clone()),
        answer: card.answer.clone(),
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
        leg: "verify".to_string(),
        kind: "verify_only".to_string(),
        card_ref: None,
    }
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
        kind: "card".to_string(),
        card_ref: None,
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
    let provider = MockProvider::new().with_default(MockResponse::Scripted(Arc::new(|request| {
        match request.stage.as_str() {
            "route" => MockResponse::Json(routed()),
            "plan" => MockResponse::Json(planned(request)),
            "synthesize" => MockResponse::Json(synthesized(request)),
            "visualize" => MockResponse::Json(visualised(request)),
            "verify" => MockResponse::Json(support_judged(request)),
            "exercise" => MockResponse::Json(exercised(request)),
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
            // Empty, and stated as a limit rather than left as an oversight.
            //
            // The M9 Concept write path turns these into proposals, so an empty
            // array means the grounded sweep never enters it. Naming entities is
            // a judgment: this corpus asks "what is the model validation
            // interval for a systemically important institution", which carries
            // no proper noun at all, so a capitalisation pass returns "What" and
            // a template pass returns whatever the templates were written with.
            // Either would put a term in the Library that nothing observed, and
            // a mock that answers plausibly is worse than one that answers
            // nothing.
            //
            // So the graph is measured by the end to end tests, where the mock
            // Router names an entity because the test wrote one, and at scale it
            // needs a real provider. BN-067 carries it with the other four.
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
/// Doc 07 section B8.2's support check, answered deterministically.
///
/// The claim is supported when the passage contains it, which on this mock is
/// the common case because the answer quotes its passages verbatim. Judging by
/// containment rather than by asking a model keeps `verifier_agreement`
/// measurable for nothing, and the Verifier's own override still runs on top.
fn support_judged(request: &CompletionRequest) -> Value {
    let prompt = prompt_of(request);

    // Doc 07 section B8.5's rules share the verify stage with the support check,
    // so the prompt says which one is being asked. The mock answers that none
    // matched: its answers are verbatim quotes of retrieved passages, so it has
    // no jurisdiction drift or scope creep to find, and inventing one would put
    // a flag in the record that nothing in the corpus expects. What this does
    // measure is that the rules are dispatched, parsed and reported at all,
    // which they were not before.
    if prompt.contains("For each rule, say whether it matches") {
        let matches: Vec<Value> = prompt
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- "))
            .filter_map(|line| line.split_once(": "))
            .map(|(rule_id, _)| json!({ "rule_id": rule_id, "matched": false }))
            .collect();
        return json!({ "matches": matches });
    }

    let passages: std::collections::BTreeMap<usize, String> = passages_in(&prompt).into_iter().collect();

    let mut verdicts = Vec::new();
    for line in prompt.lines() {
        let Some(rest) = line.trim().strip_prefix("Claim ") else {
            continue;
        };
        let Some((n, claim)) = rest.split_once(": ") else {
            continue;
        };
        let Ok(n) = n.parse::<usize>() else { continue };
        let Some(passage) = passages.get(&n) else { continue };

        // The claim carries its own citation marker and has been through the
        // answer's whitespace, so both sides are normalised before comparing.
        // Without that no claim could ever read as supported and the agreement
        // number would describe the comparison rather than the check.
        let flatten = |s: &str| {
            s.split_whitespace()
                .filter(|w| !(w.starts_with('[') && w.ends_with(']')))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let claim = flatten(claim);
        let passage = flatten(passage);
        let verdict = if claim.is_empty() || passage.contains(&claim) {
            "supported"
        } else if claim
            .split_whitespace()
            .filter(|w| w.len() > 4)
            .any(|w| passage.contains(w))
        {
            "weak"
        } else {
            "unsupported"
        };
        verdicts.push(json!({
            "n": n,
            "verdict": verdict,
            "reason": "Judged by whether the passage contains the claim.",
        }));
    }

    json!({ "verdicts": verdicts })
}

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
        let Some(close) = body.find("</passage>") else {
            break;
        };
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
    let mut values = Vec::new();
    for (ordinal, text) in passages.iter().take(6) {
        let sentence = text.split_whitespace().collect::<Vec<_>>().join(" ");
        answer.push_str(&sentence);
        answer.push_str(&format!(" [{ordinal}]"));
        answer.push(' ');
        citations.push(json!({ "ordinal": ordinal, "binding": "answer" }));
        if findings.len() < 3 {
            // A string carrying its own marker, which is the shape
            // `draft_schema` declares. Objects were silently dropped by the
            // Synthesizer's `filter_map(Value::as_str)`, so every grounded run
            // ever recorded produced a card with no findings at all.
            let short: String = sentence.chars().take(200).collect();
            findings.push(json!(format!("{short} [{ordinal}]")));
        }
        // One value per passage, labelled by the passage it came from and
        // citing it. Doc 06 section B8.1 builds a table from two or more values,
        // so this is what gives the Visualizer something to compose. Without it
        // it declined on every question and the whole of doc 06 part B went
        // unmeasured while the report said nothing was wrong.
        if let Some(number) = first_number(&sentence) {
            values.push(json!({
                "label": format!("Passage {ordinal}"),
                "value": number,
                "citation": ordinal,
            }));
        }
    }

    json!({
        "answer": answer.trim(),
        "findings": findings,
        "citations": citations,
        "structured_summary": {
            "values": values,
            "steps": [],
            "groups": [],
            "relations": []
        },
        "scope_statement": "Answered from the retrieved passages.",
        "confidence": 0.6,
        "caveats": []
    })
}

/// The first number in a passage, as the mock's stand in for a value worth
/// tabulating. Deterministic, so two runs of one corpus compose one table.
fn first_number(text: &str) -> Option<String> {
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || (!current.is_empty() && (ch == '.' || ch == ',')) {
            current.push(ch);
        } else if !current.is_empty() {
            let trimmed = current.trim_end_matches(['.', ',']).to_string();
            if trimmed.chars().any(|c| c.is_ascii_digit()) {
                return Some(trimmed);
            }
            current.clear();
        }
    }
    let trimmed = current.trim_end_matches(['.', ',']).to_string();
    trimmed.chars().any(|c| c.is_ascii_digit()).then_some(trimmed)
}

fn visualised(request: &CompletionRequest) -> Value {
    // Compose a table from the values the summary carries, in the order the
    // prompt lists them. The labels have to come back verbatim or doc 06
    // section B8.3 cannot trace them, which is exactly what this exercises.
    // The prompt carries the summary as pretty printed json, so the labels are
    // read back out of it. They have to come back verbatim: doc 06 section B8.3
    // drops a block whose label traces to nothing in the summary, and this is
    // what exercises that.
    let prompt = prompt_of(request);
    let mut labels = Vec::new();
    let mut values = Vec::new();
    for line in prompt.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("\"label\": \"") {
            labels.push(rest.trim_end_matches([',', '"']).to_string());
        } else if let Some(rest) = line.strip_prefix("\"value\": \"") {
            values.push(rest.trim_end_matches([',', '"']).to_string());
        }
    }

    if labels.is_empty() {
        return json!({ "declined": true, "reason": "no_structure", "visual": null });
    }

    json!({
        "type": "table",
        "title": "Values by source",
        "payload": {
            "columns": ["Source", "Value"],
            "rows": labels
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    json!([label, values.get(i).cloned().unwrap_or_else(|| label.clone())])
                })
                .collect::<Vec<_>>(),
        },
        "caveats": []
    })
}

/// An exercise built from the cards in the prompt, quoting them.
///
/// The same contract as `synthesized`: it invents nothing and it judges nothing.
/// The correct option is a sentence lifted from the card, so doc 08 section 5's
/// traceability rule passes for a reason rather than by luck, and the
/// distractors are statements about absence, which cannot be true on another
/// card because no card says them.
///
/// What this cannot measure is whether a real model writes a question worth
/// answering: doc 08 section 12's fourth line, "answerable from the source card
/// by a second model", needs two real models. That is on the spend list.
fn exercised(request: &CompletionRequest) -> Value {
    let prompt = prompt_of(request);
    let mut items: Vec<Value> = Vec::new();

    let mut card_id: Option<String> = None;
    let mut question: Option<String> = None;
    for line in prompt.lines() {
        let line = line.trim();
        if let Some(id) = line.strip_prefix("card_id: ") {
            card_id = Some(id.to_string());
            question = None;
        } else if let Some(q) = line.strip_prefix("question: ") {
            question = Some(q.to_string());
        } else if let Some(answer) = line.strip_prefix("answer: ")
            && let (Some(id), Some(q)) = (card_id.clone(), question.clone())
        {
            // The first sentence, which is what a recall item asks for and what
            // the traceability check will look for in the card.
            let claim = answer
                .split_once(". ")
                .map(|(first, _)| first.to_string())
                .unwrap_or_else(|| answer.to_string());
            let claim = claim.trim().trim_end_matches('.').to_string();
            if claim.split_whitespace().count() < 3 {
                continue;
            }
            items.push(json!({
                "id": format!("i{}", items.len() + 1),
                "kind": "recall",
                "prompt": format!("According to this card, {}", q.trim_end_matches('?')),
                "options": [
                    { "id": "a", "text": claim },
                    { "id": "b", "text": "This card does not say." },
                    { "id": "c", "text": "The card gives a range rather than a figure." },
                    { "id": "d", "text": "The card defers to a later regulation." },
                ],
                "answer_id": "a",
                "explanation": "The card states it in its opening sentence.",
                "source_card_id": id,
            }));
        }
        if items.len() >= 8 {
            break;
        }
    }

    json!({ "items": items })
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

    // The same choice the Core makes, rather than a second one. This read
    // `OsKeychain` directly while `keystore` honoured `TESSERA_CI` beside it, so
    // the environment path existed, was tested, and could never actually fetch a
    // provider secret: the plan builder is what reaches for one. BN-095 recorded
    // the CI eval as written and unproven, and this is what unproven was hiding.
    let keys = keystore(false);

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
fn configure_retrievers(core: &mut Core, corpus: &Path, snapshot: &str) -> Result<(), String> {
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
        (
            "regulatory",
            "Central Authority for Prudential Oversight",
            corpus.join("corpus/regulatory"),
        ),
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

    // Doc 02 section 6's twenty prior boards. Without them the boards retriever
    // searches an empty index, and doc 15's three gates measure nothing while
    // reporting 0.000, which reads as a broken retriever rather than an
    // unasked question.
    let pack_id = core.active_pack_id().map_err(|e| format!("pack: {e}"))?;
    let seeded = boards::load(corpus)?;
    let report = boards::seed(
        &mut core.store,
        &profile_id,
        &pack_id,
        &seeded,
        snapshot,
        embedder.as_deref(),
    )?;
    if !report.eligibility_disagreements.is_empty() {
        // Doc 15 section 3 is the rule and the corpus label is a second opinion.
        // Where they differ one of them is wrong, and finding out which is worth
        // more than quietly trusting either.
        eprintln!(
            "  boards: {} of {} cards disagree with the corpus on eligibility: {}",
            report.eligibility_disagreements.len(),
            report.cards,
            report.eligibility_disagreements.join("; ")
        );
    }
    println!(
        "  boards: {} boards, {} cards, {} eligible to remember",
        report.boards, report.cards, report.indexed
    );

    // Doc 16 section 3.3: the vault is a folder like any other and its pages
    // are a class of their own. Seeded after the boards, because two dozen of
    // them are pages saved from cards on those boards.
    let pages = vault::load(corpus)?;
    if !pages.is_empty() {
        let report = vault::seed(
            &mut core.store,
            &profile_id,
            &pack_id,
            corpus,
            &pages,
            embedder.as_deref(),
        )?;
        if !report.disagreements.is_empty() {
            // The corpus writes the file and the row from one page. A
            // difference is a generator bug, and quietly trusting one of them
            // would score the product against a vault nobody wrote.
            eprintln!(
                "  vault: {} pages disagree with their files: {}",
                report.disagreements.len(),
                report.disagreements.join(", ")
            );
        }
        println!(
            "  vault: {} pages ({} saved from cards, {} said nowhere else), {} links, \
             {} unresolved",
            report.pages, report.saved_from_cards, report.page_only, report.links, report.unresolved
        );
        configured.push((
            "vault".to_string(),
            IndexedConfig::pages(vec![tessera_retrievers::VAULT_FOLDER.to_string()]),
        ));
    }

    core.retrievers = RetrieverSet {
        indexed: configured,
        embedder,
    };
    Ok(())
}

/// The folder names doctrine says never to open.
fn doctrine_exclusions(core: &Core) -> Vec<String> {
    core.packs
        .get(&core.pack_code)
        .map(|p| p.must_exclude())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn write_records(
    dir: &Path,
    records: &[RunRecord],
    exercises: &[ExerciseRecord],
    vault_links: &[vault::LinkRow],
    args: &Args,
    pack: &str,
    total: usize,
    failures: usize,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;

    // Doc 16 phase 12c: backlinks are a query over PageLink. The rows say what
    // the query answered for every link the corpus planted, and the scorer does
    // the arithmetic, because measuring a query with itself reports 1.00
    // whatever it does.
    if !vault_links.is_empty() {
        let mut file = std::fs::File::create(dir.join("vault_links.jsonl"))?;
        for link in vault_links {
            writeln!(file, "{}", serde_json::to_string(link).unwrap_or_default())?;
        }
    }

    let mut file = std::fs::File::create(dir.join("runs.jsonl"))?;
    for r in records {
        writeln!(file, "{}", serde_json::to_string(r).unwrap_or_default())?;
    }

    // Written only when there is one. An empty file would have the scorer read
    // zero items and report a ratio over nothing, and doc 08's two gates say
    // n/a when no exercise ran rather than 0.
    if !exercises.is_empty() {
        let mut file = std::fs::File::create(dir.join("exercises.jsonl"))?;
        for e in exercises {
            writeln!(file, "{}", serde_json::to_string(e).unwrap_or_default())?;
        }
    }

    let mut counts = serde_json::Map::new();
    for record in records {
        let seen = counts.get(&record.provider).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(record.provider.clone(), json!(seen + 1));
    }
    let by_provider = Value::Object(counts);

    let manifest = json!({
        "corpus": corpus_name(&args.corpus),
        "policy": args.policy,
        "pack": pack,
        "snapshot": args.snapshot,
        // What was configured. `questions_by_provider` below says where the
        // questions actually went, which is not the same thing: a run with
        // `--sample-per-depth` high enough sends every question to the
        // reference leg while this still names the bulk provider. Doc 02
        // section 10.1 keeps this field so two runs' numbers stay comparable
        // instead of mixed, and a run recorded as one provider that ran on
        // another is the mixing it exists to prevent.
        "provider": if args.mock { "mock" } else { args.bulk_provider.as_str() },
        "bulk_provider": args.bulk_provider,
        "bulk_models": {
            "small": args.bulk_small,
            "medium": args.bulk_medium,
            "frontier": args.bulk_frontier
        },
        "reference_provider": if args.mock { "mock" } else { "anthropic" },
        "sample_per_depth": args.sample_per_depth,
        // A re-verification asks nothing, so it counts cards read back rather
        // than questions run. Reporting them as questions would have the scorer
        // divide answer metrics by a denominator nobody was asked.
        "questions_run": if args.verify_only { 0 } else { total },
        "verify_only": args.verify_only,
        "cards_reverified": if args.verify_only { records.len() } else { 0 },
        "baseline": args.baseline.as_deref().map(corpus_name),
        "cards_failed": failures,
        // Doc 07 section B8.2's check runs from M8, so the verdicts in this run
        // are the Verifier's own rather than a placeholder. Doc 07 section B9
        // still withholds full automation until agreement with the ledger check
        // reaches 0.90, which is what `verifier_agreement` measures; the flag
        // says the verdicts are real, not that the gate has been passed.
        "support_check_enabled": true,
        // Doc 08 section 12's two gates report n/a until this is true, because
        // a run that generated no exercise has nothing to say about items that
        // do not exist.
        "exercise_enabled": args.exercise,
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
        // Counted from the records rather than from the arguments, so the
        // manifest says where the questions went and not where they were meant
        // to go.
        "questions_by_provider": by_provider,
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )?;
    Ok(())
}
