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
            // BN-036: the model no longer answers a domain; the label is
            // observed by the keyword pass. Stakes is what it answers now, and
            // a question of plain understanding carries none.
            "regulatory_stakes": false,
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
    // card must say it found nothing, never fall back to model knowledge. The
    // finance pack, because its retrievers are enabled: a pack with none is doc
    // 04 section 10's failure, tested separately, not this honest thin card.
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
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
fn the_router_asks_about_stakes_and_never_enumerates_domains() {
    // BN-036, the owner's decision after two paid sweeps. A bare domain list
    // made the bulk model guess the first name; a vocabulary list made it
    // answer unknown for anything the list missed, and the list always misses,
    // because nobody can enumerate what users will ask. The model is asked one
    // question it can answer in any domain without being taught: does this
    // carry regulatory stakes. The domain label survives only as an observed
    // annotation from the free keyword pass.
    let provider = mock();
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("the shipped pack loads");

    let board_id = core.create_board("Board", "fast").expect("board");
    core.ask(&board_id, "what applies when a customer initiates a transfer?", None)
        .expect("the card runs");

    let route = provider
        .calls()
        .into_iter()
        .find(|c| c.stage == "route")
        .expect("the route call happened");
    assert!(
        route.prompt.contains("regulatory_stakes"),
        "the stakes question is missing from the route prompt"
    );
    assert!(
        !route.prompt.contains("Available domains"),
        "the prompt is enumerating domains again"
    );
    assert!(
        !route.prompt.contains("strong customer authentication"),
        "the prompt is teaching vocabulary again"
    );

    // The label is observed, not judged: no vocabulary term appears in the
    // question, so the keyword pass stayed silent and the label is unknown.
    let events = core.store.events(Some(&board_id)).expect("events");
    let routed = events
        .iter()
        .find(|e| e.event_type == "card.routed.v1")
        .expect("routed");
    assert_eq!(routed.payload["domain"], "unknown");
}

#[test]
fn an_unknown_domain_costs_the_card_nothing() {
    // The bug the taxonomy hid: `unknown` was silently removing the regulatory
    // retriever from the plan, so the model's honest uncertainty stripped the
    // card of its ranked source. Retrieval is ungated now: every enabled
    // evidence retriever joins whatever the label says.
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
    core.ask(&board_id, "how do the new rules treat this?", Some("research"))
        .expect("runs");

    let events = core.store.events(Some(&board_id)).expect("events");
    let planned = events
        .iter()
        .find(|e| e.event_type == "card.planned.v1")
        .expect("planned");
    let ids = planned.payload["retriever_ids"].as_array().expect("ids");
    for id in ["regulatory", "local", "web"] {
        assert!(
            ids.iter().any(|i| i == id),
            "{id} was gated out of an unknown-domain plan: {ids:?}"
        );
    }
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

#[test]
fn a_deep_card_reaches_the_synthesizer_with_passages_from_the_index() {
    // The point of M6. Every deep card before this was honest and empty:
    // pipeline.rs handed the Synthesizer an empty vector and doc 06 section
    // A10 turned it into "no sources found". This is the first one that is not.
    use tessera_retrievers::{IndexedConfig, chunking::Chunk, chunking::ChunkLocation, index};

    let provider = Arc::new(
        MockProvider::new()
            .on("route", MockResponse::Json(router_output(true)))
            .on("plan", MockResponse::Json(plan_output()))
            .on("synthesize", MockResponse::Json(synth_output()))
            .on("visualize", MockResponse::Json(visual_output())),
    );
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("pack");

    // A watched folder with one document in it.
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
            "The capital conservation buffer for a significant institution is 2.5 %.",
            ChunkLocation::ArticleParagraph { article: "12".into(), paragraph: 1 },
            0,
        )],
        None,
        "now",
    )
    .expect("index");

    core.retrievers = tessera_core::retrieval::RetrieverSet {
        indexed: vec![("regulatory".into(), IndexedConfig::regulatory("reg"))],
        embedder: None,
    };

    let board_id = core.create_board("Board", "deep").expect("board");
    core.ask(&board_id, "what is the capital conservation buffer?", Some("deep"))
        .expect("the card runs");

    // The passages reached the Synthesizer's packet, which is the contract
    // doc 06 section A4 describes.
    let synth = provider
        .calls()
        .into_iter()
        .find(|c| c.stage == "synthesize")
        .expect("the synthesizer ran");
    assert!(
        synth.prompt.contains("capital conservation buffer"),
        "the passage never reached the prompt"
    );

    // And the retrieval is in the audit trail, with a real Source behind it.
    let events: Vec<String> = core
        .store
        .events(Some(&board_id))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(events.contains(&"retrieval.started.v1".to_string()), "{events:?}");
    assert!(events.contains(&"retrieval.completed.v1".to_string()), "{events:?}");
    assert!(events.contains(&"source.created.v1".to_string()), "{events:?}");

    let sources: i64 = core
        .store
        .conn()
        .query_row("SELECT count(*) FROM source", [], |r| r.get(0))
        .expect("count");
    assert_eq!(sources, 1, "the retrieval did not persist a source");
}

