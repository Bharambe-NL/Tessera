//! The agent state machine base. Pattern 1.
//!
//! Every agent in the set declares one: doc 03 section 6, doc 04 section 6, doc
//! 05 section 6, doc 06 sections A6 and B6, doc 07 sections A6 and B6, doc 08
//! section 6. They differ only in the list of states, so the machine is built
//! once here and each agent supplies its own sequence.
//!
//! Three rules are shared and enforced here rather than per agent:
//!   - states advance in order, never skipping;
//!   - a state may be retried at most once (every spec says "retry (once)");
//!   - a machine that failed is finished, and any further transition is a bug.
//!
//! Doc 07 section B6 adds one exception, which the machine supports through
//! [`Machine::forbid_retry`]: the Verifier's deterministic stages never retry,
//! they either run or fail the run.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::failure::Failure;

/// A transition, recorded so a test can assert a run walked every state and so
/// the diagnostics page can show where a run stopped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub kind: TransitionKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Advance,
    Retry,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    Failed,
}

#[derive(Debug)]
pub struct MachineError {
    pub message: String,
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MachineError {}

/// A linear machine over a fixed sequence of states.
pub struct Machine {
    agent_id: String,
    states: Vec<String>,
    position: usize,
    retries: Vec<u8>,
    max_retries: u8,
    status: Status,
    history: Vec<Transition>,
}

impl Machine {
    /// `states` is the agent's own sequence, ending at its terminal state.
    /// The first entry is where the machine starts.
    pub fn new(agent_id: impl Into<String>, states: &[&str]) -> Self {
        assert!(!states.is_empty(), "a machine needs at least one state");
        Self {
            agent_id: agent_id.into(),
            states: states.iter().map(|s| (*s).to_string()).collect(),
            position: 0,
            retries: vec![0; states.len()],
            max_retries: 1,
            status: Status::Running,
            history: Vec::new(),
        }
    }

