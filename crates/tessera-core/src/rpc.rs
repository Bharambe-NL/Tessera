//! JSON-RPC 2.0 over local IPC. Pattern 25.
//!
//! Doc 10 section 2: "The pipeline, storage, and event log are a library (the
//! core) with a JSON-RPC boundary. The desktop shell is the first client. A
//! future hosted backend and the reduced web client wrap the same core."
//!
//! Doc 10 section 13 says why the boundary is worth its cost now, before there
//! is a second client: "The web client (reduced) is a later target: the same UI
//! against a hosted core with keys in the browser session and only the web
//! retriever enabled. It is out of the build prompt's scope and named here so
//! the RPC boundary is kept clean."
//!
//! Keeping it clean means one rule: the shell may not reach into the core except
//! through a method registered here. A Tauri command that touches the store
//! directly is a shortcut that the web client cannot take.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Standard JSON-RPC 2.0 codes, plus the application range.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// The core's own failures start here. They carry the failure taxonomy code
    /// in `data.kind` so the UI can act on the category rather than the message.
    pub const CORE_ERROR: i32 = -32000;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Absent for a notification, which expects no reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

impl Request {
    pub fn new(method: impl Into<String>, params: Value, id: impl Into<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: Some(params),
            id: Some(id.into()),
        }
    }

    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(codes::METHOD_NOT_FOUND, format!("no method `{method}`"))
    }

    pub fn invalid_params(detail: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, detail)
    }

    /// A core failure, carrying its taxonomy code so the UI can branch on the
    /// category. House style: the message says what happened and how to fix it
    /// (doc 11 section 9), so it is fit to show as is.
    pub fn core(kind: &str, message: impl Into<String>) -> Self {
        Self::new(codes::CORE_ERROR, message).with_data(serde_json::json!({ "kind": kind }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Value,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn err(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result: None,
            error: Some(error),
            id,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

pub type MethodResult = std::result::Result<Value, RpcError>;

/// The registered surface. A method takes params and returns a result.
///
/// Deliberately not async in the signature: the core owns a tokio runtime and
/// blocks inside a handler, so the shell's command layer stays a thin adapter
/// over the same types the socket client will use.
pub type Handler<S> = Box<dyn Fn(&mut S, Option<Value>) -> MethodResult + Send + Sync>;

pub struct Router<S> {
    methods: BTreeMap<String, Handler<S>>,
}

impl<S> Default for Router<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Router<S> {
    pub fn new() -> Self {
        Self {
            methods: BTreeMap::new(),
        }
    }

    pub fn register<F>(&mut self, method: &str, handler: F) -> &mut Self
    where
        F: Fn(&mut S, Option<Value>) -> MethodResult + Send + Sync + 'static,
    {
        self.methods.insert(method.to_string(), Box::new(handler));
        self
    }

    /// Every registered method, for the shell's capability check and for a test
    /// that asserts the surface did not shrink by accident.
    pub fn methods(&self) -> impl Iterator<Item = &str> {
        self.methods.keys().map(String::as_str)
    }

    /// Dispatch one request. Returns `None` for a notification, per JSON-RPC.
    pub fn dispatch(&self, state: &mut S, request: Request) -> Option<Response> {
        let id = request.id.clone().unwrap_or(Value::Null);

        if request.jsonrpc != "2.0" {
            return Some(Response::err(
                id,
                RpcError::new(codes::INVALID_REQUEST, "jsonrpc must be \"2.0\""),
            ));
        }

        let Some(handler) = self.methods.get(&request.method) else {
            if request.is_notification() {
                return None;
            }
            return Some(Response::err(id, RpcError::method_not_found(&request.method)));
        };

        let is_notification = request.is_notification();
        let result = handler(state, request.params);
        if is_notification {
            return None;
        }
        Some(match result {
            Ok(value) => Response::ok(id, value),
            Err(e) => Response::err(id, e),
        })
    }

    /// Dispatch from a raw string, which is what the IPC and socket transports
    /// both hand over.
    pub fn dispatch_str(&self, state: &mut S, raw: &str) -> Option<String> {
        let request: Request = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                let response = Response::err(Value::Null, RpcError::new(codes::PARSE_ERROR, e.to_string()));
                return serde_json::to_string(&response).ok();
            }
        };
        let response = self.dispatch(state, request)?;
        serde_json::to_string(&response).ok()
    }
}

/// Pull a typed params object out of a request.
pub fn params<T: serde::de::DeserializeOwned>(params: Option<Value>) -> std::result::Result<T, RpcError> {
    let value = params.unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| RpcError::invalid_params(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct State {
        calls: usize,
    }

    fn router() -> Router<State> {
        let mut r = Router::new();
        r.register("board.count", |s: &mut State, _| {
            s.calls += 1;
            Ok(json!({ "count": s.calls }))
        });
        r.register("board.rename", |_: &mut State, p| {
            #[derive(serde::Deserialize)]
            struct P {
                title: String,
            }
            let p: P = params(p)?;
            if p.title.trim().is_empty() {
                return Err(RpcError::core("empty_title", "A board needs a title."));
            }
            Ok(json!({ "title": p.title }))
        });
        r
    }

    #[test]
    fn a_registered_method_returns_its_result() {
        let r = router();
        let mut s = State::default();
        let response = r
            .dispatch(&mut s, Request::new("board.count", json!({}), 1))
            .expect("a request gets a reply");
        assert!(response.is_ok());
        assert_eq!(response.result.expect("result")["count"], 1);
    }

    #[test]
    fn an_unknown_method_is_refused_rather_than_ignored() {
        let r = router();
        let mut s = State::default();
        let response = r
            .dispatch(&mut s, Request::new("board.teleport", json!({}), 2))
            .expect("reply");
        assert_eq!(response.error.expect("error").code, codes::METHOD_NOT_FOUND);
    }

    #[test]
    fn a_notification_gets_no_reply_but_still_runs() {
        let r = router();
        let mut s = State::default();
        let request = Request {
            jsonrpc: "2.0".into(),
            method: "board.count".into(),
            params: None,
            id: None,
        };
        assert!(r.dispatch(&mut s, request).is_none());
        assert_eq!(s.calls, 1, "the handler still ran");
    }

    #[test]
    fn bad_params_come_back_as_invalid_params() {
        let r = router();
        let mut s = State::default();
        let response = r
            .dispatch(
                &mut s,
                Request::new("board.rename", json!({ "titel": "typo" }), 3),
            )
            .expect("reply");
        assert_eq!(response.error.expect("error").code, codes::INVALID_PARAMS);
    }

    #[test]
    fn a_core_error_carries_its_taxonomy_kind() {
        // The UI branches on the category, not on the message text.
        let r = router();
        let mut s = State::default();
        let response = r
            .dispatch(&mut s, Request::new("board.rename", json!({ "title": "  " }), 4))
            .expect("reply");
        let e = response.error.expect("error");
        assert_eq!(e.code, codes::CORE_ERROR);
        assert_eq!(e.data.expect("data")["kind"], "empty_title");
        assert_eq!(e.message, "A board needs a title.");
    }

    #[test]
    fn malformed_json_is_a_parse_error_not_a_panic() {
        let r = router();
        let mut s = State::default();
        let raw = r.dispatch_str(&mut s, "{ not json").expect("a reply");
        let response: Response = serde_json::from_str(&raw).expect("parse");
        assert_eq!(response.error.expect("error").code, codes::PARSE_ERROR);
    }

    #[test]
    fn a_wrong_protocol_version_is_refused() {
        let r = router();
        let mut s = State::default();
        let mut request = Request::new("board.count", json!({}), 5);
        request.jsonrpc = "1.0".into();
        let response = r.dispatch(&mut s, request).expect("reply");
        assert_eq!(response.error.expect("error").code, codes::INVALID_REQUEST);
    }

    #[test]
    fn a_successful_response_carries_no_error_key_on_the_wire() {
        let r = router();
        let mut s = State::default();
        let raw = r
            .dispatch_str(
                &mut s,
                &serde_json::to_string(&Request::new("board.count", json!({}), 6)).expect("encode"),
            )
            .expect("reply");
        assert!(!raw.contains("\"error\""), "got {raw}");
        assert!(raw.contains("\"result\""));
    }
}
