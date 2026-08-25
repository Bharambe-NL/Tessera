//! The work ledger and run scheduler. Patterns 27 and 28.
//!
//! Doc 10 section 6: "A single machine still needs a ledger: a table of runs
//! with `claimed_by` (worker id), `claimed_at`, `heartbeat_at`, so an app crash
//! mid research leaves a claim that the next start reclaims or marks failed
//! (liveness floor, Pattern 28). Concurrency: at most 3 runs in flight, at most
//! 6 retriever assignments in flight, one Verifier at a time per board (so batch
//! stale flags do not race)."
//!
//! The Verifier limit is the subtle one. `verify_only` runs fire in a batch when
//! a source goes stale (doc 07 section B3), and two of them on one board would
//! race to set the same card's status.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tessera_store::{Result as StoreResult, Store, new_id, now_iso8601};

/// Doc 10 section 6.
pub const MAX_RUNS_IN_FLIGHT: usize = 3;
pub const MAX_RETRIEVER_ASSIGNMENTS: usize = 6;
pub const MAX_VERIFIERS_PER_BOARD: usize = 1;

/// A run whose worker has not reported in for this long is presumed dead.
/// Long enough that a slow research run is not reclaimed out from under itself,
/// short enough that a crash is noticed on the next start.
pub const HEARTBEAT_TIMEOUT_SECONDS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Card,
    Read,
    Exercise,
    Index,
    VerifyOnly,
}

impl RunKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RunKind::Card => "card",
            RunKind::Read => "read",
            RunKind::Exercise => "exercise",
            RunKind::Index => "index",
            RunKind::VerifyOnly => "verify_only",
        }
    }
}

/// What a caller was refused, and why. The UI turns these into a wait rather
/// than an error: the run is queued, not rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admitted,
    RunsInFlight { current: usize, limit: usize },
    RetrieversInFlight { current: usize, limit: usize },
    VerifierBusy { board_id: String },
}

impl Admission {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Admission::Admitted)
    }
}

/// The in flight counters. Held beside the database rather than in it, because
/// they describe this process's work and the database describes the profile's.
#[derive(Default)]
struct InFlight {
    runs: usize,
    retriever_assignments: usize,
    verifiers_by_board: HashMap<String, usize>,
}

/// One worker's view of the ledger.
#[derive(Clone)]
pub struct Ledger {
    worker_id: String,
    in_flight: Arc<Mutex<InFlight>>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            // A fresh id per process start. A claim carrying a previous run's id
            // is exactly what marks it as abandoned.
            worker_id: format!("worker-{}", new_id()),
            in_flight: Arc::new(Mutex::new(InFlight::default())),
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// Ask whether a run may start now. Doc 10 section 6's three limits.
    pub fn admit(&self, kind: RunKind, board_id: Option<&str>) -> Admission {
        let Ok(f) = self.in_flight.lock() else {
            return Admission::RunsInFlight {
                current: MAX_RUNS_IN_FLIGHT,
                limit: MAX_RUNS_IN_FLIGHT,
            };
        };

        if kind == RunKind::VerifyOnly
            && let Some(board) = board_id
        {
            let current = f.verifiers_by_board.get(board).copied().unwrap_or(0);
            if current >= MAX_VERIFIERS_PER_BOARD {
                return Admission::VerifierBusy {
                    board_id: board.to_string(),
                };
            }
        }

        if f.runs >= MAX_RUNS_IN_FLIGHT {
            return Admission::RunsInFlight {
                current: f.runs,
                limit: MAX_RUNS_IN_FLIGHT,
            };
        }
        Admission::Admitted
    }

    /// Claim a run. Writes `claimed_by`, `claimed_at` and the first heartbeat in
    /// one statement, so a crash between them is not possible.
    pub fn claim(
        &self,
        store: &Store,
        run_id: &str,
        kind: RunKind,
        board_id: Option<&str>,
    ) -> StoreResult<()> {
        let now = now_iso8601();
        store.conn().execute(
            "UPDATE run SET claimed_by = ?1, claimed_at = ?2, heartbeat_at = ?2 WHERE id = ?3",
            params![self.worker_id, now, run_id],
        )?;
        if let Ok(mut f) = self.in_flight.lock() {
            f.runs += 1;
            if kind == RunKind::VerifyOnly
                && let Some(board) = board_id
            {
                *f.verifiers_by_board.entry(board.to_string()).or_insert(0) += 1;
            }
        }
        Ok(())
    }

