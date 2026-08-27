//! Measure what the index can find, before any agent is built on top of it.
//!
//! Doc 10 section 17 question 2 leaves the embedding model to be settled "with
//! the synthetic recall numbers", and doc 05 section 12 sets the gates: local
//! 0.90, regulatory 0.95. Those are properties of the index, not of the agent
//! layer, so they are measurable now and there is no reason to build a
//! retriever agent on an index nobody has weighed.
//!
//! This writes what it retrieved and scores nothing. Scoring happens in
//! `gen score-retrieval`, against the same `matchers` module every other metric
//! uses, because a second matcher written in a second language is a second
//! definition of a hit and the two will disagree eventually.
//!
//! Usage:
//!   cargo run -p tessera-retrievers --bin index_recall -- \
//!     --corpus eval/synthetic/42 --out eval/results/retrieval/hybrid.jsonl

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tessera_retrievers::embed::{Embedder, LocalEmbedder};
use tessera_retrievers::{index, parse_file};

#[derive(Debug, Deserialize)]
struct Question {
    q_id: String,
    text: String,
    #[serde(default)]
    required_facts: Vec<String>,
    #[serde(default)]
    required_sources: Vec<String>,
    #[serde(default)]
    depth_expected: String,
    #[serde(default)]
    edge_case_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Retrieved {
    q_id: String,
    text: String,
    required_facts: Vec<String>,
    required_sources: Vec<String>,
    depth_expected: String,
    /// Doc 02 section 5.2's case tags, so the scorer can tell a retriever miss
    /// from a question that names no subject to find.
    edge_case_ids: Vec<String>,
    /// Which folder each passage came from, so recall can be split the way doc
    /// 05 section 12 splits its gates.
    passages: Vec<Passage>,
}

#[derive(Debug, Serialize)]
struct Passage {
    folder: String,
    document: String,
    text: String,
    score: f64,
}

/// How many passages a retriever is allowed to return. Doc 05 section 4's
/// packet defaults to twelve.
const MAX_PASSAGES: usize = 12;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let corpus = flag(&args, "--corpus").unwrap_or_else(|| "eval/synthetic/42".into());
    let out = flag(&args, "--out").unwrap_or_else(|| "eval/results/retrieval/run.jsonl".into());
    let lexical_only = args.iter().any(|a| a == "--lexical-only");
    let max_passages: usize = flag(&args, "--max-passages")
        .and_then(|v| v.parse().ok())
        .unwrap_or(MAX_PASSAGES);

    let corpus = PathBuf::from(corpus);
    if !corpus.is_dir() {
        eprintln!("no corpus at {}", corpus.display());
        return std::process::ExitCode::from(2);
    }

    // The embedder is optional on purpose: the lexical half alone is a real
    // number worth having, and it is the number that says how much the model
    // is actually buying.
    let embedder: Option<Box<dyn Embedder>> = if lexical_only {
        println!("lexical only, no embedding model");
        None
    } else {
        match LocalEmbedder::multilingual() {
            Ok(e) => {
                println!("embedding with {}", e.model_id());
                Some(Box::new(e))
            }
            Err(e) => {
                eprintln!("no embedding model ({e}); measuring the lexical half alone");
                None
            }
        }
    };
    let embedder_ref = embedder.as_deref();

    let store = match tessera_store::Store::open_in_memory() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not open a store: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    if let Err(e) = seed_profile(store.conn()) {
        eprintln!("could not seed the profile: {e}");
        return std::process::ExitCode::from(2);
    }

    // Doc 02 section 10.1 fixes the roots: local at corpus/internal, regulatory
    // at corpus/regulatory, web at the local static server. The Sensitive
    // folder is a folder of its own so that excluding it is a matter of not
    // naming it, which is what the hook does in the product.
    let folders: BTreeMap<&str, PathBuf> = BTreeMap::from([
        ("regulatory", corpus.join("corpus/regulatory")),
        ("local", corpus.join("corpus/internal")),
        ("web", corpus.join("corpus/web")),
    ]);

