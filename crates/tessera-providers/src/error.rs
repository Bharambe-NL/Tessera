//! Provider failure taxonomy.
//!
//! Pattern 4. Every agent spec in the set names its recovery per failure type
//! (doc 03 section 10, doc 04 section 10, doc 05 section 10, doc 06 sections A10
//! and B10, doc 07 sections A10 and B10). Those recipes only work if the
//! provider layer reports failures in categories the agent can match on, rather
//! than as one opaque string.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    /// The alias resolved to a provider with no active key. Doc 03 section 8.3:
    /// the Router fails with `policy_unresolvable` rather than guessing.
    #[error("no active key for provider `{provider}` (alias `{alias}`)")]
    NoKey { provider: String, alias: String },

    /// The key exists but the provider rejected it. Doc 03 section 10: after
    /// three runs the UI shows the key as failing.
    #[error("provider `{provider}` rejected the key")]
    Auth { provider: String },

    /// 429. Doc 05 section 10: back off once within budget, then partial.
    #[error("provider `{provider}` rate limited{}", .retry_after_ms.map(|ms| format!(", retry after {ms} ms")).unwrap_or_default())]
    RateLimited {
        provider: String,
        retry_after_ms: Option<u64>,
    },

    /// 5xx or a transport failure. The caller tries the fallback alias once.
    #[error("provider `{provider}` unavailable: {detail}")]
    Unavailable { provider: String, detail: String },

    #[error("provider `{provider}` timed out after {elapsed_ms} ms")]
    Timeout { provider: String, elapsed_ms: u64 },

    /// The provider answered, but not with what was asked for. Distinct from
    /// `Unavailable` because the recovery differs: a retry with the violation
    /// attached, not a fallback alias.
    #[error("provider `{provider}` returned no usable content: {detail}")]
    Malformed { provider: String, detail: String },

    /// 4xx that is not auth: a bad model id, an over long request, a rejected
    /// schema. Retrying the same request cannot help.
    #[error("provider `{provider}` refused the request: {detail}")]
    BadRequest { provider: String, detail: String },

    /// The model declined on policy grounds. HTTP 200 with a refusal stop
    /// reason, so it has to be checked rather than caught.
    #[error("provider `{provider}` declined: {category}")]
    Refused { provider: String, category: String },

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("keychain: {0}")]
    Keychain(String),

    #[error("transport: {0}")]
    Transport(String),
}

impl ProviderError {
    /// Whether trying the same request again could plausibly succeed.
    ///
    /// Doc 05 section 10 backs off once on a rate limit; doc 03 section 10 tries
    /// the fallback alias once on `provider_unavailable`. A bad request or a
    /// missing key is not retryable, and retrying one wastes the user's money.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited { .. }
                | ProviderError::Unavailable { .. }
                | ProviderError::Timeout { .. }
        )
    }

    /// Whether the caller should try the alias's fallback list.
    pub fn should_try_fallback(&self) -> bool {
        matches!(
            self,
            ProviderError::Unavailable { .. }
                | ProviderError::Timeout { .. }
                | ProviderError::Auth { .. }
                | ProviderError::RateLimited { .. }
        )
    }

    /// A stable code for the `failure.type` field on a Step and for the
    /// diagnostics page's "failures by type" breakdown (doc 10 section 11).
    pub fn kind(&self) -> &'static str {
        match self {
            ProviderError::NoKey { .. } => "policy_unresolvable",
            ProviderError::Auth { .. } => "provider_auth",
            ProviderError::RateLimited { .. } => "rate_limited",
            ProviderError::Unavailable { .. } => "provider_unavailable",
            ProviderError::Timeout { .. } => "model_timeout",
            ProviderError::Malformed { .. } => "schema_violation",
            ProviderError::BadRequest { .. } => "bad_request",
            ProviderError::Refused { .. } => "provider_refused",
            ProviderError::Json(_) => "schema_violation",
            ProviderError::Keychain(_) => "keychain",
            ProviderError::Transport(_) => "provider_unavailable",
        }
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;
