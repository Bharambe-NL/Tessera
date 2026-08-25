#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! M1 acceptance (doc 12 phase 1): "schema tests pass."
//!
//! Two kinds of test here. The first checks the registry itself loads and that
//! every boundary named in `ids::ALL` is guarded. The second is the one that
//! earns its keep: for each schema, a valid instance passes *and* an instance
//! that breaks a specific rule from the spec is rejected. A schema that accepts
//! everything also compiles.

use serde_json::json;
use tessera_schema::{Registry, ids};

fn registry() -> Registry {
    Registry::load().expect("every embedded schema must compile")
}

const RUN: &str = "01JAV9YQ4M8T7R2K5N6P3W1XZQ";
const CARD: &str = "01JAV9YQ4M8T7R2K5N6P3W1XZR";

#[test]
fn every_embedded_schema_compiles() {
    let r = registry();
    assert!(r.ids().count() >= ids::ALL.len());
}

#[test]
fn every_declared_boundary_is_guarded() {
    // A renamed or deleted schema file must not quietly leave a boundary open.
    let r = registry();
    for id in ids::ALL {
        assert!(r.contains(id), "no schema registered under `{id}`");
    }
}

#[test]
fn an_unknown_schema_id_is_an_error_not_a_pass() {
    let r = registry();
    assert!(r.validate("tessera:output/nonexistent.v1", &json!({})).is_err());
}

// ------------------------------------------------------------------ router --

