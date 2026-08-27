#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! The UI, served over HTTP against a real core.
//!
//! The shell talks to the core through `window.__TAURI__.core.invoke('rpc', …)`,
//! which exists only inside the Tauri webview. That makes the UI unreachable to
//! a browser driver, and a screen nobody can drive is a screen whose behaviour
//! is claimed rather than measured: `render.ts` emitted three `data-act` verbs
//! for four milestones and no file listened for any of them.
//!
//! So this serves `app/ui/dist` and one `POST /rpc` endpoint over the same
//! router the shell registers. Point Playwright at it, shim `__TAURI__` to fetch
//! `/rpc`, and every verb on the board is exercisable. Doc 12 phase 11 wants a
//! nightly eval in CI; this is the piece that lets the UI be part of it.
//!
//! The provider is the deterministic mock, so a run costs nothing and answers
//! the same way twice. There is no authentication and it binds to loopback: it
//! is a test harness, and it is never part of a shipped build.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use tessera_core::{Core, build_router};
use tessera_providers::{MockProvider, MockResponse};

/// A provider that answers every stage, every time.
///
/// `MockProvider::on` queues one response and then falls through to garbage,
/// which is right for a test asserting one card and wrong here: the second card
/// on a board found an exhausted script and failed with a schema violation, and
/// the failure looked like a bug in the follow-up verb rather than in the
/// fixture. A scripted default is consulted rather than consumed, so it answers
/// for as long as the server runs.
fn mock() -> Arc<MockProvider> {
    Arc::new(
        MockProvider::new().with_default(MockResponse::Scripted(Arc::new(|request| {
            match request.stage.as_str() {
                "route" => MockResponse::Json(json!({
                    "classification": {
                        "question_type": "definitional",
                        "regulatory_stakes": false,
                        "audience_id": null,
                        "language": "en",
                        "needs_current_information": false,
                        "needs_internal_documents": false,
                        "needs_structured_data": false,
                        "entities": ["world model"],
                        "is_follow_up_of_context": false
                    }
                })),
                "plan" => MockResponse::Json(json!({
                    "sub_questions": [{
                        "sq_id": "sq1",
                        "text": "what is being asked",
                        "purpose": "establish the definition",
                        "retrievers": [{ "id": "local", "query": "world model" }]
                    }],
                    "constraints": { "must_exclude": [], "value_policy": "cite_only" }
                })),
                "synthesize" => MockResponse::Json(json!({
                    "answer": "A world model is an internal representation an agent uses to \
                               predict how a situation will change. It lets the agent try an \
                               action in simulation before trying it for real.",
                    "findings": ["A world model predicts state, not text."],
                    "structured_summary": {
                        "entities": ["World model", "Perception", "Dynamics predictor"],
                        "relations": [
                            { "from": "World model", "to": "Perception", "kind": "has" },
                            { "from": "World model", "to": "Dynamics predictor", "kind": "has" }
                        ]
                    }
                })),
                "visualize" => MockResponse::Json(json!({
                    "title": "Parts of a world model",
                    "payload": {
                        "root": {
                            "label": "World model",
                            "children": [
                                { "label": "Perception", "note": "Turns observations into a state." },
                                { "label": "Dynamics predictor", "note": "Predicts the next state." }
                            ]
                        }
                    }
                })),
                // Doc 07 sections B8.2 and B8.5 both run on the verify stage and
                // want different shapes, so answering one fails the other and
                // the card is held back. Fail closed is right; a fixture that
                // wants an admitted card has to answer both.
                "verify" => verify_reply(request),
                // The same contract as the rest of this fixture: it quotes the
                // cards in its prompt and invents nothing, so doc 08 section 5's
                // traceability rule passes for a reason rather than by luck.
                "exercise" => exercise_reply(request),
                _ => MockResponse::Garbage,
            }
        }))),
    )
}

fn exercise_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    let mut prompt = String::new();
    for message in &request.messages {
        for block in &message.content {
            if let tessera_providers::ContentBlock::Text { text } = block {
                prompt.push('\n');
                prompt.push_str(text);
            }
        }
    }

    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut card_id: Option<String> = None;
    for line in prompt.lines() {
        let line = line.trim();
        if let Some(id) = line.strip_prefix("card_id: ") {
            card_id = Some(id.to_string());
        } else if let Some(answer) = line.strip_prefix("answer: ")
            && let Some(id) = card_id.clone()
        {
            let claim = answer
                .split_once(". ")
                .map(|(f, _)| f.to_string())
                .unwrap_or_else(|| answer.to_string());
            items.push(json!({
                "id": format!("i{}", items.len() + 1),
                "kind": "recall",
                "prompt": "What does this card say a world model is?",
                "options": [
                    { "id": "a", "text": claim },
                    { "id": "b", "text": "This card does not say." },
                    { "id": "c", "text": "The card gives a range rather than a definition." },
                    { "id": "d", "text": "The card defers to a later source." },
                ],
                "answer_id": "a",
                "explanation": "The card states it in its opening sentence.",
                "source_card_id": id,
            }));
        }
    }
    MockResponse::Json(json!({ "items": items }))
}

