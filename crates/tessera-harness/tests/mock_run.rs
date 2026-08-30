#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! M2 acceptance (doc 12 phase 2): "a mock run walks every state and emits the
//! expected events; a crash mid run is reclaimed on restart."
//!
//! The agent under test stands in for the Router: it walks doc 03 section 6's
//! state sequence, makes one call with the small alias, and returns the router
//! output. Using the real schemas and the real state sequence means these tests
//! break when the specs change, which is the point.

use async_trait::async_trait;
use rusqlite::params;
use serde_json::{Value, json};
use tessera_harness::{
    Agent, AgentContext, Failure, Ledger, Recovery, RunAgent, RunKind, run_agent, sequences,
};
use tessera_providers::{CompletionRequest, Effort, MockFailure, MockProvider, MockResponse, ResolvedPolicy};
use tessera_schema::{Registry, ids};
use tessera_store::{Source, Store, new_id, now_iso8601};

// ------------------------------------------------------------------ fixture --

#[derive(Clone)]
struct Ids {
    board_id: String,
    card_id: String,
    run_id: String,
}

/// Removes the profile folder when the test ends, whether it passed or not.
struct TempRoot(std::path::PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    store: Store,
    registry: Registry,
    ids: Ids,
    _root: TempRoot,
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("tessera-m2-{}", new_id()));
    let store = Store::open(&root).expect("store");
    let now = now_iso8601();
    let (profile, pack, board, card, run) = (new_id(), new_id(), new_id(), new_id(), new_id());

    let c = store.conn();
    c.execute(
        "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                    flag_rules, retrievers, exercise_templates, created_at)
         VALUES (?1, 'general', '1.0.0', '[]', '[]', '{}', '[]', '[]', '[]', ?2)",
        params![pack, now],
    )
    .expect("pack");
    c.execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                              retriever_config, created_at, updated_at)
         VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
        params![profile, pack, now],
    )
    .expect("profile");
    c.execute(
        "INSERT INTO board (id, profile_id, title, doctrine_pack_id, default_depth, created_at, updated_at)
         VALUES (?1, ?2, 'Capital', ?3, 'deep', ?4, ?4)",
        params![board, profile, pack, now],
    )
    .expect("board");
    c.execute(
        "INSERT INTO card (id, board_id, kind, question, depth, status, created_at, updated_at)
         VALUES (?1, ?2, 'root', 'What changed in CAR3?', 'deep', 'queued', ?3, ?3)",
        params![card, board, now],
    )
    .expect("card");
    c.execute(
        "INSERT INTO run (id, board_id, card_id, kind, depth, model_policy_snapshot,
                          doctrine_pack_version, status, started_at)
         VALUES (?1, ?2, ?3, 'card', 'deep', '{}', '1.0.0', 'running', ?4)",
        params![run, board, card, now],
    )
    .expect("run");

    Fixture {
        store,
        registry: Registry::load().expect("registry"),
        ids: Ids {
            board_id: board,
            card_id: card,
            run_id: run,
        },
        _root: TempRoot(root),
    }
}

fn packet(ids: &Ids) -> Value {
    json!({
        "schema_version": "1.0",
        "run_id": ulid_like(&ids.run_id),
        "card_id": ulid_like(&ids.card_id),
        "request": { "text": "What changed in CAR3?", "kind": "root", "depth_override": null },
        "board": {
            "board_id": ulid_like(&ids.board_id),
            "title": "Capital",
            "default_depth": "deep",
            "doctrine_pack": { "code": "general", "version": "1.0.0" }
        },
        "parent": null,
        "profile": { "role": null, "default_depth": "deep", "model_policy": {} },
        "doctrine": { "audiences": [], "domains": ["capital"], "sensitivity_rules": [] },
        "recent": [],
        "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
    })
}

/// The store hands out real ULIDs, and the schema requires the ULID alphabet.
fn ulid_like(id: &str) -> String {
    id.to_string()
}

