//! Anthropic adapter.
//!
//! Raw HTTP rather than an SDK because there is no official Anthropic SDK for
//! Rust. The request shape follows the current Messages API: adaptive thinking,
//! effort inside `output_config`, and structured output through
//! `output_config.format`.
//!
//! Doc 10 section 7: no tools are ever declared. Retrieval is the core's job so
//! provenance is uniform, and a claim the model found by itself would have no
//! Passage row behind it for the Verifier to check (BN-010).

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::{ProviderError, Result};
use crate::model::{Completion, CompletionRequest, ContentBlock, ModelProvider, Role, Usage};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Model families that accept adaptive thinking and `output_config.effort`.
///
/// Both parameters arrived with the 4.6 generation. Sending either to an older
/// model is a 400, not a silently ignored field, so this cannot be left to
/// optimism: the `small` alias resolves to Haiku 4.5, which means the Router
/// would have failed on every real call while passing every mock test.
///
/// The list is an allowlist rather than a denylist. A model nobody here has
/// heard of gets the conservative request, which works everywhere, instead of
/// the modern one, which works only on what was current when this was written.
const ADAPTIVE_FAMILIES: &[&str] = &[
    "claude-opus-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-fable-5",
    "claude-mythos-5",
];

/// Whether this model takes `thinking: {type: "adaptive"}` and an effort level.
pub fn supports_adaptive(model: &str) -> bool {
    ADAPTIVE_FAMILIES.iter().any(|family| model.starts_with(family))
}

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, API_URL)
    }

    /// The base url is injectable so the eval harness and the adapter's own
    /// tests can point at a local server without a network call.
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
        })
    }

    fn body(&self, req: &CompletionRequest) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let content: Vec<Value> = m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                        ContentBlock::Image { media_type, data } => json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": media_type, "data": data }
                        }),
                    })
                    .collect();
                json!({
                    "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                    "content": content
                })
            })
            .collect();

        let adaptive = supports_adaptive(&req.model);

        let mut output_config = serde_json::Map::new();
        if adaptive {
            // Effort is how depth reaches the call (BN-007), and it arrived with
            // the same generation as adaptive thinking.
            output_config.insert("effort".into(), json!(req.effort.as_str()));
        }
        if let Some(schema) = &req.output_schema {
            output_config.insert(
                "format".into(),
                json!({ "type": "json_schema", "schema": schema }),
            );
        }

        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": messages,
        });

        if adaptive {
            // Adaptive is the only on-mode on models that have it, and a fixed
            // thinking budget is rejected there. On an older model the parameter
            // is absent entirely: a classification call at low effort has no use
            // for thinking anyway, so nothing is lost by leaving it off.
            body["thinking"] = json!({ "type": "adaptive" });
        }
        if !output_config.is_empty() {
            body["output_config"] = Value::Object(output_config);
        }

        if let Some(system) = &req.system {
            body["system"] = json!(system);
        }
        body
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn has_json_mode(&self) -> bool {
        true
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        let started = Instant::now();

        let response = self
            .client
            .post(&self.base_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&self.body(req))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout {
                        provider: "anthropic".into(),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    }
                } else {
                    ProviderError::Unavailable {
                        provider: "anthropic".into(),
                        detail: e.to_string(),
                    }
                }
            })?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|s| s * 1000);

        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => ProviderError::Auth {
                    provider: "anthropic".into(),
                },
                429 => ProviderError::RateLimited {
                    provider: "anthropic".into(),
                    retry_after_ms: retry_after,
                },
                408 | 504 => ProviderError::Timeout {
                    provider: "anthropic".into(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
                500..=599 => ProviderError::Unavailable {
                    provider: "anthropic".into(),
                    detail: truncate(&detail),
                },
                _ => ProviderError::BadRequest {
                    provider: "anthropic".into(),
                    detail: truncate(&detail),
                },
            });
        }

        let body: Value = response.json().await.map_err(|e| ProviderError::Malformed {
            provider: "anthropic".into(),
            detail: e.to_string(),
        })?;

        // A policy decline arrives as HTTP 200 with a refusal stop reason, so it
        // has to be checked rather than caught.
        let stop_reason = body
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string);
        if stop_reason.as_deref() == Some("refusal") {
            let category = body
                .get("stop_details")
                .and_then(|d| d.get("category"))
                .and_then(Value::as_str)
                .unwrap_or("unspecified")
                .to_string();
            return Err(ProviderError::Refused {
                provider: "anthropic".into(),
                category,
            });
        }

        // Thinking blocks are skipped: only text is the answer.
        let text = body
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        if text.is_empty() {
            return Err(ProviderError::Malformed {
                provider: "anthropic".into(),
                detail: format!("no text block in the response (stop reason {stop_reason:?})"),
            });
        }

        let usage = body.get("usage");
        let field = |name: &str| {
            usage
                .and_then(|u| u.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };

        Ok(Completion {
            text,
            usage: Usage {
                input_tokens: field("input_tokens"),
                output_tokens: field("output_tokens"),
                cache_read_tokens: field("cache_read_input_tokens"),
            },
            model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&req.model)
                .to_string(),
            provider: "anthropic".into(),
            latency_ms: started.elapsed().as_millis() as u64,
            stop_reason,
        })
    }
}

