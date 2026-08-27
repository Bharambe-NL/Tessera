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
                // Every retriever the fixture configures, so a board question
                // reads the corpus and a notebook question reads the vault. The
                // difference between them is what the run is allowed to open,
                // not what the Planner asked for, which is the thing doc 16
                // section 3.4 is worth testing about.
                "plan" => MockResponse::Json(json!({
                    "sub_questions": [{
                        "sq_id": "sq1",
                        "text": "what is being asked",
                        "purpose": "establish the definition",
                        "retrievers": [
                            { "id": "regulatory", "query": "world model" },
                            { "id": "vault", "query": "world model" },
                            { "id": "boards", "query": "world model" }
                        ]
                    }],
                    "constraints": { "must_exclude": [], "value_policy": "cite_only" }
                })),
                "synthesize" => synthesize_reply(request),
                // Doc 06 section B8: the Visualizer names the shape it chose
                // from the summary, and the fixture answers in that shape. A
                // fixture that always drew a tree would leave doc 16 section
                // 3.5's two types unreachable from the product, which is the
                // only place they can be seen.
                "visualize" => visualize_reply(request),
                // Doc 07 sections B8.2 and B8.5 both run on the verify stage and
                // want different shapes, so answering one fails the other and
                // the card is held back. Fail closed is right; a fixture that
                // wants an admitted card has to answer both.
                "verify" => verify_reply(request),
                // The same contract as the rest of this fixture: it quotes the
                // cards in its prompt and invents nothing, so doc 08 section 5's
                // traceability rule passes for a reason rather than by luck.
                "exercise" => exercise_reply(request),
                // Doc 14. Like the others it quotes rather than judges, so doc
                // 14 section 3.5's four rules pass for a reason.
                "tutor" => tutor_reply(request),
                // Doc 17 section 7's one model call, so the Map and the
                // placement flow are drivable from the product.
                "learning_plan" => learning_plan_reply(request),
                // A mock has no eyes. What this stands in for is the shape of a
                // vision answer, so the deterministic half of doc 07 part A is
                // drivable: the injection check, the summary mapping and the
                // card. Whether a real model recovers a real table is measured
                // on a live vision run and nowhere else.
                "read" => MockResponse::Json(json!({
                    "description": "A hand drawn table of two rules and the values beside them.",
                    "recovered_structure": {
                        "kind": "table",
                        "table": {
                            "columns": ["Rule", "Value"],
                            "rows": [
                                ["the model validation interval", "20 months"],
                                ["the confidence level", "96.5 per cent"]
                            ]
                        },
                        "text_blocks": [{ "text": "Rule", "bbox": [0, 0, 40, 12] }]
                    },
                    "detected_source_markers": [],
                    "notable": [{ "text": "20 months", "kind": "number" }],
                    "legibility": 0.9,
                    "injection_suspected": false,
                    "caveats": []
                })),
                _ => MockResponse::Garbage,
            }
        }))),
    )
}

/// A core that can answer a deep card and remember what it answered.
///
/// The boards retriever alone is not enough: doc 04 section 10's
/// `no_retriever_enabled` refuses a plan with nothing to retrieve from, and doc
/// 05 adds `boards` to every sub-question rather than making it a substitute
/// for one. So this indexes one document in a watched folder as well, which is
/// the smallest thing a deep card needs to run at all.
fn with_memory(core: &mut Core) {
    use tessera_retrievers::chunking::{Chunk, ChunkLocation};
    use tessera_retrievers::{IndexedConfig, index};

    core.use_pack("finance-eu-synthetic").expect("pack");
    core.store
        .conn()
        .execute(
            "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
             VALUES ('reg', ?1, 'corpus/regulatory', 'Central Authority for Prudential Oversight', 'now')",
            rusqlite::params![core.profile_id],
        )
        .expect("folder");
    index::write_document(
        core.store.conn(),
        "reg",
        "reg-car3-v1.md",
        &[Chunk::new(
            "A world model is an internal representation an agent uses to predict how a \
             situation will change.",
            ChunkLocation::ArticleParagraph {
                article: "12".into(),
                paragraph: 1,
            },
            0,
        )],
        None,
        "now",
    )
    .expect("index");

    core.retrievers = tessera_core::retrieval::RetrieverSet {
        indexed: vec![
            ("regulatory".into(), IndexedConfig::regulatory("reg")),
            ("boards".into(), IndexedConfig::boards()),
        ],
        embedder: None,
    };
}

/// A vault with one page in it. Doc 16 section 3.4.
///
/// Layered on top of `with_memory` rather than folded into it, and behind its
/// own flag, because a page that answers the same question as the corpus
/// changes what every other test sees: the card that rests on it is flagged for
/// page sole support, and a flagged card is not remembered, which is the
/// premise doc 15's own test is built on.
fn with_vault(core: &mut Core) {
    use tessera_retrievers::IndexedConfig;

    let profile_id = core.profile_id.clone();
    let pack_id = core.active_pack_id().expect("pack id");
    // Written through the path the Pages view writes through, so what
    // Playwright drives is what a person would have.
    tessera_core::vault::write_page(
        &mut core.store,
        &profile_id,
        Some(&pack_id),
        None,
        "World models",
        "# World models\n\nA world model is an internal representation an agent uses to \
         predict how a situation will change. I wrote this down after reading about it.",
    )
    .expect("a page in the vault");

    core.retrievers.indexed.push((
        "vault".into(),
        IndexedConfig::pages(vec![tessera_retrievers::VAULT_FOLDER.to_string()]),
    ));
}

