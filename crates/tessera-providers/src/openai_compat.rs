//! An adapter for providers that speak the OpenAI chat completions shape.
//!
//! Doc 10 section 3 lists anthropic, openai, google, mistral and ollama as the
//! providers to support, and Pattern 21 exists so adding one is an adapter
//! rather than a change to any agent. Several of those speak the same wire
//! format, so one adapter serves them: it takes a base url, a provider id and a
//! model, and nothing above it knows the difference.
//!
//! Moonshot's Kimi is the reason this exists now. The eval sweep is 400
//! questions, which is a real cost against a frontier model, and doc 02 section
//! 10.1 records the model policy under test with the results precisely so
//! numbers from different policies stay comparable rather than being mixed.
//!
//! Doc 10 section 7 still applies in full: no tools are ever declared, because
//! retrieval is the core's job and a claim the model found by itself would have
//! no Passage row behind it (BN-010).

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::{ProviderError, Result};
use crate::model::{Completion, CompletionRequest, ContentBlock, ModelProvider, Role, Usage};

/// Where a known OpenAI-compatible provider lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    pub id: &'static str,
    pub base_url: &'static str,
    /// Whether the provider honours `response_format: {"type": "json_object"}`.
    pub json_mode: bool,
}

/// Moonshot's Kimi. The international endpoint; `api.moonshot.cn` is the
/// mainland one and takes a separately issued key.
pub const MOONSHOT: Endpoint = Endpoint {
    id: "moonshot",
    base_url: "https://api.moonshot.ai/v1",
    json_mode: true,
};

pub const OPENAI: Endpoint = Endpoint {
    id: "openai",
    base_url: "https://api.openai.com/v1",
    json_mode: true,
};

/// A local Ollama server. Doc 10 section 10: an Ollama adapter is what makes a
/// fully offline configuration possible.
pub const OLLAMA: Endpoint = Endpoint {
    id: "ollama",
    base_url: "http://127.0.0.1:11434/v1",
    json_mode: false,
};

/// Extra output budget for a model that reasons before it answers.
///
/// The agents size `max_tokens` for the content they expect: the Router asks for
/// 1,200, which is generous for a classification block. A reasoning model spends
/// that budget thinking and stops at the limit having emitted nothing, which
/// arrives as `finish_reason: length` with empty content rather than as anything
/// resembling a useful error.
///
/// The headroom is added rather than the caller's figure replaced, so an agent
/// that asks for a long answer still gets one.
const REASONING_HEADROOM: u32 = 6_000;

pub fn endpoint_for(provider: &str) -> Option<Endpoint> {
    match provider {
        "moonshot" | "kimi" => Some(MOONSHOT),
        "openai" => Some(OPENAI),
        "ollama" => Some(OLLAMA),
        _ => None,
    }
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    api_key: String,
    id: String,
    base_url: String,
    json_mode: bool,
}

impl OpenAiCompatProvider {
    pub fn new(endpoint: Endpoint, api_key: impl Into<String>) -> Result<Self> {
        Self::custom(endpoint.id, endpoint.base_url, api_key, endpoint.json_mode)
    }

