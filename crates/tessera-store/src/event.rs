//! The event envelope. Doc 01 section 6.3.
//!
//! Every action in Tessera is an event, including the user's own. That is what
//! makes "who changed what" answerable without a second mechanism, and it is why
//! `emitter_type` has a `user` member.

use serde::{Deserialize, Serialize};

/// Where an event came from. Doc 01 section 6.3.
///
/// `test` and `replay` are filtered out of policy hooks by default (doc 10
/// section 5), so an eval run cannot trip a rule meant for live work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Live,
    Test,
    Replay,
    Healthcheck,
    Harness,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Live => "live",
            Source::Test => "test",
            Source::Replay => "replay",
            Source::Healthcheck => "healthcheck",
            Source::Harness => "harness",
        }
    }

    /// Doc 01 section 6.3: policy checks do not fire on replay, and doc 10
    /// section 5 extends that to test provenance.
    pub fn fires_policy_hooks(self) -> bool {
        matches!(self, Source::Live | Source::Healthcheck)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmitterType {
    Agent,
    Harness,
    User,
    Retriever,
}

impl EmitterType {
    pub fn as_str(self) -> &'static str {
        match self {
            EmitterType::Agent => "agent",
            EmitterType::Harness => "harness",
            EmitterType::User => "user",
            EmitterType::Retriever => "retriever",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    Verified,
    Unverified,
    Degraded,
}

impl TrustLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustLevel::Verified => "verified",
            TrustLevel::Unverified => "unverified",
            TrustLevel::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: Source,
    pub emitter_id: String,
    pub emitter_type: EmitterType,
    pub run_id: Option<String>,
    pub trust_level: TrustLevel,
}

impl Provenance {
    /// An event the user caused. The trust level is `verified` because the user
    /// is the authority on their own actions.
    pub fn user() -> Self {
        Self {
            source: Source::Live,
            emitter_id: "user".into(),
            emitter_type: EmitterType::User,
            run_id: None,
            trust_level: TrustLevel::Verified,
        }
    }

    pub fn harness(emitter_id: impl Into<String>, run_id: Option<String>) -> Self {
        Self {
            source: Source::Live,
            emitter_id: emitter_id.into(),
            emitter_type: EmitterType::Harness,
            run_id,
            trust_level: TrustLevel::Verified,
        }
    }

    /// An agent's output before the Verifier has seen it is `unverified` by
    /// definition. The Verifier is the only thing that raises it.
    pub fn agent(agent_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            source: Source::Live,
            emitter_id: agent_id.into(),
            emitter_type: EmitterType::Agent,
            run_id: Some(run_id.into()),
            trust_level: TrustLevel::Unverified,
        }
    }

    /// A retriever. Doc 01 section 6.3 makes this its own emitter type, and it
    /// earns the distinction: retrievers are the only agents that touch data
    /// from outside the profile, so "which of these events came from something
    /// that reached out" is a question the audit trail has to answer directly
    /// rather than by knowing which agent ids happen to be retrievers.
    ///
    /// Unverified like any other agent output. What a retriever found is
    /// evidence, and evidence is not a verdict.
    pub fn retriever(retriever_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            source: Source::Live,
            emitter_id: retriever_id.into(),
            emitter_type: EmitterType::Retriever,
            run_id: Some(run_id.into()),
            trust_level: TrustLevel::Unverified,
        }
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    pub fn with_trust(mut self, trust_level: TrustLevel) -> Self {
        self.trust_level = trust_level;
        self
    }
}

/// An event ready to append. `monotonic_index` is assigned by the store, inside
/// the same transaction as the insert, so it is absent here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvent {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub provenance: Provenance,
    pub causal_parent_id: Option<String>,
    /// Denormalised so the board's history and a card's own events are one index
    /// scan rather than a payload search.
    pub board_id: Option<String>,
    pub card_id: Option<String>,
}

impl NewEvent {
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value, provenance: Provenance) -> Self {
        Self {
            event_type: event_type.into(),
            payload,
            provenance,
            causal_parent_id: None,
            board_id: None,
            card_id: None,
        }
    }

    pub fn on_board(mut self, board_id: impl Into<String>) -> Self {
        self.board_id = Some(board_id.into());
        self
    }

    pub fn on_card(mut self, card_id: impl Into<String>) -> Self {
        self.card_id = Some(card_id.into());
        self
    }

    pub fn caused_by(mut self, event_id: impl Into<String>) -> Self {
        self.causal_parent_id = Some(event_id.into());
        self
    }
}

