#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! M3 acceptance: a real question goes through Router, Synthesizer, Visualizer
//! and Verifier and lands as a card, and every part of that card is
//! reconstructable from the Event table alone.
//!
//! The provider is the deterministic mock, so these tests assert the pipeline's
//! behaviour rather than a model's. What a real model produces is measured from
//! M4, against the synthetic corpus.

use std::sync::Arc;

use serde_json::{Value, json};
use tessera_core::{Core, build_router, rpc::Request};
use tessera_providers::{MockProvider, MockResponse};

fn router_output(run_id_free: bool) -> Value {
    let _ = run_id_free;
    json!({
        "classification": {
            "question_type": "definitional",
            "domain": "general",
            "audience_id": null,
            "language": "en",
            "needs_current_information": false,
            "needs_internal_documents": false,
            "needs_structured_data": false,
            "entities": ["world model"],
            "is_follow_up_of_context": false
        }
    })
}

fn synth_output() -> Value {
    json!({
        "answer": "A world model is an internal representation an agent uses to predict how a \
    situation will change. It lets the agent try an action in simulation before trying it for real.",
        "findings": ["A world model predicts state, not text."],
        "structured_summary": {
            "entities": ["World model", "Perception", "Dynamics predictor", "Action policy"],
            "relations": [
                { "from": "World model", "to": "Perception", "kind": "has" },
                { "from": "World model", "to": "Dynamics predictor", "kind": "has" },
                { "from": "World model", "to": "Action policy", "kind": "has" }
            ]
        }
    })
}

fn visual_output() -> Value {
    json!({
        "title": "Parts of a world model",
        "payload": {
            "root": {
                "label": "World model",
                "children": [
                    { "label": "Perception", "note": "Turns observations into a compact state." },
                    { "label": "Dynamics predictor", "note": "Predicts the next state given an action." },
                    { "label": "Action policy", "note": "Chooses actions by planning in the model." }
                ]
            }
        }
    })
}

fn mock() -> Arc<MockProvider> {
    Arc::new(
        MockProvider::new()
            .on("route", MockResponse::Json(router_output(true)))
            .on("synthesize", MockResponse::Json(synth_output()))
            .on("visualize", MockResponse::Json(visual_output())),
    )
}

fn core_with(provider: Arc<MockProvider>) -> Core {
    Core::in_memory(provider).expect("core comes up")
}

#[test]
fn a_question_becomes_a_card_with_a_visual() {
    let provider = mock();
    let mut core = core_with(Arc::clone(&provider));

    let board_id = core.create_board("Untitled board", "fast").expect("board");
    let outcome = core
        .ask(&board_id, "what are world models?", None)
        .expect("the card runs");

    assert_eq!(provider.calls_for("route"), 1);
    assert_eq!(provider.calls_for("synthesize"), 1);
    assert_eq!(provider.calls_for("visualize"), 1);

    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board exists");
    assert_eq!(board.cards.len(), 1);

    let card = &board.cards[0];
    assert_eq!(card.id, outcome.card_id);
    assert_eq!(card.question, "what are world models?");
    assert!(card.answer.as_deref().is_some_and(|a| a.contains("world model")));

    let visual = card.visual.as_ref().expect("a visual was produced");
    assert_eq!(visual["type"], "tree");
    assert_eq!(visual["payload"]["root"]["label"], "World model");

    // Every block carries its JSON pointer, which is what makes "Investigate
    // this further" an exact reference rather than a label match.
    let blocks = visual["block_index"].as_array().expect("block index");
    assert_eq!(blocks.len(), 4, "the root and its three children");
    assert!(
        blocks
            .iter()
            .all(|b| b["ref"].as_str().is_some_and(|r| r.starts_with('/')))
    );
}

#[test]
fn a_fast_card_says_it_is_unverified_rather_than_claiming_confidence() {
    // Doc 06 section A8 point 6 and doc 07 section B8.1's fast_mode_notice.
    let mut core = core_with(mock());
    let board_id = core.create_board("Board", "fast").expect("board");
    let outcome = core.ask(&board_id, "what are world models?", None).expect("runs");

    assert_eq!(
        outcome.confidence, 0.0,
        "fast is fixed at 0 and shows as Unverified"
    );

    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board");
    let card = &board.cards[0];
    assert!(card.citations.is_empty(), "fast mode cites nothing");
    assert!(
        card.flags.iter().any(|f| f["rule_id"] == "fast_mode_notice"),
        "the reader is told, got {:?}",
        card.flags
    );
}