    /// For a provider not in the table, or a self hosted one.
    pub fn custom(
        id: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        json_mode: bool,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            api_key: api_key.into(),
            id: id.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            json_mode,
        })
    }

    /// Ask the provider what it can run.
    ///
    /// Model names move, and guessing one produces a 404 that reads like an
    /// outage. This is how the Profile's Models page confirms a key works and
    /// offers the models it actually has.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| ProviderError::Unavailable {
                provider: self.id.clone(),
                detail: e.to_string(),
            })?;

        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => ProviderError::Auth {
                    provider: self.id.clone(),
                },
                _ => ProviderError::BadRequest {
                    provider: self.id.clone(),
                    detail: truncate(&detail),
                },
            });
        }

        let body: Value = response.json().await.map_err(|e| ProviderError::Malformed {
            provider: self.id.clone(),
            detail: e.to_string(),
        })?;

        let mut models: Vec<String> = body["data"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| r["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        models.sort();
        Ok(models)
    }

    fn body(&self, req: &CompletionRequest) -> Value {
        let mut messages: Vec<Value> = Vec::new();
        if let Some(system) = &req.system {
            messages.push(json!({ "role": "system", "content": system }));
        }

        for m in &req.messages {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            // A text only message goes as a plain string, which every provider
            // in this family accepts. The array form is only needed for images.
            let has_image = m.content.iter().any(|b| matches!(b, ContentBlock::Image { .. }));

            if has_image {
                let parts: Vec<Value> = m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                        ContentBlock::Image { media_type, data } => json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{media_type};base64,{data}") }
                        }),
                    })
                    .collect();
                messages.push(json!({ "role": role, "content": parts }));
            } else {
                let text = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        ContentBlock::Image { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                messages.push(json!({ "role": role, "content": text }));
            }
        }

        let mut body = json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens + REASONING_HEADROOM,
        });

        // Temperature is deliberately not sent.
        //
        // This family has no adaptive thinking and no effort parameter, so the
        // depth signal cannot be carried the way it is on Anthropic (BN-007),
        // and a low temperature looked like the natural substitute for a
        // structured extraction. It is not: a reasoning model in this family
        // fixes temperature and rejects anything else outright. Kimi K2.6
        // answers "only 1 is allowed for this model" with a 400.
        //
        // Omitting it takes the provider's own default, which is correct on
        // every model in the family including the ones that would have accepted
        // a value. The same conservative-request rule as BN-022: send what a
        // model is known to take, not what would be nice to set.

        if self.json_mode && req.output_schema.is_some() {
            // The schema itself is not accepted here, only the mode. The prompt
            // carries the schema (prompts::json_only) and the schema guard
            // catches what the model got wrong, which is doc 10 section 7's
            // "else schema prompting plus validation".
            body["response_format"] = json!({ "type": "json_object" });
        }
        body
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn has_json_mode(&self) -> bool {
        self.json_mode
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<Completion> {
        let started = Instant::now();

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&self.body(req))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout {
                        provider: self.id.clone(),
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    }
                } else {
                    ProviderError::Unavailable {
                        provider: self.id.clone(),
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
                    provider: self.id.clone(),
                },
                429 => ProviderError::RateLimited {
                    provider: self.id.clone(),
                    retry_after_ms: retry_after,
                },
                408 | 504 => ProviderError::Timeout {
                    provider: self.id.clone(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                },
                500..=599 => ProviderError::Unavailable {
                    provider: self.id.clone(),
                    detail: truncate(&detail),
                },
                _ => ProviderError::BadRequest {
                    provider: self.id.clone(),
                    detail: truncate(&detail),
                },
            });
        }

        let body: Value = response.json().await.map_err(|e| ProviderError::Malformed {
            provider: self.id.clone(),
            detail: e.to_string(),
        })?;

        let choice = &body["choices"][0];
        let finish = choice["finish_reason"].as_str().map(str::to_string);

        // A content filter stop is this family's refusal. Doc 06 section A10
        // treats a provider that declined as a failure, never as an answer.
        if finish.as_deref() == Some("content_filter") {
            return Err(ProviderError::Refused {
                provider: self.id.clone(),
                category: "content_filter".into(),
            });
        }

        let text = choice["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        if text.is_empty() {
            // Distinguish the two ways this happens, because the fixes differ.
            // Running out of budget mid reasoning is a configuration problem;
            // an empty answer for any other reason is a provider problem.
            let detail = if finish.as_deref() == Some("length") {
                "the model used its whole output budget before writing anything, which usually means it reasoned for longer than the budget allowed"
                    .to_string()
            } else {
                format!("no content in the response (finish reason {finish:?})")
            };
            return Err(ProviderError::Malformed {
                provider: self.id.clone(),
                detail,
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
                input_tokens: field("prompt_tokens"),
                output_tokens: field("completion_tokens"),
                cache_read_tokens: usage
                    .and_then(|u| u.get("prompt_tokens_details"))
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            },
            model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&req.model)
                .to_string(),
            provider: self.id.clone(),
            latency_ms: started.elapsed().as_millis() as u64,
            stop_reason: finish,
        })
    }
}

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
    use crate::model::{Effort, Message};

    fn provider() -> OpenAiCompatProvider {
        OpenAiCompatProvider::new(MOONSHOT, "k").expect("provider")
    }

    #[test]
    fn the_body_never_declares_tools() {
        // Doc 10 section 7 and BN-010, on every adapter rather than just one.
        let body = provider().body(&CompletionRequest::new("kimi", "synthesize").user("q"));
        assert!(body.get("tools").is_none());
        assert!(body.get("functions").is_none());
    }

    #[test]
    fn a_system_prompt_becomes_the_first_message() {
        let body = provider().body(
            &CompletionRequest::new("kimi", "route")
                .system("you classify")
                .user("q"),
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "you classify");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn a_text_message_goes_as_a_string_not_an_array() {
        // The array form is legal but not universally accepted in this family;
        // the string form is.
        let body = provider().body(&CompletionRequest::new("kimi", "s").user("hello"));
        assert!(body["messages"][0]["content"].is_string());
    }

    #[test]
    fn an_image_message_uses_the_parts_form() {
        let mut request = CompletionRequest::new("kimi", "read");
        request = request.message(Message {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: "what is this".into(),
                },
                ContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "AAAA".into(),
                },
            ],
        });
        let body = provider().body(&request);
        let parts = body["messages"][0]["content"].as_array().expect("parts");
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(
            parts[1]["image_url"]["url"]
                .as_str()
                .is_some_and(|u| u.starts_with("data:image/png;base64,"))
        );
    }

    #[test]
    fn json_mode_is_requested_only_when_a_schema_was_asked_for() {
        let p = provider();
        let plain = p.body(&CompletionRequest::new("kimi", "s").user("q"));
        assert!(plain.get("response_format").is_none());

        let structured = p.body(&CompletionRequest::new("kimi", "s").user("q").expecting(json!({})));
        assert_eq!(structured["response_format"]["type"], "json_object");
    }

    #[test]
    fn a_provider_without_json_mode_never_asks_for_it() {
        // Ollama in this family does not honour the flag, and sending it turns
        // a working call into a 400.
        let p = OpenAiCompatProvider::new(OLLAMA, "").expect("provider");
        let body = p.body(
            &CompletionRequest::new("llama", "s")
                .user("q")
                .expecting(json!({})),
        );
        assert!(body.get("response_format").is_none());
        assert!(!p.has_json_mode());
    }

    #[test]
    fn a_reasoning_model_gets_headroom_above_what_the_agent_asked_for() {
        // The Router asks for 1,200 tokens of classification. A reasoning model
        // spends that thinking and returns nothing, so the request carries the
        // agent's figure plus room to reason.
        let body = provider().body(
            &CompletionRequest::new("kimi-k2.6", "route")
                .user("q")
                .max_tokens(1200),
        );
        assert_eq!(body["max_tokens"], 1200 + REASONING_HEADROOM);
    }

    #[test]
    fn temperature_is_never_sent() {
        // A reasoning model in this family fixes it and rejects any value with
        // a 400, so setting it turns every call into a bad request.
        let body = provider().body(&CompletionRequest::new("kimi-k2.6", "synthesize").user("q"));
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
    }

    #[test]
    fn effort_is_not_sent_where_it_does_not_exist() {
        // BN-007 carries depth through effort on Anthropic. This family has no
        // such parameter, and inventing one would be a 400 on every call.
        let body = provider().body(
            &CompletionRequest::new("kimi", "synthesize")
                .user("q")
                .effort(Effort::Xhigh),
        );
        assert!(body.get("effort").is_none());
        assert!(body.get("output_config").is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn known_provider_names_resolve_to_an_endpoint() {
        assert_eq!(endpoint_for("kimi"), Some(MOONSHOT));
        assert_eq!(endpoint_for("moonshot"), Some(MOONSHOT));
        assert_eq!(endpoint_for("openai"), Some(OPENAI));
        assert_eq!(endpoint_for("anthropic"), None, "anthropic has its own adapter");
    }
}
