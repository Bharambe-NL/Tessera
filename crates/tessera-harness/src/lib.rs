//! The harness.
//!
//! Doc 12 operating principle 3: "Patterns are infrastructure. State machines,
//! hooks, failure taxonomies, and the ledger are built once in the harness and
//! reused by every agent."
//!
//! Nothing here is domain specific. Doctrine is data (principle 4), so a rule
//! about advice language or source hierarchy lives in a pack, never in this
//! crate.

pub mod agent;
pub mod failure;
pub mod hooks;
pub mod ledger;
pub mod state;

pub use agent::{Agent, AgentContext, AgentOutcome, ModelCallRecord, RunAgent, run_agent};
pub use failure::{Failure, Recovery};
pub use hooks::{Decision, Denial, Hook, HookContext, HookSet, Phase};
pub use ledger::{
    Admission, HEARTBEAT_TIMEOUT_SECONDS, Ledger, MAX_RETRIEVER_ASSIGNMENTS, MAX_RUNS_IN_FLIGHT,
    MAX_VERIFIERS_PER_BOARD, RunKind,
};
pub use state::{Machine, Status, Transition, TransitionKind, sequences};