fn router_output(run_id: &str) -> Value {
    json!({
        "schema_version": "1.0",
        "agent_id": "router",
        "run_id": run_id,
        "classification": {
            "question_type": "regulatory", "regulatory_stakes": true, "domain": "capital",
            "audience_id": null, "language": "en",
            "needs_current_information": true, "needs_internal_documents": false,
            "needs_structured_data": false, "entities": ["CAR3"], "is_follow_up_of_context": false
        },
        "depth": {
            "chosen": "deep", "recommended": "deep",
            "reason": "Board default deep; regulatory domain hint agrees.",
            "overridden_by_user": false
        },
        "plan_required": true,
        "visual_hint": "table",
        "model_resolution": { "route": "small", "synthesize": "frontier", "verify": "medium" },
        "early_flags": [],
        "context_notes": { "parent_is_stale": false, "parent_stale_reason": null, "repetition_of_recent": null },
        "confidence": 0.75,
        "caveats": []
    })
}

// -------------------------------------------------------------- test agent --

/// Stands in for the Router until M5 builds the real one. It walks doc 03
/// section 6's states and makes one call with the small alias.
struct StubRouter;

#[async_trait]
impl Agent for StubRouter {
    fn id(&self) -> &str {
        "router"
    }
    fn packet_schema(&self) -> &'static str {
        ids::PACKET_ROUTER
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_ROUTER
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::ROUTER
    }
    fn completion_event(&self) -> Option<&'static str> {
        Some("card.routed.v1")
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        ctx.machine
            .advance_to("validating_packet")
            .map_err(machine_failure)?;
        ctx.machine.advance_to("classifying").map_err(machine_failure)?;

        let mut prompt = format!(
            "Classify this request: {}",
            packet["request"]["text"].as_str().unwrap_or_default()
        );
        // Doc 03 section 10: on a retry the agent is told what it got wrong.
        if let Some(notice) = ctx.violation_notice() {
            prompt.push_str("\n\n");
            prompt.push_str(&notice);
        }

        let completion = ctx
            .call(
                &CompletionRequest::new("claude-haiku-4-5", "route")
                    .effort(Effort::Low)
                    .user(prompt)
                    .expecting(ctx.registry.bundled(ids::OUT_ROUTER).map_err(Failure::from)?),
            )
            .await?;

        let parsed = completion.json().map_err(|e| Failure {
            kind: "schema_violation".into(),
            detail: e.to_string(),
            recovery: Recovery::Retried,
            evidence: None,
            recoverable: true,
        })?;

        ctx.machine
            .advance_to("resolving_depth")
            .map_err(machine_failure)?;
        ctx.machine
            .advance_to("resolving_policy")
            .map_err(machine_failure)?;
        ctx.machine.advance_to("screening").map_err(machine_failure)?;
        ctx.machine.advance_to("emitting").map_err(machine_failure)?;
        ctx.machine.advance_to("done").map_err(machine_failure)?;

        Ok(parsed)
    }
}

fn machine_failure(e: tessera_harness::state::MachineError) -> Failure {
    Failure::new("state_machine", e.to_string(), Recovery::Failed)
}

fn cfg<'a>(registry: &'a Registry, provider: &'a MockProvider, ids: &Ids) -> RunAgent<'a> {
    RunAgent {
        registry,
        provider,
        run_id: ids.run_id.clone(),
        card_id: Some(ids.card_id.clone()),
        board_id: Some(ids.board_id.clone()),
        sequence: 1,
        source: Source::Test,
        // The stub names its model directly, so an empty policy exercises the
        // context's fallback rather than hiding it.
        policy: ResolvedPolicy::default(),
    }
}

fn event_types(f: &Fixture) -> Vec<String> {
    f.store
        .events(Some(&f.ids.board_id))
        .expect("events")
        .into_iter()
        .map(|e| e.event_type)
        .collect()
}

// ------------------------------------------------------------------- tests --

