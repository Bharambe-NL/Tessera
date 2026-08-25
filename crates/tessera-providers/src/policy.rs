//! Model policy and alias resolution. Pattern 21.
//!
//! Doc 01 section 5 defines the policy; doc 03 section 8.3 defines how it
//! resolves: "Deterministic merge, most specific wins: profile policy, then
//! board `default_model_policy_id`, then `request.model_override`. Aliases that
//! name a provider with no active key resolve to their fallback list; if no
//! fallback has a key, the Router fails with `policy_unresolvable` rather than
//! guessing. The resolved map is snapshotted onto the Run."
//!
//! Aliases decouple the stage from the provider, which is what makes a model
//! swap a Profile edit rather than a code change, and what makes doc 10 section
//! 10's fully offline configuration possible through an Ollama alias.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{ProviderError, Result};
use crate::keychain::KeyStore;
use crate::model::Effort;

/// The pipeline stages a policy assigns a model to. Doc 01 section 5.
pub const STAGES: &[&str] = &[
    "route",
    "plan",
    "retrieve",
    "synthesize",
    "visualize",
    "read",
    "verify",
    "exercise",
    "tutor",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Alias {
    pub provider: String,
    pub model: String,
    /// Names a keychain entry. Never a secret. Doc 01 section 4.16.
    pub key_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StagePolicy {
    /// `None` means the stage uses no model. `retrieve` is the one that does.
    pub alias: Option<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPolicy {
    pub version: String,
    pub stages: BTreeMap<String, StagePolicy>,
    pub aliases: BTreeMap<String, Alias>,
}

impl ModelPolicy {
    /// The shipped default. Doc 01 section 5, with the placeholder model names
    /// replaced by current ids and the vision alias moved to Anthropic so a
    /// fresh install needs one model key (BN-005, BN-006).
    pub fn default_anthropic(key_ref: &str) -> Self {
        let alias = |model: &str| Alias {
            provider: "anthropic".into(),
            model: model.into(),
            key_ref: key_ref.into(),
        };
        let stage = |a: Option<&str>, f: &[&str]| StagePolicy {
            alias: a.map(str::to_string),
            fallback: f.iter().map(|s| (*s).to_string()).collect(),
        };

        Self {
            version: "1.0".into(),
            stages: BTreeMap::from([
                ("route".into(), stage(Some("small"), &["medium"])),
                ("plan".into(), stage(Some("medium"), &["frontier"])),
                // Retrieval is the core's job and uses no model. Doc 10 section 7.
                ("retrieve".into(), stage(None, &[])),
                ("synthesize".into(), stage(Some("frontier"), &["medium"])),
                ("visualize".into(), stage(Some("frontier"), &["medium"])),
                ("read".into(), stage(Some("vision"), &[])),
                ("verify".into(), stage(Some("medium"), &["frontier"])),
                ("exercise".into(), stage(Some("medium"), &[])),
                ("tutor".into(), stage(Some("medium"), &[])),
            ]),
            aliases: BTreeMap::from([
                ("small".into(), alias("claude-haiku-4-5")),
                ("medium".into(), alias("claude-sonnet-5")),
                ("frontier".into(), alias("claude-opus-5")),
                ("vision".into(), alias("claude-opus-5")),
            ]),
        }
    }

    /// Overlay another policy. Only the stages and aliases the overlay names are
    /// replaced, so a board override of one stage does not drop the rest.
    pub fn overlay(&self, other: &ModelPolicy) -> ModelPolicy {
        let mut merged = self.clone();
        merged.version = other.version.clone();
        for (stage, policy) in &other.stages {
            merged.stages.insert(stage.clone(), policy.clone());
        }
        for (name, alias) in &other.aliases {
            merged.aliases.insert(name.clone(), alias.clone());
        }
        merged
    }

    /// Replace the alias for one stage. This is `request.model_override`, the
    /// most specific layer, and doc 01 section 5 records it in
    /// `Card.produced_by`.
    pub fn override_stage(&self, stage: &str, alias: &str) -> ModelPolicy {
        let mut p = self.clone();
        let entry = p.stages.entry(stage.to_string()).or_default();
        entry.alias = Some(alias.to_string());
        p
    }
}

/// One stage, resolved to a concrete provider and model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedStage {
    pub stage: String,
    pub alias: String,
    pub provider: String,
    pub model: String,
    pub key_ref: String,
    /// True when the first choice had no key and a fallback was taken. Doc 03
    /// section 7 emits `model.fallback.v1` for this.
    pub used_fallback: bool,
}

/// Every stage resolved at run start, snapshotted onto the Run so a rerun months
/// later can say which model produced the card. Doc 01 section 6.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPolicy {
    pub stages: BTreeMap<String, ResolvedStage>,
    /// Stages the policy deliberately gives no model.
    pub modelless: Vec<String>,
}

impl ResolvedPolicy {
    pub fn get(&self, stage: &str) -> Option<&ResolvedStage> {
        self.stages.get(stage)
    }

    /// Doc 10 section 10: the Profile shows an "offline capable" badge when
    /// every stage resolves to a local alias.
    pub fn is_offline_capable(&self) -> bool {
        !self.stages.is_empty() && self.stages.values().all(|s| s.provider == "ollama")
    }
}

/// Resolve a policy against the keys that actually exist.
///
/// `required` names the stages this run needs. A run that never reads an image
/// must not fail because the vision alias has no key, which is why the caller
/// passes the list rather than the resolver assuming all of them.
pub fn resolve(policy: &ModelPolicy, keys: &dyn KeyStore, required: &[&str]) -> Result<ResolvedPolicy> {
    let mut stages = BTreeMap::new();
    let mut modelless = Vec::new();

    for stage in required {
        let Some(sp) = policy.stages.get(*stage) else {
            return Err(ProviderError::NoKey {
                provider: "unknown".into(),
                alias: format!("no policy entry for stage `{stage}`"),
            });
        };

        let Some(first) = &sp.alias else {
            modelless.push((*stage).to_string());
            continue;
        };

        // First choice, then the fallback list in order.
        let candidates = std::iter::once(first).chain(sp.fallback.iter());
        let mut resolved = None;
        let mut last_provider = String::new();

        for (index, name) in candidates.enumerate() {
            let Some(alias) = policy.aliases.get(name) else {
                continue;
            };
            last_provider = alias.provider.clone();
            if !keys.has(&alias.key_ref) {
                continue;
            }
            resolved = Some(ResolvedStage {
                stage: (*stage).to_string(),
                alias: name.clone(),
                provider: alias.provider.clone(),
                model: alias.model.clone(),
                key_ref: alias.key_ref.clone(),
                used_fallback: index > 0,
            });
            break;
        }

        // Doc 03 section 10: fail before any retrieval rather than guessing. The
        // UI opens Profile with the missing stage highlighted.
        let Some(resolved) = resolved else {
            return Err(ProviderError::NoKey {
                provider: last_provider,
                alias: first.clone(),
            });
        };
        stages.insert((*stage).to_string(), resolved);
    }

    Ok(ResolvedPolicy { stages, modelless })
}

/// Effort per stage. BN-007: effort is how depth reaches the call, since a fixed
/// thinking budget is rejected on current models.
pub fn effort_for(stage: &str, depth: &str) -> Effort {
    match (stage, depth) {
        ("route", _) => Effort::Low,
        // Research synthesis is the one place worth the extra thinking: doc 06
        // section A8 adds a convergence step there and nowhere else.
        ("synthesize", "research") => Effort::Xhigh,
        ("synthesize", _) | ("verify", _) | ("visualize", _) => Effort::High,
        ("plan", "research") => Effort::High,
        ("plan", _) | ("exercise", _) | ("tutor", _) => Effort::Medium,
        ("read", _) => Effort::High,
        _ => Effort::Medium,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryKeyStore;

    const CARD_STAGES: &[&str] = &["route", "plan", "retrieve", "synthesize", "visualize", "verify"];

    #[test]
    fn the_default_policy_resolves_with_one_key() {
        // Doc 12 phase 11: fresh install to a first verified deep card with one
        // model key and one search key. That only holds if every stage of a card
        // run resolves from a single keychain entry (BN-006).
        let keys = MemoryKeyStore::with("anthropic-team", "sk-test");
        let policy = ModelPolicy::default_anthropic("anthropic-team");
        let resolved = resolve(&policy, &keys, CARD_STAGES).expect("resolves");

        assert_eq!(resolved.stages.len(), 5, "retrieve uses no model");
        assert_eq!(resolved.modelless, vec!["retrieve"]);
        assert_eq!(resolved.get("route").expect("route").model, "claude-haiku-4-5");
        assert_eq!(resolved.get("synthesize").expect("synth").model, "claude-opus-5");
        assert!(resolved.stages.values().all(|s| !s.used_fallback));
    }

    #[test]
    fn the_read_stage_also_resolves_from_the_same_key() {
        // BN-006: the vision alias moved to Anthropic precisely so this holds.
        let keys = MemoryKeyStore::with("anthropic-team", "sk-test");
        let policy = ModelPolicy::default_anthropic("anthropic-team");
        let resolved = resolve(&policy, &keys, &["read"]).expect("resolves");
        assert_eq!(resolved.get("read").expect("read").provider, "anthropic");
    }

    #[test]
    fn a_stage_with_no_key_falls_back_and_says_so() {
        let keys = MemoryKeyStore::with("backup", "sk-backup");
        let mut policy = ModelPolicy::default_anthropic("primary");
        policy.aliases.insert(
            "medium".into(),
            Alias {
                provider: "anthropic".into(),
                model: "claude-sonnet-5".into(),
                key_ref: "backup".into(),
            },
        );

        // route is small with fallback medium; small's key is missing.
        let resolved = resolve(&policy, &keys, &["route"]).expect("falls back");
        let route = resolved.get("route").expect("route");
        assert_eq!(route.alias, "medium");
        assert!(
            route.used_fallback,
            "the fallback must be visible so model.fallback.v1 can fire"
        );
    }

    #[test]
    fn no_key_anywhere_fails_before_any_retrieval() {
        // Doc 03 section 10 `policy_unresolvable`: fail the run before spending.
        let keys = MemoryKeyStore::new();
        let policy = ModelPolicy::default_anthropic("anthropic-team");
        let err = resolve(&policy, &keys, &["synthesize"]).expect_err("must not guess");
        assert_eq!(err.kind(), "policy_unresolvable");
        assert!(!err.is_retryable(), "retrying a missing key cannot help");
    }

    #[test]
    fn a_run_that_reads_no_image_is_not_blocked_by_a_missing_vision_key() {
        let keys = MemoryKeyStore::with("anthropic-team", "sk-test");
        let mut policy = ModelPolicy::default_anthropic("anthropic-team");
        policy.aliases.insert(
            "vision".into(),
            Alias {
                provider: "google".into(),
                model: "gemini".into(),
                key_ref: "google-personal".into(),
            },
        );
        resolve(&policy, &keys, CARD_STAGES).expect("a card run does not need vision");
        assert!(resolve(&policy, &keys, &["read"]).is_err(), "but a read run does");
    }

    #[test]
    fn overlay_replaces_only_what_it_names() {
        let base = ModelPolicy::default_anthropic("anthropic-team");
        let mut board = ModelPolicy {
            version: "1.0".into(),
            stages: BTreeMap::new(),
            aliases: BTreeMap::new(),
        };
        board.stages.insert(
            "synthesize".into(),
            StagePolicy {
                alias: Some("medium".into()),
                fallback: vec![],
            },
        );
        let merged = base.overlay(&board);
        assert_eq!(merged.stages["synthesize"].alias.as_deref(), Some("medium"));
        assert_eq!(
            merged.stages["verify"].alias.as_deref(),
            Some("medium"),
            "untouched stages survive"
        );
        assert_eq!(merged.aliases.len(), base.aliases.len());
    }

    #[test]
    fn a_card_override_is_the_most_specific_layer() {
        let keys = MemoryKeyStore::with("anthropic-team", "sk-test");
        let policy = ModelPolicy::default_anthropic("anthropic-team").override_stage("synthesize", "small");
        let resolved = resolve(&policy, &keys, &["synthesize"]).expect("resolves");
        assert_eq!(resolved.get("synthesize").expect("s").model, "claude-haiku-4-5");
    }

    #[test]
    fn effort_tracks_depth_only_where_it_matters() {
        assert_eq!(effort_for("route", "research"), Effort::Low);
        assert_eq!(effort_for("synthesize", "deep"), Effort::High);
        assert_eq!(effort_for("synthesize", "research"), Effort::Xhigh);
        assert_eq!(effort_for("verify", "fast"), Effort::High);
    }

    #[test]
    fn offline_capable_needs_every_stage_local() {
        let keys = MemoryKeyStore::with("local", "");
        let ollama = |model: &str| Alias {
            provider: "ollama".into(),
            model: model.into(),
            key_ref: "local".into(),
        };
        let mut policy = ModelPolicy::default_anthropic("local");
        for name in ["small", "medium", "frontier", "vision"] {
            policy.aliases.insert(name.into(), ollama("llama"));
        }
        let resolved = resolve(&policy, &keys, CARD_STAGES).expect("resolves");
        assert!(resolved.is_offline_capable());

        let mixed = resolve(&ModelPolicy::default_anthropic("local"), &keys, CARD_STAGES).expect("resolves");
        assert!(!mixed.is_offline_capable());
    }
}
