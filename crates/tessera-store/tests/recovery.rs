#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Disaster recovery. Doc 10 section 15.
//!
//! "A corrupted SQLite file is detected on start; the app offers restore from
//! the last backup and keeps the damaged file aside."

use std::fs;

use tessera_store::{Store, StoreError, integrity, quarantine};

fn profile() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("tessera-recovery-{}", tessera_store::new_id()));
    fs::create_dir_all(&root).expect("dir");
    root
}

#[test]
fn a_fresh_folder_opens_rather_than_reporting_damage() {
    // No database yet is the first run, not a fault. Reporting one would meet
    // every new user with a corruption warning.
    let root = profile();
    let store = Store::open(&root).expect("a new profile opens");
    drop(store);
    assert!(integrity(&root).is_ok());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_healthy_profile_reopens() {
    let root = profile();
    {
        let mut store = Store::open(&root).expect("open");
        let _ = &mut store;
    }
    Store::open(&root).expect("a profile that was closed cleanly reopens");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_damaged_database_is_reported_and_not_migrated_over() {
    // The failure this prevents: migrations run against damaged pages, rewrite
    // tables on top of the damage, and turn a database that could still have
    // been partly read into one that cannot. After that the backup is the only
    // copy of anything.
    let root = profile();
    {
        let mut store = Store::open(&root).expect("open");
        let _ = &mut store;
    }

    // Overwrite the middle of the file, past the header, so it opens and fails
    // to read. Corrupting the header would be caught by anything at all.
    let path = root.join("tessera.sqlite");
    let mut bytes = fs::read(&path).expect("read");
    assert!(bytes.len() > 8192, "the fixture needs more than one page");
    for byte in bytes.iter_mut().skip(4096).take(2048) {
        *byte = 0x5a;
    }
    fs::write(&path, &bytes).expect("write");

    let complaint = integrity(&root).expect_err("a damaged file is not ok");
    assert!(!complaint.is_empty());

    match Store::open(&root) {
        Err(StoreError::Corrupt { detail }) => assert!(!detail.is_empty()),
        Err(other) => panic!("the damage was reported as something else: {other}"),
        Ok(_) => panic!("a damaged database opened"),
    }

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn quarantine_moves_the_damaged_file_aside_and_keeps_it() {
    // Doc 10 section 15 says aside, not away. A damaged database is usually
    // still most of someone's work, and a later build may read more of it.
    let root = profile();
    {
        let mut store = Store::open(&root).expect("open");
        let _ = &mut store;
    }

    let kept = quarantine(&root).expect("quarantine");
    assert!(kept.exists(), "the damaged file was not kept");
    assert!(!root.join("tessera.sqlite").exists());
    // The side files belong to the database they were written for. Applied to a
    // restored one they would be a second corruption on top of the first.
    assert!(!root.join("tessera.sqlite-wal").exists());
    assert!(!root.join("tessera.sqlite-shm").exists());

    // And the folder now opens as a fresh profile, which is what makes a
    // restore into it possible.
    Store::open(&root).expect("the folder opens once the damaged file is aside");
    let _ = fs::remove_dir_all(&root);
}