    let mut indexed = 0usize;
    let mut skipped = 0usize;
    for (folder_id, root) in &folders {
        if let Err(e) = register_folder(store.conn(), folder_id, root) {
            eprintln!("could not register {folder_id}: {e}");
            return std::process::ExitCode::from(2);
        }
        let mut files = Vec::new();
        walk(root, &mut files);
        files.sort();
        for path in files {
            // Doc 05 section 8.2: excluded folders are never opened. Here that
            // is the Sensitive tree, which no folder id names.
            if path.components().any(|c| c.as_os_str() == "Sensitive") {
                continue;
            }
            let reference = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            match parse_file(&path) {
                Ok(chunks) => {
                    match index::write_document(
                        store.conn(),
                        folder_id,
                        &reference,
                        &chunks,
                        embedder_ref,
                        "now",
                    ) {
                        Ok(_) => indexed += 1,
                        Err(e) => {
                            eprintln!("could not index {reference}: {e}");
                            skipped += 1;
                        }
                    }
                }
                Err(_) => skipped += 1,
            }
        }
        println!("  {folder_id}: indexed");
    }
    println!("{indexed} documents indexed, {skipped} skipped");

    // Before blaming ranking, prove the text is in the index at all. A chunker
    // that drops a sentence and a ranker that buries it look identical from the
    // outside and need opposite fixes.
    if args.iter().any(|a| a == "--dump-chunks") {
        let Ok(mut stmt) = store
            .conn()
            .prepare("SELECT folder_id, document_chunk_ref, chunk_text FROM index_entry")
        else {
            eprintln!("could not read the index");
            return std::process::ExitCode::from(2);
        };
        let rows: Vec<String> = match stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "folder": r.get::<_, String>(0)?,
                "document": r.get::<_, String>(1)?,
                "text": r.get::<_, String>(2)?,
            })
            .to_string())
        }) {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(e) => {
                eprintln!("could not read the index: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        let path = PathBuf::from("eval/results/retrieval/chunks.jsonl");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(
            &path,
            format!(
                "{}
",
                rows.join(
                    "
"
                )
            ),
        )
        .ok();
        println!("dumped {} chunks to {}", rows.len(), path.display());
        return std::process::ExitCode::SUCCESS;
    }

    let questions = match load_questions(&corpus.join("questions.jsonl")) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("could not read the questions: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let folder_ids: Vec<String> = folders.keys().map(|k| (*k).to_string()).collect();
    let mut records = Vec::with_capacity(questions.len());
    for (i, question) in questions.iter().enumerate() {
        if i % 50 == 0 && i > 0 {
            println!("  {i}/{}", questions.len());
        }
        let hits = index::search(
            store.conn(),
            &folder_ids,
            &question.text,
            embedder_ref,
            max_passages,
        )
        .unwrap_or_default();

        records.push(Retrieved {
            q_id: question.q_id.clone(),
            text: question.text.clone(),
            required_facts: question.required_facts.clone(),
            required_sources: question.required_sources.clone(),
            depth_expected: question.depth_expected.clone(),
            edge_case_ids: question.edge_case_ids.clone(),
            passages: hits
                .into_iter()
                .map(|h| Passage {
                    folder: folder_of(store.conn(), &h.entry_id),
                    document: h.document_ref,
                    text: h.text,
                    score: h.score,
                })
                .collect(),
        });
    }

    let out = PathBuf::from(out);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let body: String = records
        .iter()
        .filter_map(|r| serde_json::to_string(r).ok())
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(e) = std::fs::write(&out, format!("{body}\n")) {
        eprintln!("could not write {}: {e}", out.display());
        return std::process::ExitCode::from(2);
    }

    println!("wrote {}", out.display());
    println!("score it with: gen score-retrieval --results {}", out.display());
    std::process::ExitCode::SUCCESS
}

fn folder_of(conn: &rusqlite::Connection, entry_id: &str) -> String {
    conn.query_row(
        "SELECT folder_id FROM index_entry WHERE id = ?1",
        rusqlite::params![entry_id],
        |r| r.get(0),
    )
    .unwrap_or_else(|_| "unknown".into())
}

fn seed_profile(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
             retriever_config, created_at, updated_at)
         VALUES ('p', 'deep', 'pack', '{}', '{}', 'now', 'now')
         ON CONFLICT DO NOTHING",
        [],
    )?;
    Ok(())
}

fn register_folder(conn: &rusqlite::Connection, id: &str, root: &Path) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
         VALUES (?1, 'p', ?2, ?1, 'now') ON CONFLICT DO NOTHING",
        rusqlite::params![id, root.display().to_string()],
    )?;
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if tessera_retrievers::is_supported(&path) {
            out.push(path);
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

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