    /// Report liveness. Pattern 28: the floor under "is this run still alive".
    pub fn heartbeat(&self, store: &Store, run_id: &str) -> StoreResult<()> {
        store.conn().execute(
            "UPDATE run SET heartbeat_at = ?1 WHERE id = ?2 AND claimed_by = ?3",
            params![now_iso8601(), run_id, self.worker_id],
        )?;
        Ok(())
    }

    /// Release a finished run and drop its claim.
    pub fn release(
        &self,
        store: &Store,
        run_id: &str,
        kind: RunKind,
        board_id: Option<&str>,
        status: &str,
    ) -> StoreResult<()> {
        store.conn().execute(
            "UPDATE run SET status = ?1, ended_at = ?2, claimed_by = NULL WHERE id = ?3",
            params![status, now_iso8601(), run_id],
        )?;
        if let Ok(mut f) = self.in_flight.lock() {
            f.runs = f.runs.saturating_sub(1);
            if kind == RunKind::VerifyOnly
                && let Some(board) = board_id
                && let Some(n) = f.verifiers_by_board.get_mut(board)
            {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    f.verifiers_by_board.remove(board);
                }
            }
        }
        Ok(())
    }

    /// Take one of the six retriever assignment slots, if one is free.
    pub fn try_take_retriever_slot(&self) -> Admission {
        let Ok(mut f) = self.in_flight.lock() else {
            return Admission::RetrieversInFlight {
                current: MAX_RETRIEVER_ASSIGNMENTS,
                limit: MAX_RETRIEVER_ASSIGNMENTS,
            };
        };
        if f.retriever_assignments >= MAX_RETRIEVER_ASSIGNMENTS {
            return Admission::RetrieversInFlight {
                current: f.retriever_assignments,
                limit: MAX_RETRIEVER_ASSIGNMENTS,
            };
        }
        f.retriever_assignments += 1;
        Admission::Admitted
    }

    pub fn release_retriever_slot(&self) {
        if let Ok(mut f) = self.in_flight.lock() {
            f.retriever_assignments = f.retriever_assignments.saturating_sub(1);
        }
    }

    pub fn runs_in_flight(&self) -> usize {
        self.in_flight.lock().map(|f| f.runs).unwrap_or(0)
    }

    /// Called once at startup. Doc 10 section 6: an app crash mid research
    /// leaves a claim that the next start reclaims or marks failed.
    ///
    /// Marking failed rather than resuming is deliberate. A half finished run
    /// has spent money on steps whose outputs were never validated, and the
    /// user can rerun the card knowing what they are paying for.
    pub fn reclaim_on_start(store: &mut Store) -> StoreResult<Vec<String>> {
        store.reclaim_stale_runs(HEARTBEAT_TIMEOUT_SECONDS)
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_store() -> (Store, String) {
        let mut store = Store::open_in_memory().expect("store");
        let now = now_iso8601();
        let profile = new_id();
        let pack = new_id();
        let board = new_id();
        let c = store.conn();
        c.execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                        flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0.0', '[]', '[]', '{}', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .expect("pack");
        c.execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                                  retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .expect("profile");
        c.execute(
            "INSERT INTO board (id, profile_id, title, doctrine_pack_id, default_depth, created_at, updated_at)
             VALUES (?1, ?2, 'B', ?3, 'deep', ?4, ?4)",
            params![board, profile, pack, now],
        )
        .expect("board");
        let _ = &mut store;
        (store, board)
    }

    fn insert_run(store: &Store, board: &str, heartbeat: Option<&str>) -> String {
        let id = new_id();
        store
            .conn()
            .execute(
                "INSERT INTO run (id, board_id, kind, depth, model_policy_snapshot, doctrine_pack_version,
                                  status, started_at, claimed_by, claimed_at, heartbeat_at)
                 VALUES (?1, ?2, 'card', 'deep', '{}', '1.0.0', 'running', ?3, 'worker-old', ?3, ?4)",
                params![id, board, now_iso8601(), heartbeat],
            )
            .expect("run");
        id
    }

    #[test]
    fn three_runs_fit_and_a_fourth_waits() {
        let ledger = Ledger::new();
        let (store, board) = seeded_store();
        for _ in 0..MAX_RUNS_IN_FLIGHT {
            assert!(ledger.admit(RunKind::Card, Some(&board)).is_admitted());
            let run = insert_run(&store, &board, None);
            ledger
                .claim(&store, &run, RunKind::Card, Some(&board))
                .expect("claim");
        }
        assert_eq!(ledger.runs_in_flight(), 3);
        assert!(matches!(
            ledger.admit(RunKind::Card, Some(&board)),
            Admission::RunsInFlight { limit: 3, .. }
        ));
    }

    #[test]
    fn one_verifier_at_a_time_per_board() {
        // Doc 10 section 6: so batch stale flags do not race.
        let ledger = Ledger::new();
        let (store, board) = seeded_store();
        let run = insert_run(&store, &board, None);
        ledger
            .claim(&store, &run, RunKind::VerifyOnly, Some(&board))
            .expect("claim");

        assert!(matches!(
            ledger.admit(RunKind::VerifyOnly, Some(&board)),
            Admission::VerifierBusy { .. }
        ));
        // Another board is unaffected.
        assert!(
            ledger
                .admit(RunKind::VerifyOnly, Some("other-board"))
                .is_admitted()
        );

        ledger
            .release(&store, &run, RunKind::VerifyOnly, Some(&board), "done")
            .expect("release");
        assert!(ledger.admit(RunKind::VerifyOnly, Some(&board)).is_admitted());
    }

    #[test]
    fn six_retriever_assignments_fit_and_a_seventh_waits() {
        let ledger = Ledger::new();
        for _ in 0..MAX_RETRIEVER_ASSIGNMENTS {
            assert!(ledger.try_take_retriever_slot().is_admitted());
        }
        assert!(matches!(
            ledger.try_take_retriever_slot(),
            Admission::RetrieversInFlight { limit: 6, .. }
        ));
        ledger.release_retriever_slot();
        assert!(ledger.try_take_retriever_slot().is_admitted());
    }

    #[test]
    fn a_claim_from_a_dead_worker_is_reclaimed_on_start() {
        // Doc 12 phase 2 acceptance: a crash mid run is reclaimed on restart.
        let (mut store, board) = seeded_store();
        let stale = insert_run(&store, &board, Some("2020-01-01T00:00:00.000Z"));
        let live = insert_run(&store, &board, Some(&now_iso8601()));

        let reclaimed = Ledger::reclaim_on_start(&mut store).expect("reclaim");
        assert_eq!(reclaimed, vec![stale.clone()]);

        let status = |id: &str| -> String {
            store
                .conn()
                .query_row("SELECT status FROM run WHERE id = ?1", params![id], |r| r.get(0))
                .expect("status")
        };
        assert_eq!(
            status(&stale),
            "failed",
            "an abandoned run is failed, not resumed"
        );
        assert_eq!(status(&live), "running", "a live run is left alone");

        let claimed: Option<String> = store
            .conn()
            .query_row("SELECT claimed_by FROM run WHERE id = ?1", params![stale], |r| {
                r.get(0)
            })
            .expect("claim");
        assert!(claimed.is_none(), "the dead claim must be dropped");
    }

    #[test]
    fn a_run_with_no_heartbeat_at_all_is_reclaimed() {
        let (mut store, board) = seeded_store();
        let never = insert_run(&store, &board, None);
        let reclaimed = Ledger::reclaim_on_start(&mut store).expect("reclaim");
        assert!(reclaimed.contains(&never), "a claim that never reported is dead");
    }

    #[test]
    fn a_heartbeat_only_lands_for_the_worker_holding_the_claim() {
        let (store, board) = seeded_store();
        let run = insert_run(&store, &board, Some("2020-01-01T00:00:00.000Z"));

        // A different worker cannot keep someone else's claim alive.
        let other = Ledger::new();
        other.heartbeat(&store, &run).expect("statement runs");
        let beat: Option<String> = store
            .conn()
            .query_row("SELECT heartbeat_at FROM run WHERE id = ?1", params![run], |r| {
                r.get(0)
            })
            .expect("beat");
        assert_eq!(beat.as_deref(), Some("2020-01-01T00:00:00.000Z"));
    }
}