/// A stored event, as read back from the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: String,
    pub monotonic_index: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub provenance: Provenance,
    pub causal_parent_id: Option<String>,
    pub board_id: Option<String>,
    pub card_id: Option<String>,
    pub timestamp: String,
}

/// The v1 event vocabulary. Doc 01 section 6.3, plus the nine names doc 13
/// records as missing from it and the seven Learn mode names from doc 14
/// section 2 (BN-009).
///
/// The list exists so the schema guard can reject an event type nobody
/// declared, which is how a typo in an agent becomes a failure rather than a
/// silently unreadable audit trail.
pub const EVENT_VOCABULARY: &[&str] = &[
    // board
    "board.created.v1",
    "board.renamed.v1",
    "board.trashed.v1",
    "board.restored.v1",
    "board.purged.v1",
    "board.pack_updated.v1",
    "board.exported.v1",
    "board.imported.v1",
    // card lifecycle
    "card.requested.v1",
    "card.routed.v1",
    "card.planned.v1",
    "card.synthesized.v1",
    "card.answered.v1",
    "card.rerun.v1",
    "card.failed.v1",
    "card.superseded.v1",
    "card.blocked.v1",
    // retrieval
    "retrieval.started.v1",
    "retrieval.completed.v1",
    "source.created.v1",
    "source.deduplicated.v1",
    "source.stale.v1",
    "source.proposed.v1",
    // visual and citation
    "visual.produced.v1",
    "visual.sanitised.v1",
    "visual.declined.v1",
    "citation.bound.v1",
    "citation.verdict.v1",
    // verification and review
    "verify.completed.v1",
    "flag.raised.v1",
    "review.decided.v1",
    // concepts
    "concept.proposed.v1",
    "concept.confirmed.v1",
    "concept.linked.v1",
    "entity.resolved.v1",
    // material
    "image.pasted.v1",
    "image.generated.v1",
    "sketch.rasterised.v1",
    "read.completed.v1",
    "ink.added.v1",
    "ink.erased.v1",
    "note.added.v1",
    "note.edited.v1",
    // vault pages, doc 16 section 4. The sticky above is a board object and a
    // page is a vault object; doc 16 section 7 point 1 keeps the UI words
    // apart too, "sticky" and "page".
    "page.created.v1",
    "page.created_from_card.v1",
    "page.edited.v1",
    "page.renamed.v1",
    "page.deleted.v1",
    "page.link_resolved.v1",
    "page.link_unresolved.v1",
    // the notebook, doc 16 section 3.4
    "notebook.asked.v1",
    "notebook.grounding.v1",
    // exercise
    "exercise.generated.v1",
    "attempt.recorded.v1",
    "exercise.item_reported.v1",
    // doctrine packs, doc 10 section 9
    "pack.imported.v1",
    "pack.activated.v1",
    // index
    "index.folder_added.v1",
    "index.updated.v1",
    "index.folder_removed.v1",
    // harness
    "model.call.v1",
    "model.fallback.v1",
    "schema.violation.v1",
    "hook.denied.v1",
    "context.stale_noted.v1",
    "run.compacted.v1",
    // learn mode, doc 14 section 2
    "learn.started.v1",
    "learn.intake_answered.v1",
    "learn.planned.v1",
    "learn.check_asked.v1",
    "learn.check_answered.v1",
    "learn.card_opened.v1",
    "learn.ended.v1",
];

pub fn is_known_event_type(t: &str) -> bool {
    EVENT_VOCABULARY.contains(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for t in EVENT_VOCABULARY {
            assert!(seen.insert(*t), "duplicate event type in the vocabulary: {t}");
        }
    }

    #[test]
    fn every_event_type_is_versioned() {
        for t in EVENT_VOCABULARY {
            assert!(t.ends_with(".v1"), "{t} carries no version suffix");
            assert!(t.split('.').count() >= 3, "{t} is not <domain>.<name>.<version>");
        }
    }

    #[test]
    fn replay_and_test_do_not_fire_policy_hooks() {
        // Doc 01 section 6.3 and doc 10 section 5.
        assert!(!Source::Replay.fires_policy_hooks());
        assert!(!Source::Test.fires_policy_hooks());
        assert!(Source::Live.fires_policy_hooks());
    }
}