/// A rated concept, so a lesson has something on the frontier to work on.
///
/// Doc 17 section 3 puts a learner on the frontier from their own ratings, and
/// doc 17 section 4's ladder moves a concept. A profile with an empty map has
/// no frontier, so a check names no concept and nothing adapts, which is what a
/// learner who has rated nothing would actually get.
fn with_learning(core: &mut Core) {
    use tessera_store::repo;

    let profile_id = core.profile_id.clone();
    let pack_id = core.active_pack_id().expect("pack id");

    // Five concepts in a prerequisite chain, so the map has depth to layer by
    // and the frontier lands somewhere in the middle of it rather than on
    // everything. Written through the same calls the product uses: a fixture
    // that reached past them would draw a map the product cannot produce.
    let terms = [
        "state space",
        "transition function",
        "world model",
        "planning horizon",
        "model based control",
    ];
    let ids: Vec<String> = terms
        .iter()
        .map(|term| {
            repo::ensure_concept(&mut core.store, &profile_id, &pack_id, term).expect("a concept on the map")
        })
        .collect();

    for pair in ids.windows(2) {
        repo::propose_edge(
            &mut core.store,
            repo::NewEdge {
                from_concept_id: &pair[0],
                to_concept_id: &pair[1],
                relation: "prerequisite_of",
                proposed_by: "path",
                status: "confirmed",
                weight: 1.0,
            },
        )
        .expect("a prerequisite");
    }
    // Doc 17 section 7: an agent's guess is proposed until the learner says so,
    // and the map draws it dotted. One of them, so both edge kinds are on
    // screen.
    repo::propose_edge(
        &mut core.store,
        repo::NewEdge {
            from_concept_id: &ids[0],
            to_concept_id: &ids[3],
            relation: "prerequisite_of",
            proposed_by: "learning_planner",
            status: "proposed",
            weight: 0.6,
        },
    )
    .expect("a proposal");

    // Doc 17 section 2.1: a rating is a claim, and a claim of 2 or more is what
    // puts a concept on the frontier. Three of the five are claimed and none of
    // them has been checked, so section 3's rule puts the frontier on the
    // shallowest of the three rather than on the deepest: a learner who has
    // only ever claimed starts at the bottom of what they claimed, which is
    // what catches the overconfident rater in the first two questions.
    for id in ids.iter().take(2) {
        repo::rate_concept(&mut core.store, id, 3).expect("a rating");
    }
    repo::rate_concept(&mut core.store, &ids[2], 2).expect("a rating");
}

fn tutor_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    // Doc 14's turns, from the one fixture the eval's grounded mock also reads.
    // Two scripts would score two products.
    MockResponse::Json(tessera_core::fixtures::tutor(&prompt_of(request)))
}

fn learning_plan_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    MockResponse::Json(tessera_core::fixtures::learning_plan(&prompt_of(request)))
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

/// Quote the passages the prompt carried, and cite them.
///
/// The fixed answer this used to return cited nothing, so every card it wrote
/// was flagged `unsupported_claim` and no card was ever eligible to be
/// remembered: doc 15 section 3 rules out a card with an open block flag. The
/// dev server could not produce a verified card at all, which meant doc 12's
/// walkthrough line 15 had no path through the product.
///
/// With no passages it answers as before, so every test written against a core
/// with no retrievers keeps the card it had.
fn synthesize_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    let prompt = prompt_of(request);
    let passages = passages_in(&prompt);

    if passages.is_empty() {
        // Doc 16 section 3.5's two shapes are chosen from the summary, so the
        // only way to see one on screen is for the fixture to write a summary
        // that has one. The question says which: a loop, or a subject with two
        // quantities to it.
        if prompt.to_lowercase().contains("loop") {
            return MockResponse::Json(json!({
                "answer": "A draft goes to review, and review returns it to the draft.",
                "findings": [],
                "structured_summary": {
                    "entities": ["Draft", "Review"],
                    "relations": [
                        { "from": "Draft", "to": "Review", "kind": "goes to" },
                        { "from": "Review", "to": "Draft", "kind": "returns to" }
                    ]
                }
            }));
        }
        if prompt.to_lowercase().contains("in numbers") {
            return MockResponse::Json(json!({
                "answer": "The hall opened in 1949 and has 120 m of floor space.",
                "findings": [],
                "structured_summary": {
                    "entities": ["The hall"],
                    "values": [
                        { "label": "opened", "value": "1949", "unit": "" },
                        { "label": "floor space", "value": "120", "unit": "m" }
                    ]
                }
            }));
        }
        return MockResponse::Json(json!({
            "answer": "A world model is an internal representation an agent uses to predict how \
                       a situation will change. It lets the agent try an action in simulation \
                       before trying it for real.",
            "findings": ["A world model predicts state, not text."],
            "structured_summary": {
                "entities": ["World model", "Perception", "Dynamics predictor"],
                "relations": [
                    { "from": "World model", "to": "Perception", "kind": "has" },
                    { "from": "World model", "to": "Dynamics predictor", "kind": "has" }
                ]
            }
        }));
    }

    // The marker goes inside the sentence, before its full stop. The
    // Synthesizer binds citations by walking sentences and reading the `[n]`
    // markers in each, so a marker placed after the stop is its own sentence
    // and leaves the claim it belongs to unsupported. The first version of this
    // did exactly that and every card came back flagged.
    let mut answer = String::new();
    for (ordinal, text) in passages.iter().take(4) {
        let body = text.trim().trim_end_matches('.');
        answer.push_str(&format!("{body} [{ordinal}]. "));
    }

    MockResponse::Json(json!({
        "answer": answer.trim(),
        "findings": [],
        "structured_summary": {
            "entities": ["World model", "Perception"],
            "relations": [{ "from": "World model", "to": "Perception", "kind": "has" }]
        }
    }))
}