fn router_output() -> serde_json::Value {
    json!({
        "schema_version": "1.0",
        "agent_id": "router",
        "run_id": RUN,
        "classification": {
            "question_type": "regulatory",
            "regulatory_stakes": true,
            "domain": "capital",
            "audience_id": null,
            "language": "en",
            "needs_current_information": true,
            "needs_internal_documents": true,
            "needs_structured_data": false,
            "entities": ["CAR3", "trading book"],
            "is_follow_up_of_context": false
        },
        "depth": {
            "chosen": "deep",
            "recommended": "research",
            "reason": "Regulatory and quantitative with an internal data need; doctrine hint deep.",
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

#[test]
fn router_output_round_trips() {
    registry()
        .validate(ids::OUT_ROUTER, &router_output())
        .expect("valid router output");
}

#[test]
fn router_depth_must_be_one_of_three() {
    let r = registry();
    let mut bad = router_output();
    bad["depth"]["chosen"] = json!("exhaustive");
    let violations = r.violations(ids::OUT_ROUTER, &bad).expect("violations");
    assert!(!violations.is_empty(), "an invented depth must be rejected");
    assert!(violations.iter().any(|v| v.instance_path.contains("depth")));
}

#[test]
fn router_reason_may_not_be_empty() {
    // Doc 03 section 8.2: the reason string names the step that decided. An
    // empty one makes the depth badge's hover useless.
    let r = registry();
    let mut bad = router_output();
    bad["depth"]["reason"] = json!("");
    assert!(
        !r.violations(ids::OUT_ROUTER, &bad)
            .expect("violations")
            .is_empty()
    );
}

#[test]
fn router_run_id_must_be_a_ulid() {
    let r = registry();
    let mut bad = router_output();
    bad["run_id"] = json!("run-7");
    assert!(
        !r.violations(ids::OUT_ROUTER, &bad)
            .expect("violations")
            .is_empty()
    );
}

// ------------------------------------------------------------------ visual --

fn tree_visual(depth_below_root: usize) -> serde_json::Value {
    // Build a chain root -> child -> ... to the requested depth.
    let mut node = json!({ "label": "leaf", "citation_ordinals": [1] });
    for i in (0..depth_below_root).rev() {
        node = json!({ "label": format!("level {i}"), "children": [node] });
    }
    json!({
        "type": "tree",
        "title": "What changed",
        "payload": { "root": node },
        "block_index": [{ "ref": "/root", "label": "root", "citation_ordinals": [1] }]
    })
}

#[test]
fn a_tree_three_levels_below_root_is_allowed() {
    // Doc 01 section 4.3.1: depth at most 3 below root.
    registry()
        .validate(ids::VISUAL, &tree_visual(3))
        .expect("three levels is the limit");
}

#[test]
fn a_tree_four_levels_below_root_is_rejected() {
    let r = registry();
    let violations = r.violations(ids::VISUAL, &tree_visual(4)).expect("violations");
    assert!(
        !violations.is_empty(),
        "doc 01 section 4.3.1 caps tree depth at 3 below root"
    );
}

#[test]
fn a_node_may_not_have_seven_children() {
    // Doc 01 section 4.3.1: at most 6 children per node.
    let r = registry();
    let children: Vec<_> = (0..7).map(|i| json!({ "label": format!("c{i}") })).collect();
    let v = json!({
        "type": "tree",
        "title": "Too wide",
        "payload": { "root": { "label": "root", "children": children } },
        "block_index": []
    });
    assert!(!r.violations(ids::VISUAL, &v).expect("violations").is_empty());
}

#[test]
fn a_block_ref_must_be_a_json_pointer() {
    // Doc 01 section 4.3: ref is a JSON pointer into payload, which is what lets
    // "Investigate this further" carry an exact reference.
    let r = registry();
    let v = json!({
        "type": "steps",
        "title": "Process",
        "payload": { "steps": [{ "label": "One" }] },
        "block_index": [{ "ref": "steps.0", "label": "One", "citation_ordinals": [] }]
    });
    let violations = r.violations(ids::VISUAL, &v).expect("violations");
    assert!(!violations.is_empty(), "a dotted path is not a JSON pointer");
}

#[test]
fn a_figure_payload_must_actually_be_an_svg() {
    let r = registry();
    let v = json!({
        "type": "figure",
        "title": "Chair",
        "payload": { "svg": "<script>alert(1)</script>", "caption": "" },
        "block_index": []
    });
    assert!(!r.violations(ids::VISUAL, &v).expect("violations").is_empty());
}

#[test]
fn a_chart_point_without_a_citation_is_rejected() {
    // Doc 01 section 9: every point cites a passage or a structured query step.
    // The Visualizer may never invent a point. The stub enforces it now so v1.1
    // does not have to redesign the binding.
    let r = registry();
    let v = json!({
        "type": "chart",
        "title": "Exposure",
        "payload": { "kind": "bar", "series": [{ "name": "s", "points": [{ "x": 1, "y": 2 }] }] },
        "block_index": []
    });
    assert!(!r.violations(ids::VISUAL, &v).expect("violations").is_empty());
}

// ------------------------------------------------------------- synthesizer --

fn synth_output() -> serde_json::Value {
    json!({
        "schema_version": "1.0",
        "agent_id": "synthesizer",
        "run_id": RUN,
        "answer": "The buffer rose to 2.5 percent of risk weighted assets [1].",
        "findings": [{ "text": "The buffer applies from T3 [1].", "citations": [1] }],
        "citations": [{
            "n": 1,
            "passage_id": "01JAV9YQ4M8T7R2K5N6P3W1XZS",
            "claim_span": { "start": 0, "end": 58 },
            "binding": "answer"
        }],
        "conflicts": [],
        "scope_statement": null,
        "unsupported_statements": [],
        "audience_applied": null,
        "advice_handling": "none",
        "structured_summary": {
            "entities": ["capital buffer"],
            "values": [{ "label": "buffer", "value": "2.5", "unit": "%", "citation": 1 }]
        },
        "confidence": 0.8,
        "caveats": []
    })
}

#[test]
fn synthesizer_output_round_trips() {
    registry()
        .validate(ids::OUT_SYNTHESIZER, &synth_output())
        .expect("valid");
}

#[test]
fn a_citation_binding_of_block_belongs_to_the_visualizer_not_the_synthesizer() {
    // Doc 06 section A5 allows answer and finding only. Block bindings are the
    // Visualizer's, through block_index.
    let r = registry();
    let mut bad = synth_output();
    bad["citations"][0]["binding"] = json!("block");
    assert!(
        !r.violations(ids::OUT_SYNTHESIZER, &bad)
            .expect("violations")
            .is_empty()
    );
}

#[test]
fn an_unsupported_statement_needs_a_reason_from_the_fixed_set() {
    let r = registry();
    let mut bad = synth_output();
    bad["unsupported_statements"] = json!([{ "span": { "start": 0, "end": 5 }, "reason": "vibes" }]);
    assert!(
        !r.violations(ids::OUT_SYNTHESIZER, &bad)
            .expect("violations")
            .is_empty()
    );
}

// ---------------------------------------------------------------- verifier --

fn verifier_output() -> serde_json::Value {
    json!({
        "schema_version": "1.0",
        "agent_id": "verifier",
        "run_id": RUN,
        "citation_verdicts": [{ "n": 1, "verdict": "supported", "reason": "The passage states the value." }],
        "flags": [],
        "block_actions": [{ "ref": "/rows/2/1", "action": "hide", "flag_index": 0 }],
        "card_confidence": 0.82,
        "card_status": "done",
        "checks_run": [
            { "rule_id": "numeric_without_citation", "outcome": "pass", "detector": "deterministic:numeric_without_citation", "ms": 2 }
        ],
        "caveats": []
    })
}

#[test]
fn verifier_output_round_trips() {
    registry()
        .validate(ids::OUT_VERIFIER, &verifier_output())
        .expect("valid");
}

#[test]
fn a_skipped_check_must_say_why() {
    // Doc 07 section B5: checks_run lists every doctrine rule, and a skipped rule
    // must say why. A silent skip is how a disabled check hides.
    let r = registry();
    let mut bad = verifier_output();
    bad["checks_run"] =
        json!([{ "rule_id": "advice_language", "outcome": "skipped", "detector": "model:advice" }]);
    let violations = r.violations(ids::OUT_VERIFIER, &bad).expect("violations");
    assert!(
        !violations.is_empty(),
        "a skipped check without a reason must be rejected"
    );

    let mut good = bad.clone();
    good["checks_run"][0]["reason"] = json!("The pack rule names no detector.");
    r.validate(ids::OUT_VERIFIER, &good)
        .expect("with a reason it passes");
}

#[test]
fn a_verdict_outside_the_four_is_rejected() {
    let r = registry();
    let mut bad = verifier_output();
    bad["citation_verdicts"][0]["verdict"] = json!("probably fine");
    assert!(
        !r.violations(ids::OUT_VERIFIER, &bad)
            .expect("violations")
            .is_empty()
    );
}

#[test]
fn card_status_may_only_be_done_or_flagged() {
    // Doc 07 section B5. The Verifier never fails a card; the harness does.
    let r = registry();
    let mut bad = verifier_output();
    bad["card_status"] = json!("failed");
    assert!(
        !r.violations(ids::OUT_VERIFIER, &bad)
            .expect("violations")
            .is_empty()
    );
}

// --------------------------------------------------------------- retriever --

#[test]
fn a_passage_over_the_cap_is_rejected() {
    // Doc 05 section 5: passage text is capped at 1,200 characters and longer
    // spans are split. The cap is a schema rule so no retriever can forget it.
    let r = registry();
    let long = "x".repeat(1201);
    let out = json!({
        "schema_version": "1.0",
        "agent_id": "retriever.web",
        "run_id": RUN,
        "passages": [{
            "passage_id": "01JAV9YQ4M8T7R2K5N6P3W1XZT",
            "source_id": "01JAV9YQ4M8T7R2K5N6P3W1XZU",
            "text": long,
            "source": {
                "class": "web", "title": "A page", "locator": "https://example.invalid",
                "trust_rank": 3, "freshness_class": "web_general"
            }
        }],
        "coverage": "full",
        "confidence": 0.7
    });
    assert!(
        !r.violations(ids::OUT_RETRIEVER, &out)
            .expect("violations")
            .is_empty()
    );
}

// ------------------------------------------------------------------- tutor --

#[test]
fn the_tutor_may_not_cite() {
    // Doc 14 section 3.5: no reply may contain a citation marker. The tutor
    // cites nothing; cards do.
    let r = registry();
    let base = json!({ "schema_version": "1.0", "agent_id": "tutor", "run_id": RUN, "stage": "checking" });

    let mut with_marker = base.clone();
    with_marker["reply"] = json!("The buffer is 2.5 percent [1].");
    assert!(
        !r.violations(ids::OUT_TUTOR, &with_marker)
            .expect("violations")
            .is_empty()
    );

    let mut without = base;
    without["reply"] = json!("The buffer is on the card you just opened.");
    r.validate(ids::OUT_TUTOR, &without)
        .expect("a reply with no marker passes");
}

// ------------------------------------------------------------------- event --

#[test]
fn the_event_envelope_round_trips() {
    let r = registry();
    let ev = json!({
        "event_id": "01JAV9YQ4M8T7R2K5N6P3W1XZV",
        "event_type": "card.answered.v1",
        "payload": { "card_id": CARD },
        "provenance": {
            "source": "live",
            "emitter_id": "harness",
            "emitter_type": "harness",
            "run_id": RUN,
            "trust_level": "verified"
        },
        "sequence": { "monotonic_index": 41, "causal_parent_id": null },
        "board_id": null,
        "card_id": CARD,
        "timestamp": "2026-08-25T09:12:00.000Z"
    });
    r.validate(ids::EVENT_ENVELOPE, &ev).expect("valid envelope");
}

#[test]
fn an_unversioned_event_type_is_rejected() {
    // Doc 01 section 6.3: event types are versioned. An unversioned one cannot
    // be migrated later.
    let r = registry();
    let ev = json!({
        "event_id": "01JAV9YQ4M8T7R2K5N6P3W1XZV",
        "event_type": "card.answered",
        "payload": {},
        "provenance": { "source": "live", "emitter_id": "h", "emitter_type": "harness", "trust_level": "verified" },
        "sequence": { "monotonic_index": 1 },
        "timestamp": "2026-08-25T09:12:00.000Z"
    });
    assert!(
        !r.violations(ids::EVENT_ENVELOPE, &ev)
            .expect("violations")
            .is_empty()
    );
}

#[test]
fn an_unknown_provenance_source_is_rejected() {
    let r = registry();
    let ev = json!({
        "event_id": "01JAV9YQ4M8T7R2K5N6P3W1XZV",
        "event_type": "card.answered.v1",
        "payload": {},
        "provenance": { "source": "production", "emitter_id": "h", "emitter_type": "harness", "trust_level": "verified" },
        "sequence": { "monotonic_index": 1 },
        "timestamp": "2026-08-25T09:12:00.000Z"
    });
    assert!(
        !r.violations(ids::EVENT_ENVELOPE, &ev)
            .expect("violations")
            .is_empty()
    );
}

// -------------------------------------------------------------------- pack --

#[test]
fn a_flag_rule_detector_must_name_deterministic_or_model() {
    // Doc 01 section 4.17: detector names a deterministic check or a model
    // prompt id. Doc 07 section B10 skips a rule whose detector is missing and
    // notifies that the pack is malformed, so the shape has to be checkable.
    let r = registry();
    let pack = |detector: &str| {
        json!({
            "code": "finance-eu",
            "version": "1.0.0",
            "audiences": [{ "id": "risk", "name": "Risk" }],
            "source_hierarchy": [{ "class": "regulatory", "trust_rank": 1 }],
            "freshness_classes": { "regulation": { "max_age_days": 365, "on_stale": "flag" } },
            "flag_rules": [{
                "rule_id": "advice_language",
                "severity": "warn",
                "description": "Recommendation phrasing.",
                "detector": detector
            }],
            "retrievers": [{ "id": "regulatory" }],
            "exercise_templates": [{ "id": "finance_basic", "item_kinds": ["recall", "trace"] }]
        })
    };

    r.validate(ids::DOCTRINE_PACK, &pack("deterministic:advice_language"))
        .expect("deterministic");
    r.validate(ids::DOCTRINE_PACK, &pack("model:advice_v1"))
        .expect("model");
    assert!(
        !r.violations(ids::DOCTRINE_PACK, &pack("advice_language"))
            .expect("violations")
            .is_empty(),
        "a detector with no kind prefix must be rejected"
    );
}

#[test]
fn a_pack_version_must_be_semver() {
    // Doc 01 section 4.17: boards pin a version, and doc 10 section 9 says a pack
    // update never rewrites a board's pinned version. That needs an ordering.
    let r = registry();
    let mut pack = json!({
        "code": "general",
        "version": "1",
        "audiences": [],
        "source_hierarchy": [],
        "freshness_classes": {},
        "flag_rules": [],
        "retrievers": [],
        "exercise_templates": []
    });
    assert!(
        !r.violations(ids::DOCTRINE_PACK, &pack)
            .expect("violations")
            .is_empty()
    );
    pack["version"] = json!("1.0.0");
    r.validate(ids::DOCTRINE_PACK, &pack).expect("semver passes");
}

// ------------------------------------------------------------------ bundle --

#[test]
fn a_bundle_manifest_pins_its_format_version() {
    let r = registry();
    let manifest = json!({
        "bundle_id": "b-1",
        "format_version": "1.0",
        "exported_at": "2026-08-25T09:12:00.000Z",
        "exported_by": "Sagar",
        "board_id": "01JAV9YQ4M8T7R2K5N6P3W1XZW",
        "doctrine_pack": { "code": "finance-eu", "version": "1.0.0" },
        "includes": { "cards": true, "visuals": true, "citations": true, "sources": true, "passages": true, "events": true },
        "local_documents": [],
        "blobs": []
    });
    r.validate(ids::BUNDLE_MANIFEST, &manifest)
        .expect("valid manifest");

    let mut future = manifest;
    future["format_version"] = json!("2.0");
    assert!(
        !r.violations(ids::BUNDLE_MANIFEST, &future)
            .expect("violations")
            .is_empty(),
        "a v1 importer must refuse a format it does not know"
    );
}

// ------------------------------------------------------------- diagnostics --

#[test]
fn violations_name_the_offending_path() {
    // Doc 03 section 10 and doc 06 section A10 retry once "with the violation
    // attached", so the message has to be specific enough to put in a prompt.
    let r = registry();
    let mut bad = router_output();
    bad["classification"]["language"] = json!("english");
    let violations = r.violations(ids::OUT_ROUTER, &bad).expect("violations");
    assert!(
        violations.iter().any(|v| v.instance_path.contains("language")),
        "got {violations:?}"
    );
    let rendered = violations[0].to_string();
    assert!(
        rendered.contains('/'),
        "a violation renders with its path: {rendered}"
    );
}