#[test]
fn a_deep_card_with_no_retrievers_reports_no_sources_rather_than_guessing() {
    // Doc 06 section A10 no_passages. Retrievers arrive at M6; until then a deep
    // card must say it found nothing, never fall back to model knowledge.
    let mut core = core_with(mock());
    let board_id = core.create_board("Board", "deep").expect("board");
    let outcome = core
        .ask(&board_id, "what changed in the capital rule?", Some("deep"))
        .expect("runs");

    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board");
    let card = &board.cards[0];
    let answer = card.answer.as_deref().unwrap_or_default();

    assert!(answer.starts_with("No sources were found"), "got `{answer}`");
    assert!(card.citations.is_empty());
    assert_eq!(outcome.confidence, 0.0);
}

#[test]
fn the_whole_card_is_reconstructable_from_the_event_log() {
    // The M3 acceptance criterion.
    let mut core = core_with(mock());
    let board_id = core.create_board("Board", "fast").expect("board");
    let outcome = core.ask(&board_id, "what are world models?", None).expect("runs");

    let events = core.store.events(Some(&board_id)).expect("events");
    let types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();

    // The pipeline's spine, in order.
    for expected in [
        "board.created.v1",
        "card.requested.v1",
        "card.routed.v1",
        "card.synthesized.v1",
        "visual.produced.v1",
        "verify.completed.v1",
        "card.answered.v1",
    ] {
        assert!(types.contains(&expected), "missing {expected} in {types:?}");
    }

    // Order matters: an answer cannot precede the routing that chose its depth.
    let at = |t: &str| types.iter().position(|x| *x == t).expect(t);
    assert!(at("card.requested.v1") < at("card.routed.v1"));
    assert!(at("card.routed.v1") < at("card.synthesized.v1"));
    assert!(at("card.synthesized.v1") < at("visual.produced.v1"));
    assert!(at("visual.produced.v1") < at("verify.completed.v1"));
    assert!(at("verify.completed.v1") < at("card.answered.v1"));

    // Every model call is on the record with its prompt hash, so the run can be
    // reproduced. Doc 01 section 6.2.
    let calls: Vec<&tessera_store::Event> = events
        .iter()
        .filter(|e| e.event_type == "model.call.v1")
        .collect();
    assert_eq!(calls.len(), 3, "route, synthesize, visualize");
    assert!(
        calls
            .iter()
            .all(|c| c.payload["prompt_hash"].as_str().is_some_and(|h| h.len() == 64))
    );

    // And the projection rebuilds to the same state.
    let before: Vec<(String, Option<f64>)> = vec![(outcome.status.clone(), Some(outcome.confidence))];
    core.store.rebuild_projections().expect("replay");
    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board");
    assert_eq!(
        vec![(board.cards[0].status.clone(), board.cards[0].confidence)],
        before
    );
}

#[test]
fn garbage_from_the_provider_never_becomes_a_card() {
    // Doc 12 operating principle 5. The mock's default is garbage, so an
    // unscripted synthesize stage exercises this.
    let provider = Arc::new(MockProvider::new().on("route", MockResponse::Json(router_output(true))));
    let mut core = core_with(Arc::clone(&provider));
    let board_id = core.create_board("Board", "fast").expect("board");

    let result = core.ask(&board_id, "what are world models?", None);
    assert!(result.is_err(), "a card must not be admitted from garbage");

    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board");
    assert_eq!(board.cards[0].status, "failed");
    assert!(board.cards[0].answer.is_none(), "no answer was stored");

    let types: Vec<String> = core
        .store
        .events(Some(&board_id))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(types.contains(&"card.failed.v1".to_string()));
    assert!(!types.contains(&"card.answered.v1".to_string()));
}

#[test]
fn a_visualizer_failure_degrades_the_card_rather_than_killing_it() {
    // Doc 06 section B10: a card without a visual is acceptable.
    let provider = Arc::new(
        MockProvider::new()
            .on("route", MockResponse::Json(router_output(true)))
            .on("synthesize", MockResponse::Json(synth_output()))
            .on("visualize", MockResponse::Garbage)
            .on("visualize", MockResponse::Garbage),
    );
    let mut core = core_with(provider);
    let board_id = core.create_board("Board", "fast").expect("board");
    core.ask(&board_id, "what are world models?", None)
        .expect("the card still lands");

    let board = tessera_store::repo::read_board(&core.store, &board_id)
        .expect("read")
        .expect("board");
    let card = &board.cards[0];
    assert!(card.answer.is_some(), "the prose survives");
    assert!(card.visual.is_none(), "the diagram does not");

    let types: Vec<String> = core
        .store
        .events(Some(&board_id))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(types.contains(&"visual.declined.v1".to_string()));
    assert!(types.contains(&"card.answered.v1".to_string()));
}

