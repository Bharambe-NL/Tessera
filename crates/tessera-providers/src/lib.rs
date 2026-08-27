//! Provider abstraction. Pattern 21.
//!
//! Doc 10 section 7 fixes the shape: one trait per provider class, adapters
//! behind it, and no tool use inside a model call so retrieval stays the core's
//! job and provenance is uniform.
//!
//! Secrets never leave the keychain. Everything here passes a `key_ref`.

pub mod anthropic;
pub mod error;
pub mod keychain;
pub mod mock;
pub mod model;
pub mod openai_compat;
pub mod policy;

pub use anthropic::AnthropicProvider;
pub use error::{ProviderError, Result};
pub use keychain::{EnvKeyStore, KeyStore, MemoryKeyStore, OsKeychain};
pub use mock::{MockFailure, MockProvider, MockResponse};
pub use model::{Completion, CompletionRequest, ContentBlock, Effort, Message, ModelProvider, Role, Usage};
pub use openai_compat::{Endpoint, MOONSHOT, OLLAMA, OPENAI, OpenAiCompatProvider, endpoint_for};
pub use policy::{
    Alias, ModelPolicy, ResolvedPolicy, ResolvedStage, STAGES, StagePolicy, effort_for, resolve,
};
