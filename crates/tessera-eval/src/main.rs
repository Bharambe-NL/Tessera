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

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tessera_core::Core;
use tessera_providers::{MockProvider, MockResponse, ModelProvider};
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

    /// Which snapshot's labels the questions are scored against.
    #[arg(long, default_value = "T1")]
    snapshot: String,

    /// Doc 02 section 10.1 loads `finance-eu-synthetic`: the same rules as the
    /// shipped finance pack with the synthetic issuers substituted in.
    #[arg(long, default_value = "finance-eu-synthetic")]
    pack: String,
}

#[derive(Debug, Deserialize)]
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
    answer: Option<String>,
    findings: Vec<String>,
    visual_type: Option<String>,
    visual_labels: Vec<String>,
    block_index: Vec<Value>,
    citations: Vec<Value>,
    flags: Vec<Value>,
    status: Option<String>,
    confidence: Option<f64>,
    /// Every event the run emitted, so the scorer can check what happened rather
    /// than only what was returned.
    events: Vec<Value>,
    cost: Value,
    latency_ms: u128,
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();

    let questions = match load_questions(&args.corpus.join("questions.jsonl")) {
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
        if args.mock { "mock" } else { "configured" }
    );

    let provider = build_provider(args.mock);
    let mut core = match Core::in_memory(provider) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not bring the core up: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    // Doc 02 section 10.1 and doc 10 section 5: test provenance, so nothing here
    // trips a policy hook meant for live work.
    core.source = Source::Test;
    if let Err(e) = core.use_pack(&args.pack) {
        eprintln!("could not load the `{}` pack: {e}", args.pack);
        return std::process::ExitCode::from(2);
    }

    let mut records = Vec::with_capacity(total);
    let mut failures = 0usize;

    for (i, q) in questions.iter().take(total).enumerate() {
        if i % 25 == 0 && i > 0 {
            println!("  {i}/{total}");
        }
        records.push(run_one(&mut core, q, &mut failures));
    }

    let dir = args
        .out
        .join(corpus_name(&args.corpus))
        .join(&args.policy)
        .join(stamp());
    if let Err(e) = write_records(&dir, &records, &args, total, failures) {
        eprintln!("could not write the results: {e}");
        return std::process::ExitCode::from(2);
    }

    println!("\n{total} questions, {failures} failed to produce a card");
    println!("wrote {}", dir.display());
    println!("score it with: gen score --results {}", dir.display());
    std::process::ExitCode::SUCCESS
}

fn run_one(core: &mut Core, q: &Question, failures: &mut usize) -> RunRecord {
    let started = std::time::Instant::now();

    let board_id = match core.create_board(&q.text, &q.depth_expected) {
        Ok(id) => id,
        Err(e) => {
            *failures += 1;
            return empty_record(q, format!("could not create a board: {e}"), started);
        }
    };

    let outcome = core.ask(&board_id, &q.text, Some(&q.depth_expected));
    let events: Vec<Value> = core
        .store
        .events(Some(&board_id))
        .unwrap_or_default()
        .into_iter()
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
            let card = board.as_ref().and_then(|b| b.cards.first());

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
        answer: None,
        findings: Vec::new(),
        visual_type: None,
        visual_labels: Vec::new(),
        block_index: Vec::new(),
        citations: Vec::new(),
        flags: Vec::new(),
        status: None,
        confidence: None,
        events: Vec::new(),
        cost: Value::Null,
        latency_ms: started.elapsed().as_millis(),
    }
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

fn build_provider(mock: bool) -> Arc<dyn ModelProvider> {
    if mock {
        // Doc 12 phase 3's acceptance: end to end on the mock, every metric 0 or
        // n/a. The default response is garbage, so nothing here can pass by
        // accident (doc 12 operating principle 5).
        return Arc::new(MockProvider::new().with_default(MockResponse::Garbage));
    }
    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(key) => match tessera_providers::AnthropicProvider::new(key) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                eprintln!("could not build the provider ({e}); falling back to the mock");
                Arc::new(MockProvider::new())
            }
        },
        Err(_) => {
            eprintln!("no ANTHROPIC_API_KEY; falling back to the mock");
            Arc::new(MockProvider::new())
        }
    }
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

fn write_records(
    dir: &Path,
    records: &[RunRecord],
    args: &Args,
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
        "pack": args.pack,
        "snapshot": args.snapshot,
        "provider": if args.mock { "mock" } else { "configured" },
        "questions_run": total,
        "cards_failed": failures,
        // Doc 07 section B9: the support check is not enabled until its
        // agreement is measured, so every verdict in this run is `unchecked`.
        // A scorer that read them as `supported` would report a number the
        // product has not earned.
        "support_check_enabled": false,
        "retrievers_enabled": false,
    });
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    )?;
    Ok(())
}