#[tokio::test]
async fn a_mock_run_walks_every_state_and_emits_the_expected_events() {
    let mut f = fixture();
    let provider = MockProvider::new().on("route", MockResponse::Json(router_output(&f.ids.run_id)));

    let outcome = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect("the run succeeds");

    // Every state in doc 03 section 6, in order.
    assert_eq!(
        outcome.visited,
        sequences::ROUTER,
        "the machine must walk the whole sequence"
    );
    assert_eq!(outcome.attempts, 1);

    // The events the Router declares in doc 03 section 7.
    assert_eq!(event_types(&f), vec!["model.call.v1", "card.routed.v1"]);

    // The Step carries its packet, its output and its model call (doc 01 6.2).
    let (agent_id, status, has_output, has_call): (String, String, bool, bool) = f
        .store
        .conn()
        .query_row(
            "SELECT agent_id, status, output IS NOT NULL, model_call IS NOT NULL FROM step WHERE id = ?1",
            params![outcome.step_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("step");
    assert_eq!(agent_id, "router");
    assert_eq!(status, "done");
    assert!(has_output && has_call);

    // Test provenance survives, so the eval harness's runs never trip a policy
    // hook (doc 02 section 10.1, doc 10 section 5).
    let ev = &f.store.events(Some(&f.ids.board_id)).expect("events")[0];
    assert_eq!(ev.provenance.source, Source::Test);
    assert!(!ev.provenance.source.fires_policy_hooks());
}

#[tokio::test]
async fn a_schema_violation_is_retried_once_with_the_violation_attached() {
    let mut f = fixture();
    // First response is well formed json of the wrong shape: the exact case the
    // schema guard exists for.
    let provider = MockProvider::new()
        .on(
            "route",
            MockResponse::WrongShape(json!({ "schema_version": "1.0", "agent_id": "router" })),
        )
        .on("route", MockResponse::Json(router_output(&f.ids.run_id)));

    let outcome = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect("the retry succeeds");

    assert_eq!(
        outcome.attempts, 2,
        "exactly one retry, as every agent spec prescribes"
    );
    assert_eq!(provider.calls_for("route"), 2);
    assert!(provider.script_exhausted());

    // The violation is announced before the retry, so board history shows why.
    let types = event_types(&f);
    assert_eq!(types[0], "model.call.v1");
    assert_eq!(types[1], "schema.violation.v1", "the violation is on the record");
    assert!(types.contains(&"card.routed.v1".to_string()));

    let violations = &f
        .store
        .events(Some(&f.ids.board_id))
        .expect("events")
        .into_iter()
        .find(|e| e.event_type == "schema.violation.v1")
        .expect("violation event")
        .payload;
    assert_eq!(violations["agent_id"], "router");
    assert!(
        violations["violations"].as_array().is_some_and(|v| !v.is_empty()),
        "the event must name what was wrong"
    );
}

#[tokio::test]
async fn garbage_from_the_provider_never_becomes_an_answer() {
    // Doc 12 operating principle 5 and BN-013. The mock's default is garbage, so
    // this is what an unscripted stage does too.
    let mut f = fixture();
    let provider = MockProvider::new();

    let failure = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect_err("garbage must not be admitted");

    assert_eq!(failure.kind, "schema_violation");
    assert!(!failure.recoverable);
    assert_eq!(provider.calls_for("route"), 2, "one retry, then stop");

    let stored_output: Option<String> = f
        .store
        .conn()
        .query_row(
            "SELECT output FROM step WHERE run_id = ?1",
            params![f.ids.run_id],
            |r| r.get(0),
        )
        .expect("step");
    assert!(stored_output.is_none(), "a failed step stores no output");

    let status: String = f
        .store
        .conn()
        .query_row(
            "SELECT status FROM step WHERE run_id = ?1",
            params![f.ids.run_id],
            |r| r.get(0),
        )
        .expect("status");
    assert_eq!(status, "failed");
}

#[tokio::test]
async fn a_malformed_packet_fails_before_any_model_call() {
    // Doc 03 section 6: a missing profile or doctrine is a hard failure, and doc
    // 03 section 10 fails the run before any retrieval spends money.
    let mut f = fixture();
    let provider = MockProvider::new().on("route", MockResponse::Json(router_output(&f.ids.run_id)));

    let mut bad = packet(&f.ids);
    bad["board"]["default_depth"] = json!("exhaustive");

    let failure = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        bad,
    )
    .await
    .expect_err("a malformed packet must be refused");

    assert_eq!(failure.kind, "packet_invalid");
    assert_eq!(
        provider.call_count(),
        0,
        "not one token may be spent on a bad packet"
    );
}

#[tokio::test]
async fn a_provider_outage_is_retried_once_then_reported() {
    let mut f = fixture();
    let provider = MockProvider::new()
        .on("route", MockResponse::Fail(MockFailure::Unavailable))
        .on("route", MockResponse::Json(router_output(&f.ids.run_id)));

    let outcome = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect("the retry succeeds");
    assert_eq!(outcome.attempts, 2);

    // And when it never recovers, the run fails rather than inventing an answer.
    let mut g = fixture();
    let dead = MockProvider::new()
        .on("route", MockResponse::Fail(MockFailure::Unavailable))
        .on("route", MockResponse::Fail(MockFailure::Unavailable));
    let failure = run_agent(
        &StubRouter,
        &mut g.store,
        cfg(&g.registry, &dead, &g.ids),
        packet(&g.ids),
    )
    .await
    .expect_err("two outages end the run");
    assert_eq!(failure.kind, "provider_unavailable");
}

#[tokio::test]
async fn an_auth_failure_is_not_retried() {
    // Retrying a rejected key wastes the user's time and tells them nothing.
    let mut f = fixture();
    let provider = MockProvider::new().on("route", MockResponse::Fail(MockFailure::Auth));

    let failure = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect_err("auth failure ends the run");

    assert_eq!(failure.kind, "provider_auth");
    assert_eq!(provider.calls_for("route"), 1, "no retry on a bad key");
}

#[tokio::test]
async fn every_model_call_reaches_the_audit_trail() {
    let mut f = fixture();
    let provider = MockProvider::new().on("route", MockResponse::Json(router_output(&f.ids.run_id)));
    let outcome = run_agent(
        &StubRouter,
        &mut f.store,
        cfg(&f.registry, &provider, &f.ids),
        packet(&f.ids),
    )
    .await
    .expect("run");

    assert_eq!(outcome.model_calls.len(), 1);
    let call = &outcome.model_calls[0];
    assert_eq!(call.stage, "route");
    assert_eq!(call.model, "claude-haiku-4-5");
    assert_eq!(call.prompt_hash.len(), 64);

    let ev = f
        .store
        .events(Some(&f.ids.board_id))
        .expect("events")
        .into_iter()
        .find(|e| e.event_type == "model.call.v1")
        .expect("model.call.v1");
    assert_eq!(ev.payload["stage"], "route");
    assert_eq!(ev.payload["prompt_hash"], call.prompt_hash);
    assert!(ev.payload["input_tokens"].as_u64().is_some());
}

#[tokio::test]
async fn a_crash_mid_run_is_reclaimed_on_restart() {
    // Doc 12 phase 2 acceptance, second half. Doc 10 section 6's liveness floor.
    let root = std::env::temp_dir().join(format!("tessera-m2-crash-{}", new_id()));
    let board = new_id();
    let run = new_id();

    {
        let store = Store::open(&root).expect("store");
        let now = now_iso8601();
        let (profile, pack) = (new_id(), new_id());
        let c = store.conn();
        c.execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                        flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0.0', '[]', '[]', '{}', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .expect("pack");
        c.execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                                  retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .expect("profile");
        c.execute(
            "INSERT INTO board (id, profile_id, title, doctrine_pack_id, default_depth, created_at, updated_at)
             VALUES (?1, ?2, 'B', ?3, 'deep', ?4, ?4)",
            params![board, profile, pack, now],
        )
        .expect("board");
        c.execute(
            "INSERT INTO run (id, board_id, kind, depth, model_policy_snapshot, doctrine_pack_version,
                              status, started_at)
             VALUES (?1, ?2, 'card', 'research', '{}', '1.0.0', 'running', ?3)",
            params![run, board, now],
        )
        .expect("run");

        let ledger = Ledger::new();
        ledger
            .claim(&store, &run, RunKind::Card, Some(&board))
            .expect("claim");
        assert_eq!(ledger.runs_in_flight(), 1);

        // The process dies here: the claim stays and the heartbeat stops. Time
        // then passes before the user reopens the app, which is what makes the
        // claim detectably dead rather than merely quiet.
        // Let the database compute the age, so the test does not depend on how
        // the machine's local clock relates to UTC.
        store
            .conn()
            .execute(
                "UPDATE run SET heartbeat_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 hour')
                 WHERE id = ?1",
                params![run],
            )
            .expect("age the heartbeat");
    }

    let mut store = Store::open(&root).expect("reopen");
    let reclaimed = Ledger::reclaim_on_start(&mut store).expect("reclaim");
    assert_eq!(reclaimed, vec![run.clone()], "the abandoned run must be found");

    let (status, claimed): (String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT status, claimed_by FROM run WHERE id = ?1",
            params![run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("run");
    assert_eq!(
        status, "failed",
        "a half finished run is failed, not silently resumed"
    );
    assert!(claimed.is_none());

    // A fresh worker can now take work again.
    let ledger = Ledger::new();
    assert!(ledger.admit(RunKind::Card, Some(&board)).is_admitted());

    let _ = std::fs::remove_dir_all(&root);
}
