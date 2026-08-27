#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! M4a acceptance: a profile written by schema version 1 upgrades to version 2
//! without losing a row.
//!
//! This is the first migration that runs on top of another, and it is the first
//! that rebuilds a table. Both matter more than the four fields it adds. SQLite
//! cannot widen a `CHECK` constraint in place, so adding `own_card` to the
//! source class means dropping and recreating `source`, and `passage.source_id`
//! cascades on delete. Get the foreign key handling wrong and the migration
//! quietly deletes every passage in the profile, which is every citation's
//! target, which is the entire audit trail.
//!
//! So the test populates a version 1 profile, migrates it, and counts.

use rusqlite::{Connection, params};
use tessera_store::{Store, new_id, now_iso8601};

const V1: &str = include_str!("../migrations/0001_initial.sql");

fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("tessera-migration-{}", new_id()))
}

struct Ids {
    profile: String,
    board: String,
    card: String,
    source: String,
    passage: String,
    concept: String,
}

/// Write a profile at schema version 1, exactly as a build from before this
/// migration existed would have left it.
fn write_v1_profile(root: &std::path::Path) -> Ids {
    std::fs::create_dir_all(root).expect("root");
    let conn = Connection::open(root.join("tessera.sqlite")).expect("open");
    conn.pragma_update(None, "foreign_keys", "ON").expect("fk");
    conn.execute_batch(V1).expect("0001 applies");
    conn.pragma_update(None, "user_version", 1).expect("version");

    let now = now_iso8601();
    let ids = Ids {
        profile: new_id(),
        board: new_id(),
        card: new_id(),
        source: new_id(),
        passage: new_id(),
        concept: new_id(),
    };
    let pack = new_id();

    conn.execute(
        "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
             freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
         VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
        params![pack, now],
    )
    .expect("pack");

    conn.execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
             retriever_config, created_at, updated_at)
         VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
        params![ids.profile, pack, now],
    )
    .expect("profile");

    conn.execute(
        "INSERT INTO board (id, profile_id, title, doctrine_pack_id, default_depth, created_at, updated_at)
         VALUES (?1, ?2, 'A board from before the migration', ?3, 'deep', ?4, ?4)",
        params![ids.board, ids.profile, pack, now],
    )
    .expect("board");

    conn.execute(
        "INSERT INTO card (id, board_id, kind, question, depth, status, created_at, updated_at)
         VALUES (?1, ?2, 'root', 'What did this profile know already?', 'deep', 'done', ?3, ?3)",
        params![ids.card, ids.board, now],
    )
    .expect("card");

    conn.execute(
        "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
             freshness_class, trust_rank, dedupe_key, created_at)
         VALUES (?1, ?2, 'regulatory', 'A consolidated text', 'https://example.invalid/a',
                 ?3, 'stable', 1, 'example.invalid/a', ?3)",
        params![ids.source, ids.profile, now],
    )
    .expect("source");

    // The row the cascade would take. Doc 01 section 4.9.
    conn.execute(
        "INSERT INTO passage (id, source_id, text, retrieved_by, created_at)
         VALUES (?1, ?2, 'The passage a citation points at.', 'regulatory', ?3)",
        params![ids.passage, ids.source, now],
    )
    .expect("passage");

    conn.execute(
        "INSERT INTO citation (id, card_id, ordinal, passage_id, claim_span, binding, created_at)
         VALUES (?1, ?2, 1, ?3, '[0,10]', 'answer', ?4)",
        params![new_id(), ids.card, ids.passage, now],
    )
    .expect("citation");

    conn.execute(
        "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, created_at, updated_at)
         VALUES (?1, ?2, 'Own funds', ?3, ?4, ?4)",
        params![ids.concept, ids.profile, pack, now],
    )
    .expect("concept");

    conn.execute(
        "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation, proposed_by, created_at)
         VALUES (?1, ?2, 'card', ?3, 'explains', 'user', ?4)",
        params![new_id(), ids.concept, ids.card, now],
    )
    .expect("concept link");

    ids
}

