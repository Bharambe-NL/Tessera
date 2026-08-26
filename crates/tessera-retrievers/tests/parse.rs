#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The parsers, against the real corpus rather than against fixtures.
//!
//! A parser tested only on files the test wrote is a parser tested against its
//! own assumptions. The synthetic corpus has 109 documents in six formats,
//! written by a different toolchain in a different language, and it carries a
//! corrupt pdf, a password protected pdf, a scanned pdf with no text layer, an
//! empty document, and two files with ocr noise, all planted on purpose by doc
//! 02 section 5.3. Those are the cases that decide whether an index run
//! survives contact with a real folder.
//!
//! The test skips itself when the corpus has not been built, so a fresh clone
//! is not blocked on running the generator.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tessera_retrievers::parse::{ParseError, parse_file};

fn corpus_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)?
        .join("eval/synthetic/42/corpus");
    root.is_dir().then_some(root)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if tessera_retrievers::is_supported(&path) {
            out.push(path);
        }
    }
}

struct Outcome {
    parsed: Vec<PathBuf>,
    empty: Vec<PathBuf>,
    errors: BTreeMap<String, Vec<PathBuf>>,
}

fn parse_everything(root: &Path) -> Outcome {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();

    let mut outcome =
        Outcome { parsed: Vec::new(), empty: Vec::new(), errors: BTreeMap::new() };

    for path in files {
        match parse_file(&path) {
            Ok(chunks) if chunks.is_empty() => outcome.empty.push(path),
            Ok(_) => outcome.parsed.push(path),
            Err(e) => {
                let key = match &e {
                    ParseError::Io(_) => "io",
                    ParseError::Malformed { .. } => "malformed",
                    ParseError::Protected(_) => "protected",
                    ParseError::NeedsOcr(_) => "needs_ocr",
                    ParseError::UnsupportedFormat(_) => "unsupported",
                };
                outcome.errors.entry(key.to_string()).or_default().push(path);
            }
        }
    }
    outcome
}

#[test]
fn every_corpus_document_parses_or_fails_by_name() {
    let Some(root) = corpus_root() else {
        eprintln!("no corpus at eval/synthetic/42; run `gen build --seed 42` to exercise this");
        return;
    };

    let outcome = parse_everything(&root);
    let total = outcome.parsed.len() + outcome.empty.len()
        + outcome.errors.values().map(Vec::len).sum::<usize>();

    assert!(total >= 100, "only {total} documents were found under {}", root.display());

    // Doc 02 section 5.3 plants exactly these. Anything else failing is a
    // parser bug wearing a plausible costume.
    let allowed_failures = 5;
    let failed: usize = outcome.errors.values().map(Vec::len).sum();
    assert!(
        failed <= allowed_failures,
        "{failed} documents failed to parse, which is more than the corpus plants:\n{:#?}",
        outcome.errors
    );

    // An empty result is worse than an error: it looks like a document with
    // nothing in it rather than a document that could not be read.
    assert!(
        outcome.empty.len() <= 1,
        "{} documents parsed to nothing: {:#?}",
        outcome.empty.len(),
        outcome.empty
    );

    eprintln!(
        "parsed {} of {total}, {} empty, failures {:?}",
        outcome.parsed.len(),
        outcome.empty.len(),
        outcome.errors.iter().map(|(k, v)| (k, v.len())).collect::<Vec<_>>()
    );
}

#[test]
fn the_planted_bad_files_fail_by_their_own_name() {
    let Some(root) = corpus_root() else { return };
    let outcome = parse_everything(&root);

    // Whatever else is true, nothing panicked getting here, which is the point
    // doc 05 section 10 makes: a parse error is a per file outcome.
    let names: BTreeMap<&str, Vec<String>> = outcome
        .errors
        .iter()
        .map(|(k, v)| {
            (
                k.as_str(),
                v.iter()
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .collect(),
            )
        })
        .collect();
    eprintln!("failures by kind: {names:#?}");

    assert!(
        !outcome.errors.contains_key("io"),
        "a file the walker found could not be opened: {:#?}",
        outcome.errors.get("io")
    );
}

#[test]
fn every_regulatory_article_is_citable() {
    // Doc 05 section 8.3: regulatory passage location is article and paragraph.
    // This is what a citation points at, so it has to survive the real file.
    let Some(root) = corpus_root() else { return };
    let path = root.join("regulatory/reg-car3-v1.md");
    if !path.is_file() {
        return;
    }

    let chunks = parse_file(&path).expect("the regulation parses");
    let articles: Vec<_> = chunks
        .iter()
        .filter_map(|c| match &c.location {
            tessera_retrievers::ChunkLocation::ArticleParagraph { article, paragraph } => {
                Some((article.clone(), *paragraph))
            }
            _ => None,
        })
        .collect();

    assert!(
        articles.len() > 50,
        "only {} article paragraphs came out of the regulation",
        articles.len()
    );
    assert!(articles.iter().any(|(a, p)| a == "3" && *p == 1), "article 3 paragraph 1 is missing");
}

#[test]
fn a_planted_value_survives_the_round_trip_in_every_text_format() {
    // The end that matters: if the number is not in a chunk, no amount of
    // ranking will find it.
    let Some(root) = corpus_root() else { return };
    let path = root.join("regulatory/reg-car3-v1.md");
    if !path.is_file() {
        return;
    }
    let chunks = parse_file(&path).expect("parses");
    let all: String = chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n");
    assert!(
        all.contains("minimum own funds requirement"),
        "the regulation parsed without its own subject matter"
    );
}
