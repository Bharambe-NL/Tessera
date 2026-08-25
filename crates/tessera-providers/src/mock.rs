//! The deterministic mock provider. Pattern 18.
//!
//! Doc 12 phase 2 requires it, and doc 03 section 12 says what it is for: "a
//! scenario file per failure type in section 10 with the mock provider returning
//! malformed JSON, timeouts, and auth errors, asserting the recovery recipe ran
//! and the events were emitted."
//!
//! Two properties matter. It is deterministic, so a test that passes once passes
//! every time. And its default is garbage, so a stage nobody scripted exercises
//! the fail closed path rather than quietly succeeding. Doc 12 operating
//! principle 5: the mock provider returning garbage must produce a flagged card.
//!
//! This is a test fixture, not a user facing fallback. Doc 06 section A10 is
//! explicit that an unreachable provider never silently becomes an answer
//! (BN-013).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ProviderError, Result};
use crate::model::{Completion, CompletionRequest, ModelProvider, Usage};

/// A scripted failure. Mirrors the taxonomy in [`ProviderError`] but stays
/// `Clone`, so one scenario can be replayed across a retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockFailure {
    Auth,
    RateLimited,
    Unavailable,
    Timeout,
    Refused(String),
    BadRequest,
}

impl MockFailure {
    fn into_error(self) -> ProviderError {
        match self {
            MockFailure::Auth => ProviderError::Auth {
                provider: "mock".into(),
            },
            MockFailure::RateLimited => ProviderError::RateLimited {
                provider: "mock".into(),
                retry_after_ms: Some(10),
            },
            MockFailure::Unavailable => ProviderError::Unavailable {
                provider: "mock".into(),
                detail: "scripted".into(),
            },
            MockFailure::Timeout => ProviderError::Timeout {
                provider: "mock".into(),
                elapsed_ms: 1,
            },
            MockFailure::Refused(category) => ProviderError::Refused {
                provider: "mock".into(),
                category,
            },
            MockFailure::BadRequest => ProviderError::BadRequest {
                provider: "mock".into(),
                detail: "scripted".into(),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum MockResponse {
    /// A well formed answer.
    Json(Value),
    Text(String),
    /// Well formed json that is not what the schema asked for. The path the
    /// schema guard exists for.
    WrongShape(Value),
    /// Not json at all. Doc 12 operating principle 5.
    Garbage,
    Fail(MockFailure),
}

#[derive(Debug, Clone)]
pub struct RecordedCall {
    pub stage: String,
    pub model: String,
    pub prompt_hash: String,
    pub had_schema: bool,
}

/// Records every call and answers from a per stage script.
pub struct MockProvider {
    script: Mutex<HashMap<String, Vec<MockResponse>>>,
    calls: Mutex<Vec<RecordedCall>>,
    /// Used when a stage has no script left. Garbage by default, so an
    /// unscripted stage fails closed rather than passing by accident.
    default: Mutex<MockResponse>,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            script: Mutex::new(HashMap::new()),
            calls: Mutex::new(Vec::new()),
            default: Mutex::new(MockResponse::Garbage),
        }
    }

    /// Queue one response for a stage. Repeated calls queue in order, so a test
    /// can script "fail, then succeed" and assert the retry happened.
    pub fn on(self, stage: impl Into<String>, response: MockResponse) -> Self {
        self.push(stage, response);
        self
    }

    pub fn push(&self, stage: impl Into<String>, response: MockResponse) {
        let stage = stage.into();
        match self.script.lock() {
            Ok(mut s) => s.entry(stage).or_default().push(response),
            Err(_) => unreachable!("the mock script mutex is never held across an await"),
        }
    }

    /// Replace the fall through response. Only worth doing in a test that is not
    /// about failure handling.
    pub fn with_default(self, response: MockResponse) -> Self {
        if let Ok(mut d) = self.default.lock() {
            *d = response;
        }
        self
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().map(|c| c.len()).unwrap_or(0)
    }

    pub fn calls_for(&self, stage: &str) -> usize {
        self.calls
            .lock()
            .map(|c| c.iter().filter(|r| r.stage == stage).count())
            .unwrap_or(0)
    }

    /// True when every scripted response was consumed. A test that scripts three
    /// responses and sees two used has a bug it would otherwise not notice.
    pub fn script_exhausted(&self) -> bool {
        self.script
            .lock()
            .map(|s| s.values().all(Vec::is_empty))
            .unwrap_or(true)
    }

    fn next_response(&self, stage: &str) -> MockResponse {
        if let Ok(mut script) = self.script.lock()
            && let Some(queue) = script.get_mut(stage)
            && !queue.is_empty()
        {
            return queue.remove(0);
        }
        self.default
            .lock()
            .map(|d| d.clone())
            .unwrap_or(MockResponse::Garbage)
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn has_json_mode(&self) -> bool {
        true
    }

    async fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(RecordedCall {
                stage: request.stage.clone(),
                model: request.model.clone(),
                prompt_hash: request.prompt_hash(),
                had_schema: request.output_schema.is_some(),
            });
        }

        let text = match self.next_response(&request.stage) {
            MockResponse::Json(v) | MockResponse::WrongShape(v) => v.to_string(),
            MockResponse::Text(t) => t,
            MockResponse::Garbage => "not json, and deliberately so".to_string(),
            MockResponse::Fail(f) => return Err(f.into_error()),
        };

        // Token counts are derived from the text so a test asserting on cost gets
        // the same number every run.
        let output_tokens = (text.len() / 4).max(1) as u64;
        let input_tokens = request
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .map(|b| match b {
                crate::model::ContentBlock::Text { text } => text.len() / 4,
                crate::model::ContentBlock::Image { data, .. } => data.len() / 1000,
            })
            .sum::<usize>() as u64;

        Ok(Completion {
            text,
            usage: Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens: 0,
            },
            model: request.model.clone(),
            provider: "mock".into(),
            latency_ms: 0,
            stop_reason: Some("end_turn".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn the_default_is_garbage_so_an_unscripted_stage_fails_closed() {
        let p = MockProvider::new();
        let c = p
            .complete(&CompletionRequest::new("m", "verify").user("q"))
            .await
            .expect("the call itself succeeds");
        assert!(
            c.json().is_err(),
            "an unscripted stage must not yield parsable json"
        );
    }

    #[tokio::test]
    async fn responses_are_consumed_in_order() {
        let p = MockProvider::new()
            .on("route", MockResponse::Fail(MockFailure::Timeout))
            .on("route", MockResponse::Json(json!({ "ok": true })));

        let first = p.complete(&CompletionRequest::new("m", "route").user("q")).await;
        assert!(matches!(first, Err(ProviderError::Timeout { .. })));

        let second = p
            .complete(&CompletionRequest::new("m", "route").user("q"))
            .await
            .expect("the retry succeeds");
        assert_eq!(second.json().expect("json")["ok"], true);
        assert!(p.script_exhausted());
        assert_eq!(p.calls_for("route"), 2);
    }

    #[tokio::test]
    async fn identical_requests_produce_identical_output() {
        // Pattern 18: a test that passes once passes every time.
        let p = MockProvider::new().with_default(MockResponse::Json(json!({ "a": 1 })));
        let req = CompletionRequest::new("m", "s").user("the same question");
        let a = p.complete(&req).await.expect("first");
        let b = p.complete(&req).await.expect("second");
        assert_eq!(a.text, b.text);
        assert_eq!(a.usage, b.usage);
    }

    #[tokio::test]
    async fn every_call_is_recorded_with_its_prompt_hash() {
        let p = MockProvider::new();
        let _ = p
            .complete(
                &CompletionRequest::new("claude-opus-5", "synthesize")
                    .user("q")
                    .expecting(json!({})),
            )
            .await;
        let calls = p.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].stage, "synthesize");
        assert_eq!(calls[0].model, "claude-opus-5");
        assert!(calls[0].had_schema);
        assert_eq!(calls[0].prompt_hash.len(), 64);
    }
}