#[test]
fn a_version_one_profile_upgrades_without_losing_a_row() {
    let root = temp_root();
    let ids = write_v1_profile(&root);

    let store = Store::open(&root).expect("migrate on open");
    assert_eq!(
        store.schema_version().expect("version"),
        tessera_store::SCHEMA_VERSION
    );

    let conn = store.conn();
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    };

    // The one that would break silently. Dropping `source` with foreign keys on
    // cascades into `passage`, and every citation points at a passage.
    assert_eq!(count("passage"), 1, "the cascade took the passages");
    assert_eq!(count("citation"), 1, "the citations went with them");
    assert_eq!(count("source"), 1, "the rebuild lost the source rows");
    assert_eq!(count("concept_link"), 1, "the rebuild lost the links");

    // Rebuilt tables keep their content, not just their row count.
    let title: String = conn
        .query_row(
            "SELECT title FROM source WHERE id = ?1",
            params![ids.source],
            |r| r.get(0),
        )
        .expect("source row");
    assert_eq!(title, "A consolidated text");

    let relation: String = conn
        .query_row("SELECT relation FROM concept_link LIMIT 1", [], |r| r.get(0))
        .expect("link row");
    assert_eq!(relation, "explains");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_new_fields_arrive_with_the_defaults_doc_01_states() {
    let root = temp_root();
    let ids = write_v1_profile(&root);
    let store = Store::open(&root).expect("migrate");
    let conn = store.conn();

    // Doc 01 section 4.4: empty for a card that used no prior work.
    let builds_on: String = conn
        .query_row(
            "SELECT builds_on FROM card WHERE id = ?1",
            params![ids.card],
            |r| r.get(0),
        )
        .expect("card row");
    assert_eq!(builds_on, "[]");

    // Doc 01 section 4.16: "Boards retriever on by default."
    let enabled: i64 = conn
        .query_row(
            "SELECT memory_enabled FROM profile WHERE id = ?1",
            params![ids.profile],
            |r| r.get(0),
        )
        .expect("profile row");
    assert_eq!(enabled, 1);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_widened_checks_accept_the_new_values_and_still_reject_nonsense() {
    let root = temp_root();
    let ids = write_v1_profile(&root);
    let store = Store::open(&root).expect("migrate");
    let conn = store.conn();
    let now = now_iso8601();

    // Doc 05 section 8.5: a prior card enters as a source of this class.
    conn.execute(
        "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
             freshness_class, trust_rank, dedupe_key, created_at)
         VALUES (?1, ?2, 'own_card', 'A prior card', 'board:1/card:2', ?3, 'stable', 5, 'own:1/2', ?3)",
        params![new_id(), ids.profile, now],
    )
    .expect("own_card is accepted");

    conn.execute(
        "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation, proposed_by, created_at)
         VALUES (?1, ?2, 'card', ?3, 'builds_on', 'harness', ?4)",
        params![new_id(), ids.concept, ids.card, now],
    )
    .expect("builds_on is accepted");

    // The rebuild kept the constraint rather than dropping it, which is the
    // failure mode of a copy and paste table rebuild.
    let bad = conn.execute(
        "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
             freshness_class, trust_rank, dedupe_key, created_at)
         VALUES (?1, ?2, 'hearsay', 'Nowhere', 'x', ?3, 'stable', 9, 'x', ?3)",
        params![new_id(), ids.profile, now],
    );
    assert!(bad.is_err(), "the class check did not survive the rebuild");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn migrating_twice_is_a_no_op() {
    let root = temp_root();
    write_v1_profile(&root);

    let first = Store::open(&root).expect("first open");
    let schema_after_first = schema_of(first.conn());
    drop(first);

    let second = Store::open(&root).expect("second open");
    assert_eq!(
        second.schema_version().expect("version"),
        tessera_store::SCHEMA_VERSION
    );
    assert_eq!(
        schema_of(second.conn()),
        schema_after_first,
        "reopening ran a migration that had already run"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_upgraded_profile_matches_one_built_from_scratch() {
    let upgraded_root = temp_root();
    write_v1_profile(&upgraded_root);
    let upgraded = Store::open(&upgraded_root).expect("upgrade");

    let fresh_root = temp_root();
    let fresh = Store::open(&fresh_root).expect("fresh");

    assert_eq!(
        schema_of(upgraded.conn()),
        schema_of(fresh.conn()),
        "an upgraded profile and a new one disagree about the schema"
    );

    std::fs::remove_dir_all(&upgraded_root).ok();
    std::fs::remove_dir_all(&fresh_root).ok();
}

/// Every object in the schema, normalised.
///
/// A table that reached its name through `ALTER TABLE ... RENAME TO` is stored
/// with the name quoted, so `CREATE TABLE "source"` and `CREATE TABLE source`
/// describe the same table and compare unequal. Quotes and runs of whitespace
/// come out; nothing else does.
fn schema_of(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT name, COALESCE(sql, '') FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare");
    stmt.query_map([], |r| {
        let name: String = r.get(0)?;
        let sql: String = r.get(1)?;
        Ok((
            name,
            sql.replace('"', "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ))
    })
    .expect("query")
    .collect::<Result<Vec<_>, _>>()
    .expect("rows")
}

#[test]
fn the_vault_enum_values_are_accepted_before_anything_writes_them() {
    // Doc 16 section 4's three CHECK widenings, landed early on purpose. The
    // enum value costs nothing; the table rebuild it needs is what BN-028
    // showed to be dangerous, and doing it once beats doing it twice.
    let root = temp_root();
    let ids = write_v1_profile(&root);
    let store = Store::open(&root).expect("migrate");
    let conn = store.conn();
    let now = now_iso8601();

    // Doc 16 section 3.3: a page is a source of its own class.
    conn.execute(
        "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
             freshness_class, trust_rank, dedupe_key, created_at)
         VALUES (?1, ?2, 'page', 'A page', 'vault/a-page.md', ?3, 'stable', 4, 'vault/a-page', ?3)",
        params![new_id(), ids.profile, now],
    )
    .expect("page is accepted as a source class");

    // Doc 16 section 3.5: two visual types the tree cannot express.
    for visual_type in ["flow", "stats"] {
        conn.execute(
            "INSERT INTO visual (id, card_id, type, title, payload, block_index, created_at)
             VALUES (?1, ?2, ?3, 'A visual', '{}', '{}', ?4)",
            params![new_id(), ids.card, visual_type, now],
        )
        .unwrap_or_else(|e| panic!("{visual_type} was rejected: {e}"));
    }

    // Doc 16 section 3.4: notebook sessions are boards, so they inherit
    // history, events, memory and export.
    conn.execute(
        "UPDATE board SET mode = 'notebook' WHERE id = ?1",
        params![ids.board],
    )
    .expect("notebook is accepted as a board mode");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_widened_checks_still_refuse_what_they_always_refused() {
    // A rebuild that quietly dropped a constraint would look exactly like a
    // rebuild that widened it, right up until the first bad row.
    let root = temp_root();
    let ids = write_v1_profile(&root);
    let store = Store::open(&root).expect("migrate");
    let conn = store.conn();
    let now = now_iso8601();

    assert!(
        conn.execute(
            "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
                 freshness_class, trust_rank, dedupe_key, created_at)
             VALUES (?1, ?2, 'rumour', 'Nowhere', 'x', ?3, 'stable', 9, 'x', ?3)",
            params![new_id(), ids.profile, now],
        )
        .is_err(),
        "the source class check did not survive the rebuild"
    );

    assert!(
        conn.execute(
            "INSERT INTO visual (id, card_id, type, title, payload, block_index, created_at)
             VALUES (?1, ?2, 'interpretive_dance', 'A visual', '{}', '{}', ?3)",
            params![new_id(), ids.card, now],
        )
        .is_err(),
        "the visual type check did not survive the rebuild"
    );

    assert!(
        conn.execute(
            "UPDATE board SET mode = 'freestyle' WHERE id = ?1",
            params![ids.board]
        )
        .is_err(),
        "the board mode check did not survive the rebuild"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_rebuilt_table_keeps_the_rows_that_pointed_into_it() {
    // Two rebuilds in one migration, and `passage.source_id` cascades. This is
    // the failure BN-028 caught the first time and the reason it is worth
    // catching again with every new rebuild.
    let root = temp_root();
    write_v1_profile(&root);
    let store = Store::open(&root).expect("migrate");
    let conn = store.conn();

    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    };
    assert_eq!(count("passage"), 1, "the source rebuild took the passages");
    assert_eq!(count("citation"), 1, "the citations went with them");
    assert_eq!(count("card"), 1, "the board rebuild took the cards");
    assert_eq!(count("board"), 1, "the board rebuild lost the board");

    std::fs::remove_dir_all(&root).ok();
}