#[test]
fn a_fast_card_never_retrieves() {
    // Doc 06 section A8: a fast card is written from model knowledge and
    // marked unverified. Going to the corpus for it would be a different
    // product, and would also cost the user a retrieval they did not ask for.
    use tessera_retrievers::{IndexedConfig, chunking::Chunk, chunking::ChunkLocation, index};

    let provider = mock();
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("pack");

    core.store
        .conn()
        .execute(
            "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
             VALUES ('reg', ?1, 'r', 'Authority', 'now')",
            rusqlite::params![core.profile_id],
        )
        .expect("folder");
    index::write_document(
        core.store.conn(),
        "reg",
        "doc.md",
        &[Chunk::new("The buffer is 2.5 %.", ChunkLocation::Whole, 0)],
        None,
        "now",
    )
    .expect("index");
    core.retrievers = tessera_core::retrieval::RetrieverSet {
        indexed: vec![("regulatory".into(), IndexedConfig::regulatory("reg"))],
        embedder: None,
    };

    let board_id = core.create_board("Board", "fast").expect("board");
    core.ask(&board_id, "what is the buffer?", Some("fast")).expect("runs");

    let events: Vec<String> = core
        .store
        .events(Some(&board_id))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(
        !events.contains(&"retrieval.started.v1".to_string()),
        "a fast card went to the corpus: {events:?}"
    );
}

#[test]
fn a_verified_card_is_remembered_and_recalled_on_another_board() {
    // Doc 15's whole point, end to end: a card answered on one board becomes
    // context on another, and the card that uses it records builds_on. Doc 15
    // section 2's rule is what keeps it honest, and that rule is the Verifier's
    // job at M8; this is the retrieval half of it.
    use tessera_retrievers::{IndexedConfig, boards};

    let provider = Arc::new(
        MockProvider::new()
            .on("route", MockResponse::Json(router_output(true)))
            .on("plan", MockResponse::Json(plan_output()))
            .on("synthesize", MockResponse::Json(synth_output()))
            .on("visualize", MockResponse::Json(visual_output())),
    );
    let mut core = core_with(Arc::clone(&provider));
    core.use_pack("finance-eu-synthetic").expect("pack");
    core.retrievers = tessera_core::retrieval::RetrieverSet {
        indexed: vec![("boards".into(), IndexedConfig::boards())],
        embedder: None,
    };

    // A first board answers a question at deep, which makes it eligible.
    let first = core.create_board("First", "deep").expect("board");
    core.ask(&first, "what is the capital conservation buffer?", Some("deep"))
        .expect("first card");

    let indexed: i64 = core
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM index_entry WHERE folder_id = ?1",
            rusqlite::params![boards::BOARDS_FOLDER],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(indexed, 1, "the answered card was not remembered");

    // A second board asks something related and should recall it.
    let second = core.create_board("Second", "deep").expect("board");
    core.ask(&second, "how does the capital conservation buffer apply?", Some("deep"))
        .expect("second card");

    let events: Vec<String> = core
        .store
        .events(Some(&second))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert!(events.contains(&"retrieval.completed.v1".to_string()), "{events:?}");

    // The prior card arrived as its own source class, which is what lets the
    // Verifier single it out at M8.
    let own_card: i64 = core
        .store
        .conn()
        .query_row("SELECT count(*) FROM source WHERE class = 'own_card'", [], |r| r.get(0))
        .expect("count");
    assert_eq!(own_card, 1, "the prior card did not arrive as own_card");

    // And the new card records what it was built on. Doc 01 section 4.4.
    let builds_on: String = core
        .store
        .conn()
        .query_row(
            "SELECT builds_on FROM card WHERE board_id = ?1",
            rusqlite::params![second],
            |r| r.get(0),
        )
        .expect("card");
    assert!(builds_on.contains(&first), "builds_on did not name the board it came from: {builds_on}");
}