/// Provider error bodies can be long. The diagnostics page wants the shape of
/// the failure, not a wall of it.
fn truncate(s: &str) -> String {
    const MAX: usize = 400;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut end = MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Effort;

    #[test]
    fn the_body_never_declares_tools() {
        // Doc 10 section 7 and BN-010. This is the test that keeps it true.
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(&CompletionRequest::new("claude-opus-5", "synthesize").user("q"));
        assert!(body.get("tools").is_none());
        assert!(body.get("mcp_servers").is_none());
    }

    #[test]
    fn thinking_is_adaptive_and_carries_no_budget() {
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(&CompletionRequest::new("claude-opus-5", "verify").user("q"));
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("budget_tokens").is_none());
    }

    #[test]
    fn an_older_model_gets_neither_thinking_nor_effort() {
        // The `small` alias resolves to Haiku 4.5, which predates both. Sending
        // either is a 400, so this is the difference between the Router working
        // against the real provider and failing on every call.
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(&CompletionRequest::new("claude-haiku-4-5", "route").user("q"));
        assert!(body.get("thinking").is_none());
        assert!(
            body.get("output_config").is_none(),
            "effort is not accepted there either"
        );
    }

    #[test]
    fn an_older_model_still_gets_its_output_schema() {
        // Structured output is not part of the same generation, so dropping it
        // alongside thinking would break the schema guard's cheap path.
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(
            &CompletionRequest::new("claude-haiku-4-5", "route")
                .user("q")
                .expecting(json!({ "type": "object" })),
        );
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert!(body["output_config"].get("effort").is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn an_unknown_model_gets_the_conservative_request() {
        // An allowlist means a model released after this was written is treated
        // as old, which works, rather than as new, which might not.
        assert!(!supports_adaptive("claude-something-9"));
        assert!(supports_adaptive("claude-opus-5"));
        assert!(supports_adaptive("claude-sonnet-4-6"));
        assert!(!supports_adaptive("claude-haiku-4-5"));
    }

    #[test]
    fn effort_rides_inside_output_config() {
        // On a model that takes effort at all. Haiku 4.5 does not, which is what
        // an_older_model_gets_neither_thinking_nor_effort covers.
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(
            &CompletionRequest::new("claude-sonnet-5", "route")
                .effort(Effort::Low)
                .user("q"),
        );
        assert_eq!(body["output_config"]["effort"], "low");
        assert!(
            body.get("effort").is_none(),
            "effort is not a top level parameter"
        );
    }

    #[test]
    fn a_schema_becomes_a_json_schema_format() {
        let p = AnthropicProvider::new("k").expect("provider");
        let schema = json!({ "type": "object", "properties": { "a": { "type": "string" } } });
        let body = p.body(
            &CompletionRequest::new("claude-opus-5", "visualize")
                .user("q")
                .expecting(schema.clone()),
        );
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(body["output_config"]["format"]["schema"], schema);
    }

    #[test]
    fn no_schema_means_no_format_key() {
        let p = AnthropicProvider::new("k").expect("provider");
        let body = p.body(&CompletionRequest::new("claude-opus-5", "s").user("q"));
        assert!(body["output_config"].get("format").is_none());
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let s = "é".repeat(500);
        let t = truncate(&s);
        assert!(t.len() <= 405);
    }
}