// ------------------------------------------------------------ rpc surface --

fn call(router: &tessera_core::Router<Core>, core: &mut Core, method: &str, params: Value) -> Value {
    let response = router
        .dispatch(core, Request::new(method, params, 1))
        .expect("a request gets a reply");
    assert!(response.is_ok(), "{method} failed: {:?}", response.error);
    response.result.expect("result")
}

#[test]
fn the_shell_can_drive_a_whole_board_through_the_rpc_surface() {
    // Doc 10 section 2: everything the shell does is a method here, so the web
    // client that arrives later talks to the identical protocol.
    let router = build_router();
    let mut core = core_with(mock());

    let created = call(
        &router,
        &mut core,
        "board.create",
        json!({ "title": "Untitled board", "depth": "fast" }),
    );
    let board_id = created["board_id"].as_str().expect("board id").to_string();

    let asked = call(
        &router,
        &mut core,
        "card.ask",
        json!({ "board_id": board_id, "question": "what are world models?" }),
    );
    assert!(asked["card_id"].as_str().is_some());

    let board = call(&router, &mut core, "board.get", json!({ "board_id": board_id }));
    assert_eq!(board["cards"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        board["title"], "what are world models?",
        "the board takes its name from the first question"
    );

    let listed = call(&router, &mut core, "board.list", json!({}));
    assert_eq!(listed["boards"].as_array().map(Vec::len), Some(1));

    let history = call(
        &router,
        &mut core,
        "board.history",
        json!({ "board_id": board_id }),
    );
    assert!(history["events"].as_array().is_some_and(|e| e.len() > 5));

    // Pattern 25: the UI reads notifications, not raw events.
    let notes = call(
        &router,
        &mut core,
        "board.notifications",
        json!({ "board_id": board_id, "after": 0 }),
    );
    let kinds: Vec<&str> = notes["notifications"]
        .as_array()
        .expect("notifications")
        .iter()
        .filter_map(|n| n["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"card_stage"), "got {kinds:?}");
    assert!(kinds.contains(&"card_answered"));
    assert!(
        !kinds.contains(&"model_call"),
        "the log's detail stays in the log"
    );
}

#[test]
fn an_empty_question_is_refused_with_something_the_user_can_act_on() {
    let router = build_router();
    let mut core = core_with(mock());
    let board_id = core.create_board("Board", "fast").expect("board");

    let response = router
        .dispatch(
            &mut core,
            Request::new("card.ask", json!({ "board_id": board_id, "question": "   " }), 1),
        )
        .expect("reply");
    let error = response.error.expect("an error");
    assert_eq!(error.message, "Type a question first.");
    assert_eq!(error.data.expect("data")["kind"], "empty_question");
}

#[test]
fn asking_with_no_model_key_says_where_to_fix_it() {
    // Doc 03 section 10 policy_unresolvable fails before any retrieval, and doc
    // 11 section 9 wants the message to say what and how: "No search key. Add one
    // in Profile to enable web search." This is the same shape for a model key.
    use tessera_providers::MemoryKeyStore;

    let root = std::env::temp_dir().join(format!("tessera-nokey-{}", tessera_store::new_id()));
    let mut core = Core::open(
        &root,
        // A keystore with no entry: exactly a fresh install before first run.
        Box::new(MemoryKeyStore::new()),
        mock(),
        "anthropic-default",
    )
    .expect("the app still opens without a key");

    let board_id = core.create_board("Board", "fast").expect("board");

    let router = build_router();
    let response = router
        .dispatch(
            &mut core,
            Request::new(
                "card.ask",
                json!({ "board_id": board_id, "question": "what are world models?" }),
                1,
            ),
        )
        .expect("reply");

    let error = response.error.expect("asking without a key must fail");
    assert_eq!(error.message, "No model key. Add one in Profile to answer cards.");
    assert_eq!(error.data.expect("data")["kind"], "policy_unresolvable");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_router_prompt_carries_the_packs_domain_vocabulary() {
    // Measured on the 400 question sweep: the deterministic keyword pass was
    // right 129 times out of 129 and fired on a third of the questions. The
    // other two thirds reached the model as four bare domain names, and the
    // bulk model answered `capital` for most of them, the first name in the
    // list. This pins the fix: the terms the pack holds for each domain reach
    // the classify prompt, so the model has what the keyword pass has.
    let provider = mock();
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("the shipped pack loads");

    let board_id = core.create_board("Board", "fast").expect("board");
    // No safeguarding vocabulary appears in the question, so the keyword pass
    // stays silent and the model is the only thing deciding.
    core.ask(&board_id, "what applies when a customer initiates a transfer?", None)
        .expect("the card runs");

    let route = provider
        .calls()
        .into_iter()
        .find(|c| c.stage == "route")
        .expect("the route call happened");
    assert!(
        route.prompt.contains("strong customer authentication"),
        "the payments vocabulary is missing from the classify prompt"
    );
    assert!(
        route.prompt.contains("risk weighted"),
        "the capital vocabulary is missing from the classify prompt"
    );
    assert!(
        route.prompt.contains("Classify by what the question is about"),
        "the instruction that vocabulary is evidence rather than a checklist is missing"
    );
}

fn plan_output() -> Value {
    json!({
        "sub_questions": [
            {
                "text": "What does the capital rule say about the buffer?",
                "purpose": "Establish the current rule.",
                "queries": { "regulatory": "capital conservation buffer article" }
            },
            {
                "text": "What changed in the latest revision?",
                "purpose": "Establish what moved.",
                "queries": { "web": "capital rule buffer change" },
                "depends_on_previous": true
            }
        ],
        "answer_scope": "The current buffer and what changed, without recommending an action.",
        "caveats": []
    })
}

#[test]
fn a_research_card_is_planned_before_it_is_synthesized() {
    // Doc 04: the Planner runs when the Router set plan_required, and its
    // completion event carries the plan summary doc 04 section 7 declares.
    let provider = Arc::new(
        MockProvider::new()
            .on("route", MockResponse::Json(router_output(true)))
            .on("plan", MockResponse::Json(plan_output()))
            .on("synthesize", MockResponse::Json(synth_output()))
            .on("visualize", MockResponse::Json(visual_output())),
    );
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("pack");

    let board_id = core.create_board("Board", "research").expect("board");
    core.ask(&board_id, "what changed in the capital buffer?", Some("research"))
        .expect("the card runs");

    assert_eq!(provider.calls_for("plan"), 1, "the Planner made its call");

    let events = core.store.events(Some(&board_id)).expect("events");
    let planned = events
        .iter()
        .find(|e| e.event_type == "card.planned.v1")
        .expect("card.planned.v1 was emitted");
    assert_eq!(planned.payload["sub_question_count"], 2);
    let ids = planned.payload["retriever_ids"].as_array().expect("retriever ids");
    assert!(
        ids.iter().any(|i| i == "regulatory"),
        "a governed domain always includes the regulatory retriever: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i == "boards"),
        "memory is on by default, so doc 05 section 8.5 adds boards: {ids:?}"
    );

    // Doc 04 section 7: one entity.resolved.v1 per literal.
    let resolved: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "entity.resolved.v1")
        .collect();
    assert!(!resolved.is_empty(), "the Router's entities were resolved");
    for event in resolved {
        assert_eq!(
            event.payload["ambiguity"], "unknown",
            "no Concept graph exists yet, so every literal is unknown"
        );
    }
}

#[test]
fn no_enabled_retriever_fails_the_card_with_a_pointer_at_the_fix() {
    // Doc 04 section 10 no_retriever_enabled. The general pack enables nothing
    // by default, and with memory switched off there is nothing to plan with.
    // Doc 06 section A10 covers retrieval that found nothing; this covers
    // having nothing to retrieve with, and they must not be confused: one is an
    // honest thin card, the other is a failure that names its fix.
    let mut core = core_with(mock());
    core.store
        .conn()
        .execute("UPDATE profile SET memory_enabled = 0", [])
        .expect("memory off");

    let board_id = core.create_board("Board", "deep").expect("board");
    let error = match core.ask(&board_id, "what changed in the capital rule?", Some("deep")) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a plan with no retrievers is not a plan"),
    };
    assert!(
        error.contains("no_retriever_enabled") || error.contains("No retriever is enabled"),
        "the failure names itself: {error}"
    );
    assert!(error.contains("Profile"), "the failure points at the fix: {error}");
}
