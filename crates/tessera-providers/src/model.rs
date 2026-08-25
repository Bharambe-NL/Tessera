//! The model provider trait and its request and response types.
//!
//! Doc 10 section 7: "Model providers: one trait, `complete(packet) -> output`,
//! with adapters per provider; structured output via JSON mode where the
//! provider has it, else schema prompting plus validation. Tool use (web search
//! inside the model call) is disabled; retrieval is always the core's job so
//! provenance is uniform."
//!
//! The last sentence is the load bearing one and it is enforced here rather than
//! by convention: there is no field on [`CompletionRequest`] for tools, so an
//! agent cannot ask for one.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

/// How hard the model should think. Doc 01 section 5 fixes an alias per stage;
/// effort is how depth reaches the call (BN-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    #[default]
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}

/// Text and images only. Doc 10 section 7 disables tool use inside the model
/// call, so there is no tool_use or tool_result variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// The Reader's path. Doc 07 section A8 preprocesses to the vision alias's
    /// limit before this point.
    Image {
        media_type: String,
        /// Base64. The blob store holds the bytes; this is the encoded copy for
        /// one call, never persisted.
        data: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// Concrete provider model id, already resolved from the alias.
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub effort: Effort,
    /// A self contained JSON Schema. When the provider has JSON mode it goes in
    /// `output_config.format`; otherwise the adapter falls back to schema
    /// prompting and the schema guard catches what the model got wrong.
    pub output_schema: Option<Value>,
    /// Set on the Step for the audit trail. Doc 01 section 6.2 stores prompts by
    /// hash with the text in the blob store.
    pub stage: String,
}

impl CompletionRequest {
    pub fn new(model: impl Into<String>, stage: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: None,
            messages: Vec::new(),
            max_tokens: 16_000,
            effort: Effort::High,
            output_schema: None,
            stage: stage.into(),
        }
    }

    pub fn system(mut self, s: impl Into<String>) -> Self {
        self.system = Some(s.into());
        self
    }

    pub fn user(mut self, s: impl Into<String>) -> Self {
        self.messages.push(Message::user(s));
        self
    }

    pub fn message(mut self, m: Message) -> Self {
        self.messages.push(m);
        self
    }

    pub fn effort(mut self, e: Effort) -> Self {
        self.effort = e;
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn expecting(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// The prompt hash that goes on the Step. Deterministic over everything that
    /// changes the answer, so replaying a run can confirm it reproduced the call.
    pub fn prompt_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(self.model.as_bytes());
        h.update(self.effort.as_str().as_bytes());
        if let Some(s) = &self.system {
            h.update(s.as_bytes());
        }
        for m in &self.messages {
            for b in &m.content {
                match b {
                    ContentBlock::Text { text } => h.update(text.as_bytes()),
                    ContentBlock::Image { data, .. } => h.update(data.as_bytes()),
                }
            }
        }
        if let Some(schema) = &self.output_schema {
            h.update(schema.to_string().as_bytes());
        }
        hex::encode(h.finalize())
    }
}

/// What a call cost. Rolls up into `Run.cost` (doc 01 section 6.1), which the
/// Profile's spend page and the composer's estimate read.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
    pub model: String,
    pub provider: String,
    pub latency_ms: u64,
    pub stop_reason: Option<String>,
}

impl Completion {
    /// Parse the response as the JSON the agent asked for.
    ///
    /// Tolerates a model that wrapped its JSON in prose or a fence, because a
    /// provider without JSON mode reaches this path and one stray sentence
    /// should not cost a retry. The schema guard still has the last word.
    pub fn json(&self) -> Result<Value> {
        let t = self.text.trim();
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            return Ok(v);
        }
        let body = strip_fence(t);
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            return Ok(v);
        }
        // Last resort: the outermost braces.
        if let (Some(start), Some(end)) = (body.find('{'), body.rfind('}'))
            && start < end
            && let Ok(v) = serde_json::from_str::<Value>(&body[start..=end])
        {
            return Ok(v);
        }
        Err(crate::error::ProviderError::Malformed {
            provider: self.provider.clone(),
            detail: "the response contained no parsable json object".into(),
        })
    }
}

fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("```") else {
        return s;
    };
    // Drop an optional language tag on the opening fence.
    let rest = rest.split_once('\n').map_or(rest, |(_, body)| body);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

/// One trait, adapters per provider. Doc 10 section 7.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Stable id used in `produced_by.provider` and in the per provider spend
    /// breakdown.
    fn id(&self) -> &str;

    /// Whether the provider constrains output to a schema natively. When false
    /// the adapter falls back to schema prompting.
    fn has_json_mode(&self) -> bool {
        false
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<Completion>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion(text: &str) -> Completion {
        Completion {
            text: text.into(),
            usage: Usage::default(),
            model: "m".into(),
            provider: "test".into(),
            latency_ms: 0,
            stop_reason: None,
        }
    }

    #[test]
    fn parses_bare_json() {
        assert_eq!(completion(r#"{"a":1}"#).json().expect("json")["a"], 1);
    }

    #[test]
    fn parses_json_inside_a_fence() {
        let c = completion("```json\n{\"a\": 2}\n```");
        assert_eq!(c.json().expect("json")["a"], 2);
    }

    #[test]
    fn parses_json_after_a_stray_sentence() {
        let c = completion("Here you go:\n{\"a\": 3}");
        assert_eq!(c.json().expect("json")["a"], 3);
    }

    #[test]
    fn refuses_a_response_with_no_json() {
        assert!(completion("I cannot help with that.").json().is_err());
    }

    #[test]
    fn the_prompt_hash_changes_with_effort() {
        // Effort changes the answer, so it has to change the hash, or replay
        // would claim to have reproduced a call it did not.
        let a = CompletionRequest::new("m", "synthesize")
            .user("q")
            .effort(Effort::High);
        let b = CompletionRequest::new("m", "synthesize")
            .user("q")
            .effort(Effort::Max);
        assert_ne!(a.prompt_hash(), b.prompt_hash());
    }

    #[test]
    fn the_prompt_hash_is_stable_across_identical_requests() {
        let a = CompletionRequest::new("m", "s").system("sys").user("q");
        let b = CompletionRequest::new("m", "s").system("sys").user("q");
        assert_eq!(a.prompt_hash(), b.prompt_hash());
    }
}