// ------------------------------------------------------ follow-up context --
// Doc 03 section 4 hands the Router the parent card; doc 04 section 4 hands the
// Planner up to three ancestors and section 9 puts "carrying the board context
// into each sub-question" in the Planner's scope. Both were built with the
// field hardcoded to null, so a follow-up reached the retrievers as a question
// with no subject. Measured through the pipeline, retrieval recall on
// standalone questions was 1.000 and on follow-ups 0.485.

fn packet_for(core: &Core, board_id: &str, agent: &str) -> Value {
    core.store
        .conn()
        .query_row(
            "SELECT s.task_packet FROM step s JOIN run r ON r.id = s.run_id
             WHERE r.board_id = ?1 AND s.agent_id = ?2
             ORDER BY s.started_at DESC, s.sequence DESC LIMIT 1",
            rusqlite::params![board_id, agent],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

#[test]
fn a_follow_up_carries_its_parent_into_the_router_packet() {
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
    let board_id = core.create_board("Board", "deep").expect("board");
    let parent = core
        .ask(&board_id, "what are world models?", Some("deep"))
        .expect("parent runs");

    core.ask_on(&board_id, "which article says so?", Some("deep"), Some(&parent.card_id))
        .expect("follow up runs");

    let packet = packet_for(&core, &board_id, "router");
    assert_eq!(packet["request"]["kind"], "follow", "a follow-up was routed as a root");
    assert_eq!(packet["parent"]["card_id"], parent.card_id.as_str());
    assert_eq!(packet["parent"]["question"], "what are world models?");
    assert!(
        packet["parent"]["answer"].as_str().is_some_and(|a| a.contains("world model")),
        "the parent's answer did not reach the Router"
    );
}

#[test]
fn a_follow_up_carries_its_ancestors_into_the_planner_packet() {
    // The Planner is the one that has to resolve "which article says so?" into
    // something a retriever can match, and it cannot do that from an empty
    // array.
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
    let board_id = core.create_board("Board", "deep").expect("board");
    let parent = core
        .ask(&board_id, "what are world models?", Some("research"))
        .expect("parent runs");

    core.ask_on(&board_id, "which article says so?", Some("research"), Some(&parent.card_id))
        .expect("follow up runs");

    let packet = packet_for(&core, &board_id, "planner");
    let ancestors = packet["context"]["ancestors"].as_array().expect("ancestors");
    assert_eq!(ancestors.len(), 1, "the ancestor chain was empty");
    assert_eq!(ancestors[0]["question"], "what are world models?");
    assert!(
        ancestors[0]["answer_excerpt"]
            .as_str()
            .is_some_and(|a| a.contains("world model")),
        "the ancestor arrived without its answer"
    );
}

#[test]
fn a_root_card_still_reports_no_parent() {
    // The other half of the same promise: context is carried where it exists
    // and never invented where it does not.
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
    let board_id = core.create_board("Board", "deep").expect("board");
    core.ask(&board_id, "what are world models?", Some("deep")).expect("runs");

    let packet = packet_for(&core, &board_id, "router");
    assert_eq!(packet["request"]["kind"], "root");
    assert_eq!(packet["parent"], Value::Null);
}

#[test]
fn the_ancestor_chain_stops_at_three() {
    // Doc 04 section 4 caps it, and the schema rejects a fourth. A deep thread
    // must not put the whole board into a prompt.
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
    let board_id = core.create_board("Board", "deep").expect("board");

    let mut previous = core
        .ask(&board_id, "what are world models?", Some("research"))
        .expect("root")
        .card_id;
    for i in 0..4 {
        previous = core
            .ask_on(&board_id, &format!("and what about {i}?"), Some("research"), Some(&previous))
            .expect("follow up")
            .card_id;
    }

    let packet = packet_for(&core, &board_id, "planner");
    let ancestors = packet["context"]["ancestors"].as_array().expect("ancestors");
    assert_eq!(ancestors.len(), 3, "the chain was not capped");
}

#[test]
fn the_board_seed_reaches_the_planner() {
    // Doc 04 section 9 lists the board seed alongside the parent answer. It was
    // null while the board carried it, so a board opened from a seed answered
    // as though it had none.
    let mut core = core_with(mock());
    core.use_pack("finance-eu-synthetic").expect("pack");
    let board_id = core.create_board("Board", "deep").expect("board");
    core.store
        .conn()
        .execute(
            "UPDATE board SET seed_label = 'CAR3 transitional rules' WHERE id = ?1",
            rusqlite::params![&board_id],
        )
        .expect("seed");

    core.ask(&board_id, "what are world models?", Some("research")).expect("runs");

    let packet = packet_for(&core, &board_id, "planner");
    assert_eq!(packet["context"]["board_seed"], "CAR3 transitional rules");
}
