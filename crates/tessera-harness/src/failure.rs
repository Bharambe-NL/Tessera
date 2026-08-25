//! Failure taxonomy and evidence bundles. Patterns 4 and 14.
//!
//! Every agent spec names its failure types and the recovery for each: doc 03
//! section 10, doc 04 section 10, doc 05 section 10, doc 06 sections A10 and
//! B10, doc 07 sections A10 and B10, doc 08 section 10. The recipes only work if
//! failures arrive as categories rather than as prose, and if what was done
//! about one is recorded next to it.
//!
//! Two postures run through the set. Upstream agents are tolerant: a weak plan
//! still produces a card, and the Verifier catches what the plan missed. The
//! Verifier is strict: when it cannot decide, the card is flagged, never
//! admitted (doc 07 section B10).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What the harness did about a failure. Stored on the Step so the audit trail
/// shows the recovery, not just the fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    /// Ran the same state again. At most once, per every agent spec.
    Retried,
    /// Took the alias's fallback. Emits `model.fallback.v1`.
    FellBack,
    /// Gave up on the model and used the deterministic default. Doc 03 section
    /// 10 does this for classification and records confidence 0.2.
    DeterministicFallback,
    /// Continued with less: a dropped passage, a dropped block, a skipped rule.
    Degraded,
    /// Stopped. The card does not get answered.
    Failed,
}

/// One failure, in the shape `Step.failure` expects (doc 01 section 6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    /// A taxonomy code from the agent's own table, for example
    /// `schema_violation`, `hook_denied`, `no_passages`, `policy_unresolvable`.
    #[serde(rename = "type")]
    pub kind: String,
    pub detail: String,
    pub recovery: Recovery,
    /// Pattern 14. Attached when the cause is unknown: the packet, partial
    /// outputs, model responses, timing. Present only for `unknown`, because a
    /// bundle on every failure would put prompt text in every log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Value>,
    /// Whether the run can continue. A tolerant agent sets this true even on a
    /// failure; the Verifier sets it false.
    pub recoverable: bool,
}

impl Failure {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>, recovery: Recovery) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            recovery,
            evidence: None,
            recoverable: !matches!(recovery, Recovery::Failed),
        }
    }

    /// A failure nobody has a recipe for. Pattern 14: capture everything that
    /// would be needed to reconstruct it, because there will be no second
    /// chance to collect it.
    pub fn unknown(agent_id: &str, detail: impl Into<String>, evidence: Value) -> Self {
        Self {
            kind: "unknown".into(),
            detail: format!("{agent_id}: {}", detail.into()),
            recovery: Recovery::Failed,
            evidence: Some(evidence),
            recoverable: false,
        }
    }

    /// Doc 12 operating principle 5 and doc 07 section B10: when the Verifier
    /// cannot decide, the card is flagged rather than admitted.
    pub fn fail_closed(kind: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            recovery: Recovery::Failed,
            evidence: None,
            recoverable: false,
        }
    }

    pub fn recoverable(mut self) -> Self {
        self.recoverable = true;
        self
    }

    pub fn with_evidence(mut self, evidence: Value) -> Self {
        self.evidence = Some(evidence);
        self
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?}): {}", self.kind, self.recovery, self.detail)
    }
}

impl std::error::Error for Failure {}

impl From<tessera_providers::ProviderError> for Failure {
    fn from(e: tessera_providers::ProviderError) -> Self {
        let recovery = if e.should_try_fallback() {
            Recovery::FellBack
        } else {
            Recovery::Failed
        };
        Failure {
            kind: e.kind().to_string(),
            detail: e.to_string(),
            recovery,
            evidence: None,
            // A provider failure at one stage does not have to end the run; the
            // agent's own taxonomy decides, and it can call `recoverable()`.
            recoverable: e.is_retryable(),
        }
    }
}

impl From<tessera_schema::SchemaError> for Failure {
    fn from(e: tessera_schema::SchemaError) -> Self {
        // Every agent spec retries a schema violation once with the violation
        // attached, so the detail has to be specific enough to put in a prompt.
        let evidence = match &e {
            tessera_schema::SchemaError::Invalid { violations, .. } => serde_json::to_value(violations).ok(),
            _ => None,
        };
        Failure {
            kind: "schema_violation".into(),
            detail: e.to_string(),
            recovery: Recovery::Retried,
            evidence,
            recoverable: true,
        }
    }
}

impl From<tessera_store::StoreError> for Failure {
    fn from(e: tessera_store::StoreError) -> Self {
        // A store failure is never recoverable at the agent level: if the event
        // did not land, nothing downstream can be trusted.
        Failure {
            kind: "store".into(),
            detail: e.to_string(),
            recovery: Recovery::Failed,
            evidence: None,
            recoverable: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_recovery_is_not_recoverable_by_default() {
        let f = Failure::new("no_retriever_enabled", "no folder and no key", Recovery::Failed);
        assert!(!f.recoverable);
    }

    #[test]
    fn a_degraded_recovery_lets_the_run_continue() {
        // Doc 05 section 10: a retriever may return nothing and the card still
        // gets answered from the other assignments.
        let f = Failure::new("empty_result", "no passages", Recovery::Degraded);
        assert!(f.recoverable);
    }

    #[test]
    fn a_schema_violation_carries_its_violations_as_evidence() {
        let registry = tessera_schema::Registry::load().expect("registry");
        let err = registry
            .validate(tessera_schema::ids::OUT_ROUTER, &serde_json::json!({}))
            .expect_err("empty output is invalid");
        let f: Failure = err.into();
        assert_eq!(f.kind, "schema_violation");
        assert_eq!(f.recovery, Recovery::Retried);
        assert!(f.evidence.is_some(), "the retry prompt needs the violations");
        assert!(f.recoverable);
    }

    #[test]
    fn a_missing_key_is_not_retryable() {
        let e = tessera_providers::ProviderError::NoKey {
            provider: "anthropic".into(),
            alias: "frontier".into(),
        };
        let f: Failure = e.into();
        assert_eq!(f.kind, "policy_unresolvable");
        assert!(!f.recoverable, "retrying a missing key wastes the user's time");
    }

    #[test]
    fn the_step_shape_serialises_type_not_kind() {
        // Doc 01 section 6.2: Step.failure is {type, detail, recovery}.
        let f = Failure::new("model_timeout", "over 2.5 s", Recovery::DeterministicFallback);
        let v = serde_json::to_value(&f).expect("serialise");
        assert_eq!(v["type"], "model_timeout");
        assert_eq!(v["recovery"], "deterministic_fallback");
        assert!(
            v.get("evidence").is_none(),
            "no bundle unless the cause is unknown"
        );
    }
}
