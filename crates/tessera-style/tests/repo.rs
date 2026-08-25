#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The house style, enforced on the tree rather than described in a document.
//!
//! `HANDOFF.md` section 7 asks for these as a lint on UI strings. This is that
//! lint. It runs under `cargo test`, so the same command that proves the code
//! works proves the strings read the way the owner asked.
//!
//! Scope is every surface a user reads: the UI, the prompts that put words in
//! an agent's mouth, and the doctrine packs, whose flag reasons appear verbatim
//! in the Flags queue.

use std::path::{Path, PathBuf};

use tessera_style::Surface;

fn repo_root() -> PathBuf {
    // crates/tessera-style -> crates -> repo root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Every file whose strings a user can read, and how to read them.
///
/// A general TypeScript module is absent on purpose. Its strings are selectors,
/// keys and svg path data, and guessing which of them is copy produces noise.
/// The convention instead is `strings.ts`: UI copy lives there and the lint
/// picks it up by name. M9 moves the copy it writes into that file.
fn surfaces(root: &Path) -> Vec<(PathBuf, Surface)> {
    let mut out = Vec::new();
    collect(&root.join("app/ui/src"), &mut out);
    collect(&root.join("packs"), &mut out);

    for single in ["app/ui/index.html", "crates/tessera-agents/src/prompts.rs"] {
        let p = root.join(single);
        if let Some(surface) = surface_of(&p) {
            out.push((p, surface));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn surface_of(path: &Path) -> Option<Surface> {
    if !path.is_file() {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    match path.extension()?.to_str()? {
        "html" => Some(Surface::Html),
        "rs" => Some(Surface::Rust),
        "json" if path.parent()?.ends_with("packs") => Some(Surface::Pack),
        "ts" if name == "strings.ts" || name == "copy.ts" => Some(Surface::Copy),
        _ => None,
    }
}

fn collect(dir: &Path, out: &mut Vec<(PathBuf, Surface)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if let Some(surface) = surface_of(&path) {
            out.push((path, surface));
        }
    }
}

#[test]
fn every_user_facing_string_follows_the_house_style() {
    let root = repo_root();
    let files = surfaces(&root);
    assert!(
        files.len() >= 4,
        "the lint found almost nothing to read, so it is probably looking in the wrong place: {files:?}"
    );

    let mut report = String::new();
    for (file, surface) in &files {
        let source = std::fs::read_to_string(file).expect("read");
        let strings = tessera_style::extract(&source, *surface);
        for v in tessera_style::violations(&source, &strings) {
            let rel = file.strip_prefix(&root).unwrap_or(file);
            report.push_str(&format!("{}:{v}\n", rel.display()));
        }
    }

    assert!(
        report.is_empty(),
        "the house style in HANDOFF.md section 7 is broken in {} place(s):\n{report}",
        report.lines().count()
    );
}

#[test]
fn the_lint_would_catch_a_violation_if_one_were_written() {
    // A lint that passes because it reads nothing is worse than no lint. This
    // seeds one string of each kind and proves every rule still bites.
    let seeded = "<p>Export This Board As A Bundle</p>\n\
                  <p>Sorry, the folder could not be read</p>\n\
                  <p>This is not a warning, it is a block</p>\n\
                  <p>Verified \u{2014} see sources</p>";
    let strings = tessera_style::extract(seeded, Surface::Html);
    let found = tessera_style::violations(seeded, &strings);
    let rules: std::collections::HashSet<_> = found.iter().map(|v| v.rule).collect();

    assert!(rules.contains(&tessera_style::Rule::TitleCase), "{found:?}");
    assert!(rules.contains(&tessera_style::Rule::Apology), "{found:?}");
    assert!(rules.contains(&tessera_style::Rule::NotXButY), "{found:?}");
    assert!(rules.contains(&tessera_style::Rule::Dash), "{found:?}");
}

#[test]
fn the_lint_is_actually_reading_the_surfaces() {
    // The failure mode this guards: an extractor that quietly returns nothing
    // makes the check above pass on an empty set and report a clean tree.
    let root = repo_root();
    let mut total = 0usize;
    let mut per_surface: Vec<(String, usize)> = Vec::new();
    for (file, surface) in surfaces(&root) {
        let source = std::fs::read_to_string(&file).expect("read");
        let n = tessera_style::extract(&source, surface).len();
        total += n;
        if n > 0 {
            let rel = file.strip_prefix(&root).unwrap_or(&file).display().to_string();
            per_surface.push((rel, n));
        }
    }
    assert!(total >= 20, "only {total} strings read from {per_surface:?}");
}