/// Lay the summary out in the shape the Visualizer asked for.
///
/// The type is read back out of the prompt rather than guessed, because the
/// Visualizer chose it from the summary and a payload of the wrong shape is
/// declined: the card would arrive with no visual and the screen would say
/// nothing about why.
fn visualize_reply(request: &tessera_providers::CompletionRequest) -> MockResponse {
    let prompt = prompt_of(request);
    let visual_type = prompt
        .split(" as a ")
        .nth(1)
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("tree")
        .trim();

    MockResponse::Json(match visual_type {
        "flow" => json!({
            "title": "The review loop",
            "payload": {
                "nodes": [{ "id": "a", "label": "Draft" }, { "id": "b", "label": "Review" }],
                "edges": [
                    { "from": "a", "to": "b", "label": "goes to" },
                    { "from": "b", "to": "a", "label": "returns to" }
                ]
            }
        }),
        "stats" => json!({
            "title": "The hall in numbers",
            "payload": { "tiles": [
                { "value": "1949", "unit": "", "label": "opened" },
                { "value": "120", "unit": "m", "label": "floor space" }
            ]}
        }),
        _ => json!({
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
        }),
    })
}

/// Every `<passage n="…">` the prompt carried, in packet order.
fn passages_in(prompt: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut rest = prompt;
    while let Some(start) = rest.find("<passage n=\"") {
        let after = &rest[start + "<passage n=\"".len()..];
        let Some(quote) = after.find('"') else { break };
        let Ok(ordinal) = after[..quote].parse::<usize>() else {
            rest = after;
            continue;
        };
        let Some(open_end) = after.find('>') else { break };
        let body = &after[open_end + 1..];
        let Some(close) = body.find("</passage>") else {
            break;
        };
        out.push((ordinal, body[..close].trim().to_string()));
        rest = &body[close..];
    }
    out
}

/// Every text block of a request, joined.
fn prompt_of(request: &tessera_providers::CompletionRequest) -> String {
    let mut prompt = String::new();
    for message in &request.messages {
        for block in &message.content {
            if let tessera_providers::ContentBlock::Text { text } = block {
                prompt.push('\n');
                prompt.push_str(text);
            }
        }
    }
    prompt
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
    // `/reset?keyless=1` gives back a profile with nothing in its keychain,
    // which is what a fresh install actually looks like and what the first run
    // screen exists for. The default reset seeds a key, because every other
    // test needs a core that can answer.
    if method == "POST" && path.starts_with("/reset") {
        *core = if path.contains("keyless") {
            Core::in_memory_with_keys(
                mock(),
                Box::new(tessera_providers::MemoryKeyStore::new()),
                "test-key",
            )
            .expect("core comes up")
        } else {
            Core::in_memory(mock()).expect("core comes up")
        };
        core.use_pack("general").expect("pack");
        // `/reset?memory=1` turns the boards retriever on, which is what lets a
        // card on one board build on a verified card from another. Off by
        // default so every other test keeps the core it was written against:
        // memory adds an own_card source to a board that had none, and the
        // Library counts sources.
        if path.contains("memory") {
            with_memory(core);
        }
        // `/reset?vault=1` adds a page to the vault, which is what a notebook
        // question reads. Its own flag rather than part of the memory fixture:
        // see `with_vault`.
        if path.contains("vault") {
            with_vault(core);
        }
        // `/reset?learning=1` puts one rated concept on the map, which is what
        // doc 17 section 4's ladder needs to have something to move. Its own
        // flag: a concept on the map is a Library row, and the Library test
        // asserts what a proposed concept looks like on a profile that had
        // none.
        if path.contains("learning") {
            with_learning(core);
        }
        respond(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            b"{\"reset\":true}",
        );
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
        respond(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            reply.as_bytes(),
        );
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
