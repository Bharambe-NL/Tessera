//! One provider that routes each call to the adapter its model belongs to.
//!
//! The policy layer has been multi provider since M2: aliases name a provider
//! and `resolve` skips any alias whose key is missing. What the product lacked
//! until 2026-08-30 was the other half: the app built exactly one adapter, so a
//! stage that resolved to Moonshot on paper was still sent to Anthropic. This
//! type closes that gap. It is built from the same policy the resolver reads,
//! so the two halves cannot disagree about which model lives where.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::anthropic::AnthropicProvider;
use crate::error::{ProviderError, Result};
use crate::keychain::KeyStore;
use crate::mock::MockProvider;
use crate::model::{Completion, CompletionRequest, ModelProvider};
use crate::openai_compat::{OpenAiCompatProvider, endpoint_for};
use crate::policy::ModelPolicy;

pub struct MultiProvider {
    /// Provider id, shown on the Profile page. Joined from the adapters that
    /// actually built, so "anthropic+moonshot" says what this install can reach.
    id: String,
    /// model id -> adapter, derived from the policy aliases at build time.
    by_model: BTreeMap<String, Arc<dyn ModelProvider>>,
    /// The lone adapter, when there is exactly one, so a model id the aliases
    /// do not know still goes somewhere sensible instead of failing on a map.
    only: Option<Arc<dyn ModelProvider>>,
}

impl MultiProvider {
    /// Route table from a policy and the adapters that had keys.
    fn from_adapters(policy: &ModelPolicy, adapters: BTreeMap<String, Arc<dyn ModelProvider>>) -> Self {
        let mut by_model = BTreeMap::new();
        for alias in policy.aliases.values() {
            if let Some(adapter) = adapters.get(&alias.provider) {
                by_model.insert(alias.model.clone(), Arc::clone(adapter));
            }
        }
        let only = if adapters.len() == 1 {
            adapters.values().next().cloned()
        } else {
            None
        };
        Self {
            id: adapters.keys().cloned().collect::<Vec<_>>().join("+"),
            by_model,
            only,
        }
    }
}

#[async_trait]
impl ModelProvider for MultiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        let adapter = self
            .by_model
            .get(&request.model)
            .or(self.only.as_ref())
            .ok_or_else(|| {
                ProviderError::Keychain(format!(
                    "no provider is configured for the model `{}`",
                    request.model
                ))
            })?;
        adapter.complete(request).await
    }
}

/// Build the live provider for the keys that exist right now.
///
/// One adapter per provider the policy names, built only when its key is in the
/// keychain, wrapped so each call reaches the adapter its model belongs to.
/// With no key anywhere this returns the mock, which never answers a live card:
/// `resolve` fails with `policy_unresolvable` before any call is made.
pub fn live_provider(keys: &dyn KeyStore, policy: &ModelPolicy) -> Arc<dyn ModelProvider> {
    let mut wanted: BTreeMap<String, String> = BTreeMap::new();
    for alias in policy.aliases.values() {
        wanted
            .entry(alias.provider.clone())
            .or_insert_with(|| alias.key_ref.clone());
    }

    let mut adapters: BTreeMap<String, Arc<dyn ModelProvider>> = BTreeMap::new();
    for (provider, key_ref) in &wanted {
        let Ok(secret) = keys.get(key_ref) else {
            continue;
        };
        let built: Option<Arc<dyn ModelProvider>> = match provider.as_str() {
            "anthropic" => AnthropicProvider::new(secret).ok().map(|p| Arc::new(p) as _),
            other => endpoint_for(other)
                .and_then(|endpoint| OpenAiCompatProvider::new(endpoint, secret).ok())
                .map(|p| Arc::new(p) as _),
        };
        if let Some(adapter) = built {
            adapters.insert(provider.clone(), adapter);
        }
    }

    if adapters.is_empty() {
        return Arc::new(MockProvider::new());
    }
    Arc::new(MultiProvider::from_adapters(policy, adapters))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryKeyStore;

    #[test]
    fn no_key_anywhere_builds_the_mock() {
        let keys = MemoryKeyStore::new();
        let policy = ModelPolicy::default_anthropic(ModelPolicy::ANTHROPIC_KEY_REF);
        assert_eq!(live_provider(&keys, &policy).id(), "mock");
    }

    #[test]
    fn one_key_builds_that_adapter_alone() {
        let keys = MemoryKeyStore::with(ModelPolicy::MOONSHOT_KEY_REF, "sk-test");
        let policy = ModelPolicy::default_anthropic(ModelPolicy::ANTHROPIC_KEY_REF);
        assert_eq!(live_provider(&keys, &policy).id(), "moonshot");
    }

    #[test]
    fn both_keys_name_both_providers() {
        let keys = MemoryKeyStore::with(ModelPolicy::ANTHROPIC_KEY_REF, "sk-a");
        keys.set(ModelPolicy::MOONSHOT_KEY_REF, "sk-m").expect("set");
        let policy = ModelPolicy::default_anthropic(ModelPolicy::ANTHROPIC_KEY_REF);
        assert_eq!(live_provider(&keys, &policy).id(), "anthropic+moonshot");
    }
}
