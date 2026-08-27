//! Storage for one profile: SQLite, the blob store, and the event log.
//!
//! Doc 10 section 1: all state lives on the user's machine. Doc 10 section 15:
//! the profile folder is the unit of backup, restore, and "open profile from
//! folder", which is why the database and the blob directory live side by side
//! under one root.

pub mod blob;
pub mod error;
pub mod event;
pub mod projection;
pub mod repo;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use ulid::Ulid;

pub use crate::blob::BlobStore;
pub use crate::error::{Result, StoreError};
pub use crate::event::{
    EVENT_VOCABULARY, EmitterType, Event, NewEvent, Provenance, Source, TrustLevel, is_known_event_type,
};

/// Bumped by one per migration file. `PRAGMA user_version` carries it.
///
/// Public because the diagnostics export reports it and because a test that
/// hard codes the number has to be edited by every migration that follows.
pub const SCHEMA_VERSION: i32 = 4;

const MIGRATIONS: &[(i32, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
    (2, include_str!("../migrations/0002_memory.sql")),
    (3, include_str!("../migrations/0003_index_vectors.sql")),
    (4, include_str!("../migrations/0004_vault_enums.sql")),
];

pub struct Store {
    conn: Connection,
    blobs: BlobStore,
    root: PathBuf,
}

impl Store {
    /// Open (or create) a profile at `root`. The database is `tessera.sqlite`
    /// and the blobs are in `blobs/`, so the whole profile is one folder.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;

        // Doc 10 section 15: detected on start. Before the migrations run,
        // because a migration against a damaged page file is how a database
        // that could still have been read becomes one that cannot: it would
        // rewrite tables on top of the damage and the backup would then be the
        // only copy of anything.
        //
        // Nothing is moved here. `quarantine` is a separate call the shell
        // makes after telling the person what it found, because a start that
        // silently renamed someone's work and carried on is the behaviour a
        // person would least expect and could least undo.
        // No database yet is the first run, not a fault, so the check only
        // speaks about a file that is already there.
        if root.join("tessera.sqlite").exists()
            && let Err(detail) = integrity(&root)
        {
            return Err(StoreError::Corrupt { detail });
        }

        let conn = Connection::open(root.join("tessera.sqlite"))?;
        Self::from_parts(conn, BlobStore::open(root.join("blobs"))?, root)
    }

    /// An in memory profile, for tests and for the eval harness.
    pub fn open_in_memory() -> Result<Self> {
        let root = std::env::temp_dir().join(format!("tessera-mem-{}", Ulid::generate()));
        let conn = Connection::open_in_memory()?;
        Self::from_parts(conn, BlobStore::open(root.join("blobs"))?, root)
    }

    fn from_parts(conn: Connection, blobs: BlobStore, root: PathBuf) -> Result<Self> {
        // WAL so a reader never blocks the writer; doc 01 section 8.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;

        let mut store = Self { conn, blobs, root };
        store.migrate()?;
        Ok(store)
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Escape hatch for the repository layer. Every write that must be atomic
    /// with an event goes through [`Store::append_with`] instead.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ------------------------------------------------------------ migrate --

    fn migrate(&mut self) -> Result<()> {
        let found: i32 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

        if found > SCHEMA_VERSION {
            // A newer Tessera wrote this profile. Refusing beats corrupting it.
            return Err(StoreError::SchemaVersion {
                found,
                expected: SCHEMA_VERSION,
            });
        }

        if found == SCHEMA_VERSION {
            return Ok(());
        }

        // Foreign keys go off for the duration.
        //
        // SQLite cannot widen a CHECK constraint in place, so a migration that
        // adds an enum value has to rebuild the table, which means dropping it.
        // Dropping a parent table with foreign keys on runs an implicit delete
        // that fires ON DELETE CASCADE: 0002 drops `source`, and `passage`
        // cascades from it, so every passage in the profile would go with it.
        // Deferring is not enough, because deferring delays the violation check
        // and not the cascade itself.
        //
        // The pragma is a no-op inside a transaction, so it has to sit outside
        // the loop rather than at the top of a migration file. `foreign_key_check`
        // below is what earns the right to turn them back on.
        self.conn.pragma_update(None, "foreign_keys", "OFF")?;
        let outcome = Self::run_migrations(&mut self.conn, found);
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        outcome
    }

    fn run_migrations(conn: &mut Connection, found: i32) -> Result<()> {
        for (version, sql) in MIGRATIONS {
            if *version <= found {
                continue;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;

            // Step 10 of the rebuild procedure. A rebuild that copied rows into
            // the replacement but missed one would leave a dangling reference
            // that nothing notices until a much later read, so it is caught here
            // and rolled back with the rest of the migration.
            let broken: i64 =
                tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
            if broken > 0 {
                return Err(StoreError::MigrationBrokeReferences {
                    version: *version,
                    rows: broken,
                });
            }

            tx.pragma_update(None, "user_version", *version)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i32> {
        Ok(self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    // -------------------------------------------------------------- events --

    /// Append one event and fold it into the projections, in one transaction.
    ///
    /// Doc 10 section 4: "projections are updated in the same transaction as the
    /// event write so a crash cannot leave them apart."
    pub fn append(&mut self, ev: NewEvent) -> Result<Event> {
        self.append_with(ev, |_| Ok(()))
    }

    /// Append an event, run `writes` in the same transaction, and fold the event
    /// into the projections. This is how an entity row and the event announcing
    /// it stay atomic: a Visual row and its `visual.produced.v1` either both
    /// land or neither does.
    pub fn append_with<F>(&mut self, ev: NewEvent, writes: F) -> Result<Event>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<()>,
    {
        if !is_known_event_type(&ev.event_type) {
            return Err(StoreError::UnknownEventType(ev.event_type));
        }

        let event_id = Ulid::generate().to_string();
        let timestamp = now_iso8601();
        let payload = serde_json::to_string(&ev.payload)?;

        let tx = self.conn.transaction()?;

        // The sequence lives in a row rather than in AUTOINCREMENT so it is
        // claimed under this transaction's lock: two writers cannot take the
        // same index, and a rolled back append does not burn one.
        let index: i64 = tx.query_row("SELECT next FROM event_sequence WHERE id = 1", [], |r| r.get(0))?;
        tx.execute(
            "UPDATE event_sequence SET next = ?1 WHERE id = 1",
            params![index + 1],
        )?;

        writes(&tx)?;

        tx.execute(
            "INSERT INTO event (
                event_id, monotonic_index, event_type, payload,
                source, emitter_id, emitter_type, run_id, trust_level,
                causal_parent_id, board_id, card_id, timestamp
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                event_id,
                index,
                ev.event_type,
                payload,
                ev.provenance.source.as_str(),
                ev.provenance.emitter_id,
                ev.provenance.emitter_type.as_str(),
                ev.provenance.run_id,
                ev.provenance.trust_level.as_str(),
                ev.causal_parent_id,
                ev.board_id,
                ev.card_id,
                timestamp,
            ],
        )?;

        projection::apply(
            &tx,
            &projection::Projected {
                event_type: &ev.event_type,
                payload: &ev.payload,
                card_id: ev.card_id.as_deref(),
                run_id: ev.provenance.run_id.as_deref(),
                timestamp: &timestamp,
            },
        )?;

        tx.commit()?;

        Ok(Event {
            event_id,
            monotonic_index: index,
            event_type: ev.event_type,
            payload: ev.payload,
            provenance: ev.provenance,
            causal_parent_id: ev.causal_parent_id,
            board_id: ev.board_id,
            card_id: ev.card_id,
            timestamp,
        })
    }

    /// Every event, in order. The board history surface (doc 09 section 12) and
    /// the audit exporter read this.
    pub fn events(&self, board_id: Option<&str>) -> Result<Vec<Event>> {
        let sql = match board_id {
            Some(_) => {
                "SELECT event_id, monotonic_index, event_type, payload, source, emitter_id,
                        emitter_type, run_id, trust_level, causal_parent_id, board_id, card_id, timestamp
                 FROM event WHERE board_id = ?1 ORDER BY monotonic_index ASC"
            }
            None => {
                "SELECT event_id, monotonic_index, event_type, payload, source, emitter_id,
                        emitter_type, run_id, trust_level, causal_parent_id, board_id, card_id, timestamp
                 FROM event ORDER BY monotonic_index ASC"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Event> {
            let payload: String = r.get(3)?;
            Ok(Event {
                event_id: r.get(0)?,
                monotonic_index: r.get(1)?,
                event_type: r.get(2)?,
                payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                provenance: Provenance {
                    source: parse_source(&r.get::<_, String>(4)?),
                    emitter_id: r.get(5)?,
                    emitter_type: parse_emitter(&r.get::<_, String>(6)?),
                    run_id: r.get(7)?,
                    trust_level: parse_trust(&r.get::<_, String>(8)?),
                },
                causal_parent_id: r.get(9)?,
                board_id: r.get(10)?,
                card_id: r.get(11)?,
                timestamp: r.get(12)?,
            })
        };
        let rows = match board_id {
            Some(b) => stmt
                .query_map(params![b], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }

    pub fn event_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))?)
    }

    /// Throw away every projection and fold the whole log back over them.
    /// The debugging path for a bad card, and the M1 acceptance test.
    pub fn rebuild_projections(&mut self) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let n = projection::rebuild(&tx)?;
        tx.commit()?;
        Ok(n)
    }

    // ------------------------------------------------------------- ledger --

    /// Reclaim runs whose worker died. Doc 10 section 6: an app crash mid
    /// research leaves a claim that the next start reclaims or marks failed.
    ///
    /// Returns the run ids that were failed.
    pub fn reclaim_stale_runs(&mut self, stale_after_seconds: i64) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;
        let ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM run
                 WHERE status = 'running'
                   AND (heartbeat_at IS NULL
                        OR CAST((julianday('now') - julianday(heartbeat_at)) * 86400 AS INTEGER) > ?1)",
            )?;
            stmt.query_map(params![stale_after_seconds], |r| r.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for id in &ids {
            tx.execute(
                "UPDATE run SET status = 'failed', ended_at = ?1, claimed_by = NULL WHERE id = ?2",
                params![now_iso8601(), id],
            )?;
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn board_exists(&self, board_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM board WHERE id = ?1", params![board_id], |_| Ok(()))
            .optional()?
            .is_some())
    }
}

fn parse_source(s: &str) -> Source {
    match s {
        "test" => Source::Test,
        "replay" => Source::Replay,
        "healthcheck" => Source::Healthcheck,
        "harness" => Source::Harness,
        _ => Source::Live,
    }
}

fn parse_emitter(s: &str) -> EmitterType {
    match s {
        "harness" => EmitterType::Harness,
        "user" => EmitterType::User,
        "retriever" => EmitterType::Retriever,
        _ => EmitterType::Agent,
    }
}

fn parse_trust(s: &str) -> TrustLevel {
    match s {
        "verified" => TrustLevel::Verified,
        "degraded" => TrustLevel::Degraded,
        _ => TrustLevel::Unverified,
    }
}

// ------------------------------------------------------------- integrity --
//
// Doc 10 section 15: "A corrupted SQLite file is detected on start; the app
// offers restore from the last backup and keeps the damaged file aside." Both
// halves live here rather than beside the backup writer, because `Store::open`
// is what has to call the first one and it cannot reach a crate that depends
// on it.

/// Whether the database at `root` is readable, and what SQLite says if not.
///
/// Doc 10 section 15: detected on start. `PRAGMA integrity_check` reads every
/// page, which is the point: an opened handle proves the header parsed and
/// nothing else.
pub fn integrity(root: &Path) -> std::result::Result<(), String> {
    let path = root.join("tessera.sqlite");
    if !path.exists() {
        return Err("there is no database in this folder".into());
    }
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    let verdict: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if verdict == "ok" { Ok(()) } else { Err(verdict) }
}

/// Move a damaged database aside and return where it went.
///
/// Doc 10 section 15: "keeps the damaged file aside". Renamed rather than
/// deleted, because a damaged database is usually still most of someone's work
/// and a later build may read more of it than this one can.
pub fn quarantine(root: &Path) -> Result<PathBuf> {
    let from = root.join("tessera.sqlite");
    let to = root.join(format!(
        "tessera.damaged-{}.sqlite",
        now_iso8601().replace([':', '.'], "-")
    ));
    std::fs::rename(&from, &to)?;
    // The side files belong to the database they were written for. Left behind
    // they would be applied to whatever is restored in its place, which is a
    // second corruption on top of the first.
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(root.join(format!("tessera.sqlite{suffix}")));
    }
    Ok(to)
}

/// ISO 8601 with offset, per doc 01 section 3.
pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    Ulid::generate().to_string()
}
