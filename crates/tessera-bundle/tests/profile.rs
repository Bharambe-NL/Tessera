#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Backup, restore and diagnostics. Doc 10 sections 11 and 15.
//!
//! Three files leave a profile folder and each one is for a different person.
//! A bundle is a board someone chose to share. A backup goes to their own disk.
//! A diagnostics zip is handed to a stranger, so the tests below spend most of
//! their effort on the third.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use rusqlite::params;
use serde_json::json;
use tessera_bundle::{back_up, diagnostics, restore};
use tessera_store::event::{NewEvent, Provenance};
use tessera_store::{Store, new_id, now_iso8601, repo};

fn folder(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("tessera-{label}-{}", new_id()));
    fs::create_dir_all(&root).expect("dir");
    root
}

/// A profile with one board, one answered card, and a blob.
fn seeded(root: &PathBuf) -> (Store, String, String) {
    let mut store = Store::open(root).expect("store");
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());
    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .expect("pack");
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .expect("profile");

    let board = repo::create_board(
        &mut store,
        repo::NewBoard {
            profile_id: &profile,
            title: "Capital rules",
            doctrine_pack_id: &pack,
            default_depth: "deep",
            named_by_user: true,
            parent_board_id: None,
            seed_label: None,
            context: None,
        },
    )
    .expect("board");

    let card = repo::create_card(
        &mut store,
        repo::NewCard {
            board_id: &board,
            parent_card_id: None,
            kind: "root",
            question: "What buffer applies?",
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )
    .expect("card");

    let run = repo::start_run(
        &store,
        repo::NewRun {
            board_id: &board,
            card_id: Some(&card),
            kind: "card",
            depth: Some("deep"),
            policy_snapshot: &json!({}),
            pack_version: "1.0",
        },
    )
    .expect("run");

    repo::write_answer(
        &mut store,
        repo::CardRef {
            card_id: &card,
            board_id: &board,
            run_id: &run,
        },
        "The buffer is 2.5 per cent, said the private memo.",
        &json!([]),
        &json!({ "agent_id": "synthesizer" }),
        json!({ "card_id": card }),
    )
    .expect("answer");

    store.blobs().put(b"a picture").expect("blob");
    (store, profile, board)
}

#[test]
fn a_profile_backs_up_and_restores_into_an_empty_folder() {
    let from = folder("backup-from");
    let (store, _profile, board) = seeded(&from);

    let mut archive = Cursor::new(Vec::new());
    let manifest = back_up(&store, &mut archive).expect("back up");
    assert_eq!(manifest.counts["board"], 1);
    assert_eq!(manifest.counts["card"], 1);
    assert_eq!(manifest.blobs, 1);
    drop(store);

    // The snapshot is a second copy of everything, so it is not left behind.
    let strays: Vec<_> = fs::read_dir(&from)
        .expect("read")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("backup-"))
        .collect();
    assert!(strays.is_empty(), "a snapshot was left in the profile folder");

    let into = folder("backup-into");
    fs::remove_dir_all(&into).expect("start empty");
    archive.set_position(0);
    let read = restore(archive, &into).expect("restore");
    assert_eq!(read.counts["card"], 1);

    // And it is a working profile, not a file that merely landed.
    let restored = Store::open(&into).expect("the restored profile opens");
    let cards: i64 = restored
        .conn()
        .query_row("SELECT COUNT(*) FROM card WHERE board_id = ?1", [&board], |r| {
            r.get(0)
        })
        .expect("count");
    assert_eq!(cards, 1);
    assert!(into.join("blobs").exists(), "the blobs did not travel");

    let _ = fs::remove_dir_all(&from);
    let _ = fs::remove_dir_all(&into);
}

#[test]
fn a_restore_refuses_a_folder_that_already_holds_a_profile() {
    // Doc 10 section 15 offers a restore to someone whose database is damaged.
    // The worst reading of that offer is one that overwrites the damaged file
    // before anyone has looked at it.
    let from = folder("refuse-from");
    let (store, _, _) = seeded(&from);
    let mut archive = Cursor::new(Vec::new());
    back_up(&store, &mut archive).expect("back up");
    drop(store);

    archive.set_position(0);
    let refused = restore(archive, &from);
    assert!(refused.is_err(), "a restore wrote over a live profile");

    let _ = fs::remove_dir_all(&from);
}

#[test]
fn a_diagnostics_export_carries_no_answer_and_no_passage() {
    // The file people send to a stranger. Doc 10 section 11: prompt text
    // redacted, no remote reporting.
    let root = folder("diag");
    let (mut store, _, board) = seeded(&root);

    // An event whose payload nests content two levels down, which is where a
    // shallow redaction leaks.
    store
        .append(
            NewEvent::new(
                "flag.raised.v1",
                json!({
                    "card_id": "01M113",
                    "flags": [{
                        "rule_id": "stale_source",
                        "reason": "A cited source changed.",
                        "evidence": { "passage_text": "the private memo said this" }
                    }]
                }),
                Provenance::user(),
            )
            .on_board(&board),
        )
        .expect("event");

    let mut archive = Cursor::new(Vec::new());
    let summary = diagnostics(&store, &mut archive).expect("diagnostics");
    assert!(summary.runs >= 1);
    assert!(summary.events >= 1);

    let bytes = archive.into_inner();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut all = String::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let mut text = String::new();
        // Every file, not a chosen one: the point is that nothing in the zip
        // carries it, and a test that reads one file proves it about one file.
        if std::io::Read::read_to_string(&mut entry, &mut text).is_ok() {
            all.push_str(&text);
        }
    }

    for leaked in [
        "The buffer is 2.5 per cent",
        "private memo",
        "A cited source changed",
        "What buffer applies?",
    ] {
        assert!(!all.contains(leaked), "the export carries {leaked:?}");
    }

    // And it still says something. A file that carried nothing would be safe
    // and useless, which is the other way to fail this.
    assert!(
        all.contains("stale_source"),
        "the rule that fired is not in the export"
    );
    assert!(
        all.contains("flag.raised.v1"),
        "the event type is not in the export"
    );
    assert!(
        all.contains("latency_ms"),
        "the health numbers are not in the export"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_diagnostics_export_names_what_it_dropped() {
    // So a reader asks for a field rather than concluding it never existed.
    let root = folder("diag-counts");
    let (store, _, _) = seeded(&root);
    let mut archive = Cursor::new(Vec::new());
    let summary = diagnostics(&store, &mut archive).expect("diagnostics");
    assert!(
        summary.redacted.contains_key("answer") || summary.redacted.contains_key("question"),
        "nothing was reported as redacted: {:?}",
        summary.redacted
    );
    let _ = fs::remove_dir_all(&root);
}