    /// Doc 07 section B6: the Verifier's deterministic stages never retry.
    pub fn forbid_retry(mut self) -> Self {
        self.max_retries = 0;
        self
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn state(&self) -> &str {
        &self.states[self.position]
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn history(&self) -> &[Transition] {
        &self.history
    }

    pub fn is_finished(&self) -> bool {
        !matches!(self.status, Status::Running)
    }

    /// The states actually entered, in order, once each. What the phase 2
    /// acceptance test ("a mock run walks every state") asserts against.
    pub fn visited(&self) -> Vec<&str> {
        let mut seen = vec![self.states[0].as_str()];
        for t in &self.history {
            if t.kind == TransitionKind::Advance && seen.last() != Some(&t.to.as_str()) {
                seen.push(&t.to);
            }
        }
        seen
    }

    /// Move to the next state.
    pub fn advance(&mut self) -> Result<&str, MachineError> {
        self.ensure_running()?;
        if self.position + 1 >= self.states.len() {
            let from = self.states[self.position].clone();
            self.status = Status::Done;
            self.history.push(Transition {
                from: from.clone(),
                to: from,
                kind: TransitionKind::Advance,
            });
            return Ok(self.state());
        }
        let from = self.states[self.position].clone();
        self.position += 1;
        self.history.push(Transition {
            from,
            to: self.states[self.position].clone(),
            kind: TransitionKind::Advance,
        });
        Ok(self.state())
    }

    /// Advance until the named state is current. Used by agents whose pipeline
    /// skips a state, for instance the Synthesizer skipping `applying_audience`
    /// when no audience is set.
    pub fn advance_to(&mut self, state: &str) -> Result<(), MachineError> {
        self.ensure_running()?;
        let Some(target) = self.states.iter().position(|s| s == state) else {
            return Err(MachineError {
                message: format!("`{state}` is not a state of {}", self.agent_id),
            });
        };
        if target < self.position {
            return Err(MachineError {
                message: format!(
                    "{} cannot go back from `{}` to `{state}`",
                    self.agent_id,
                    self.state()
                ),
            });
        }
        while self.position < target {
            self.advance()?;
        }
        Ok(())
    }

    /// Retry the current state. Returns `Err` once the budget is spent, which is
    /// the signal for the agent to take its deterministic fallback.
    pub fn retry(&mut self) -> Result<&str, MachineError> {
        self.ensure_running()?;
        if self.retries[self.position] >= self.max_retries {
            return Err(MachineError {
                message: format!("{} has no retry left in `{}`", self.agent_id, self.state()),
            });
        }
        self.retries[self.position] += 1;
        let state = self.states[self.position].clone();
        self.history.push(Transition {
            from: state.clone(),
            to: state,
            kind: TransitionKind::Retry,
        });
        Ok(self.state())
    }

    pub fn retries_used(&self) -> u8 {
        self.retries[self.position]
    }

    pub fn fail(&mut self, failure: &Failure) {
        if self.is_finished() {
            return;
        }
        let from = self.states[self.position].clone();
        self.status = Status::Failed;
        self.history.push(Transition {
            from,
            to: format!("failed:{}", failure.kind),
            kind: TransitionKind::Fail,
        });
    }

    fn ensure_running(&self) -> Result<(), MachineError> {
        match self.status {
            Status::Running => Ok(()),
            Status::Done => Err(MachineError {
                message: format!("{} already finished", self.agent_id),
            }),
            Status::Failed => Err(MachineError {
                message: format!("{} already failed", self.agent_id),
            }),
        }
    }
}

/// The state sequences from the agent specs, so the machine and the spec cannot
/// drift apart in two places.
pub mod sequences {
    /// Doc 03 section 6.
    pub const ROUTER: &[&str] = &[
        "received",
        "validating_packet",
        "classifying",
        "resolving_depth",
        "resolving_policy",
        "screening",
        "emitting",
        "done",
    ];

    /// Doc 04 section 6.
    pub const PLANNER: &[&str] = &[
        "received",
        "validating",
        "resolving_entities",
        "decomposing",
        "assigning_retrievers",
        "constraining",
        "budgeting",
        "emitting",
        "done",
    ];

    /// Doc 05 section 6.
    pub const RETRIEVER: &[&str] = &[
        "received",
        "pre_hooks",
        "querying",
        "fetching",
        "extracting",
        "chunking",
        "ranking",
        "persisting",
        "post_hooks",
        "emitting",
        "done",
    ];

    /// Doc 06 section A6.
    pub const SYNTHESIZER: &[&str] = &[
        "received",
        "validating",
        "drafting",
        "binding_citations",
        "reconciling_conflicts",
        "applying_audience",
        "summarising_structure",
        "emitting",
        "done",
    ];

    /// Doc 06 section B6.
    pub const VISUALIZER: &[&str] = &[
        "received",
        "selecting_type",
        "composing",
        "indexing_blocks",
        "sanitising",
        "emitting",
        "done",
    ];

    /// Doc 07 section A6.
    pub const READER: &[&str] = &[
        "received",
        "preprocessing",
        "recognising",
        "structuring",
        "summarising",
        "emitting",
        "done",
    ];

    /// Doc 07 section B6.
    pub const VERIFIER: &[&str] = &[
        "received",
        "validating",
        "deterministic_checks",
        "support_check",
        "visual_binding_check",
        "freshness_check",
        "doctrine_model_checks",
        "deciding",
        "emitting",
        "done",
    ];

    /// Doc 08 section 6.
    pub const EXERCISE: &[&str] = &[
        "received",
        "selecting_cards",
        "drafting",
        "checking_traceability",
        "checking_distractors",
        "emitting",
        "done",
    ];

    /// Doc 14 section 3.3. The Tutor runs once per turn, so this is its own
    /// sequence and not doc 14 section 3.4's session machine: the session is a
    /// row that outlives any one run, and the run is one decision inside it.
    pub const TUTOR: &[&str] = &[
        "received",
        "reading_session",
        "deciding",
        "checking_rules",
        "emitting",
        "done",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure::Recovery;

    #[test]
    fn a_machine_walks_its_states_in_order() {
        let mut m = Machine::new("router", sequences::ROUTER);
        assert_eq!(m.state(), "received");
        while m.state() != "done" {
            m.advance().expect("advance");
        }
        m.advance().expect("the terminal state settles");
        assert_eq!(m.status(), Status::Done);
        assert_eq!(m.visited(), sequences::ROUTER);
    }

    #[test]
    fn a_state_may_be_retried_once_and_no_more() {
        let mut m = Machine::new("synthesizer", sequences::SYNTHESIZER);
        m.advance_to("drafting").expect("to drafting");
        m.retry().expect("the one retry every spec allows");
        assert_eq!(m.retries_used(), 1);
        assert!(m.retry().is_err(), "a second retry is the signal to fall back");
    }

    #[test]
    fn the_retry_budget_is_per_state_not_per_run() {
        let mut m = Machine::new("planner", sequences::PLANNER);
        m.advance_to("decomposing").expect("to decomposing");
        m.retry().expect("retry decomposing");
        m.advance_to("budgeting").expect("to budgeting");
        m.retry().expect("a fresh budget in a new state");
    }

    #[test]
    fn the_verifiers_deterministic_stages_never_retry() {
        // Doc 07 section B6: they either run or fail the run.
        let mut m = Machine::new("verifier", sequences::VERIFIER).forbid_retry();
        m.advance_to("deterministic_checks").expect("to checks");
        assert!(m.retry().is_err());
    }

    #[test]
    fn a_machine_cannot_go_backwards() {
        let mut m = Machine::new("visualizer", sequences::VISUALIZER);
        m.advance_to("indexing_blocks").expect("forward");
        assert!(m.advance_to("composing").is_err());
    }

    #[test]
    fn a_failed_machine_is_finished() {
        let mut m = Machine::new("reader", sequences::READER);
        m.advance_to("recognising").expect("to recognising");
        m.fail(&Failure::new(
            "image_unreadable",
            "no legible content",
            Recovery::Failed,
        ));
        assert_eq!(m.status(), Status::Failed);
        assert!(m.advance().is_err());
        assert!(m.retry().is_err());

        let last = m.history().last().expect("a fail transition");
        assert_eq!(last.kind, TransitionKind::Fail);
        assert_eq!(last.to, "failed:image_unreadable");
    }

    #[test]
    fn skipping_a_state_is_recorded_as_having_visited_it() {
        // The Synthesizer skips applying_audience when no audience is set. The
        // run still passed through it, and "How this was built" says so.
        let mut m = Machine::new("synthesizer", sequences::SYNTHESIZER);
        m.advance_to("summarising_structure").expect("skip ahead");
        assert!(m.visited().contains(&"applying_audience"));
    }
}