fn verify_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    let mut prompt = String::new();
    for message in &request.messages {
        for block in &message.content {
            if let tessera_providers::ContentBlock::Text { text } = block {
                prompt.push('\n');
                prompt.push_str(text);
            }
        }
    }

    if prompt.contains("For each rule, say whether it matches") {
        let matches: Vec<serde_json::Value> = prompt
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- "))
            .filter_map(|line| line.split_once(": "))
            .map(|(rule_id, _)| json!({ "rule_id": rule_id, "matched": false }))
            .collect();
        return MockResponse::Json(json!({ "matches": matches }));
    }

    let verdicts: Vec<serde_json::Value> = (1..=6)
        .map(|n| json!({ "n": n, "verdict": "supported", "reason": "The passage states it." }))
        .collect();
    MockResponse::Json(json!({ "verdicts": verdicts }))
}

struct Args {
    ui: PathBuf,
    port: u16,
}

fn args() -> Args {
    let mut ui = PathBuf::from("app/ui/dist");
    let mut port = 8732u16;
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--ui" if i + 1 < raw.len() => {
                ui = PathBuf::from(&raw[i + 1]);
                i += 2;
            }
            "--port" if i + 1 < raw.len() => {
                port = raw[i + 1].parse().expect("--port takes a number");
                i += 2;
            }
            other => panic!("unknown argument: {other}"),
        }
    }
    Args { ui, port }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// Resolve a request path inside the UI directory.
///
/// Every `..` segment is dropped rather than resolved, so a request cannot walk
/// out of the served directory even though this only ever listens on loopback.
fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = path.split('?').next().unwrap_or(path);
    let mut out = root.to_path_buf();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        out.push(segment);
    }
    if out == root || out.is_dir() {
        out.push("index.html");
    }
    out
}

fn handle(stream: &mut TcpStream, root: &Path, core: &mut Core, router: &tessera_core::Router<Core>) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.trim().is_empty() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }

    // Test surface, not product surface. A driver asks for a core with no
    // boards on it so each test starts from a first run rather than from
    // whatever the last one left behind, which is the difference between
    // asserting "a second card appeared" and asserting "six cards exist".
    if method == "POST" && path == "/reset" {
        *core = Core::in_memory(mock()).expect("core comes up");
        core.use_pack("general").expect("pack");
        respond(stream, "200 OK", "application/json; charset=utf-8", b"{\"reset\":true}");
        return;
    }

    if method == "POST" && path == "/rpc" {
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            respond(stream, "400 Bad Request", "text/plain", b"short body");
            return;
        }
        let raw = String::from_utf8_lossy(&body);
        // The same entry point the Tauri IPC command uses, so this transport
        // cannot drift from the one the shell ships. A `None` is a notification,
        // which has no reply and which the shell never sends.
        let reply = router
            .dispatch_str(core, &raw)
            .unwrap_or_else(|| "{}".to_string());
        respond(stream, "200 OK", "application/json; charset=utf-8", reply.as_bytes());
        return;
    }

    let file = resolve(root, &path);
    match std::fs::read(&file) {
        Ok(body) => respond(stream, "200 OK", content_type(&file), &body),
        Err(_) => respond(stream, "404 Not Found", "text/plain; charset=utf-8", b"not here"),
    }
}

fn main() {
    let args = args();
    if !args.ui.join("index.html").is_file() {
        eprintln!(
            "no index.html under {}. Run `pnpm --dir app/ui build` first.",
            args.ui.display()
        );
        std::process::exit(2);
    }

    let mut core = Core::in_memory(mock()).expect("core comes up");
    core.use_pack("general").expect("pack");
    let router = build_router();

    let listener = TcpListener::bind(("127.0.0.1", args.port)).expect("bind");
    println!("serving {} on http://127.0.0.1:{}", args.ui.display(), args.port);

    // One connection at a time on purpose. The core is a `&mut` and the point of
    // this binary is to drive one board deterministically, so a queue behind a
    // running card is the honest behaviour rather than a lock nobody reads.
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => handle(&mut stream, &args.ui, &mut core, &router),
            Err(e) => eprintln!("connection failed: {e}"),
        }
    }
}
