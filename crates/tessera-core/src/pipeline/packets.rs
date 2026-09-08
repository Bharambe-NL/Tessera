//! The packets the card run hands its agents.
//!
//! Every one of these is a schema the registry validates at the boundary, so a
//! builder here is the one place a field either reaches the model or does not.

use serde_json::{Value, json};
use tessera_harness::Failure;
use tessera_store::{Store, repo};

use super::support::{LEARN, truncate_chars};
use super::{RunContext, Subject};

/// The active mission's `sources_hint`, for a board that is running a lesson.
///
/// Read here rather than carried down from the caller: the Planner packet is
/// built for every card, and a lookup that returns nothing outside a lesson is
/// cheaper to read than a parameter threaded through every path that does not
/// use it.
fn mission_sources(store: &Store, ctx: &RunContext<'_>, board_mode: &str) -> Vec<String> {
    if board_mode != LEARN {
        return Vec::new();
    }
    tessera_store::repo::active_mission(store, &ctx.profile_id).unwrap_or(Value::Null)["sources_hint"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Doc 03 section 4's `parent` block, from the card's own ancestry.
///
/// This was `null` for every card, which made every follow-up a question with
/// no subject. "Which article says so?" retrieves nothing on its own, and
/// nothing is what it retrieved: recall on standalone questions measured 1.000
/// and on follow-ups 0.485, and the whole of that gap was this field.
fn parent_block(ancestors: &[repo::Ancestor]) -> Value {
    let Some(parent) = ancestors.first() else {
        return Value::Null;
    };
    json!({
        "card_id": parent.card_id,
        "question": parent.question,
        // Doc 03 section 6: the prompt gets the first 600 characters of the
        // parent answer, so there is no reason to carry more than that.
        "answer": parent.answer.as_deref().map(|a| truncate_chars(a, 600)).unwrap_or_default(),
        "depth": parent.depth,
        "confidence": parent.confidence,
        "answered_at": parent.answered_at,
        "citation_count": parent.citations.len(),
        "stale_citations": parent.stale_citations()
    })
}

pub(super) fn build_router_packet(
    board: &Value,
    subject: &Subject<'_>,
    depth_override: Option<&str>,
    model_override: Option<&str>,
    ctx: &RunContext<'_>,
) -> Value {
    let card = subject.card;
    json!({
        "schema_version": "1.0",
        "run_id": subject.run_id,
        "card_id": card.id,
        "request": {
            "text": subject.question,
            "kind": card.kind,
            "anchor_text": card.anchor_text,
            "anchor_block_ref": card.anchor_block_ref,
            "depth_override": depth_override,
            // The user's model choice from the chat window names the alias that
            // writes the answer. Doc 01 section 5's per stage shape carries it.
            "model_override": model_override
                .map(|alias| json!({ "stage": "synthesize", "alias": alias }))
                .unwrap_or(Value::Null),
            "audience_override": null
        },
        "board": board,
        "parent": parent_block(subject.ancestors),
        "profile": {
            "role": null,
            "default_depth": board["default_depth"].clone(),
            "model_policy": {}
        },
        "doctrine": {
            "audiences": serde_json::to_value(&ctx.pack.audiences).unwrap_or(json!([])),
            "domains": serde_json::to_value(&ctx.pack.domains).unwrap_or(json!([])),
            "domain_vocabulary": serde_json::to_value(&ctx.pack.domain_vocabulary).unwrap_or(json!({})),
            "sensitivity_rules": serde_json::to_value(&ctx.pack.sensitivity_rules).unwrap_or(json!([])),
            "type_preferences": serde_json::to_value(&ctx.pack.visual_preferences.type_preferences).unwrap_or(json!({}))
        },
        "recent": [],
        "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
    })
}

/// Doc 04 section 4, assembled from what exists at M5.
///
/// `concepts` is empty because the Concept graph has no write path until M9;
/// entity resolution degrades exactly as the spec says it should, to literals
/// marked `unknown`. `retrievers` comes from the pack's list with the pack's
/// own defaults, because per-profile retriever configuration is an M9 Profile
/// surface. Doc 05 v0.2 section 8.5 adds `boards` when the profile has memory
/// on, which is doc 01 section 4.16's default.
/// Doc 04 section 4's ancestor chain, capped at three by the schema.
///
/// The Planner's job, doc 04 section 9, is "carrying the board context (parent
/// answer, seed, highlighted phrase) into each sub-question". It cannot do that
/// from an empty array, which is what it was given.
fn ancestor_blocks(ancestors: &[repo::Ancestor]) -> Value {
    Value::Array(
        ancestors
            .iter()
            .take(3)
            .map(|a| {
                json!({
                    "card_id": a.card_id,
                    "question": a.question,
                    // The schema caps the excerpt at 800 characters. Sending
                    // more would be rejected at the boundary, which is the
                    // schema guard doing its job and not a reason to send it.
                    "answer_excerpt": a.answer.as_deref().map(|t| truncate_chars(t, 800)).unwrap_or_default(),
                    "citations": a.citations.iter().map(|c| json!({
                        "ordinal": c["ordinal"],
                        "source_title": c["source_title"],
                        "source_class": c["source_class"],
                        "stale": c["stale"]
                    })).collect::<Vec<_>>()
                })
            })
            .collect(),
    )
}

pub(super) fn build_planner_packet(
    store: &Store,
    board: &Value,
    routed: &Value,
    subject: &Subject<'_>,
    ctx: &RunContext<'_>,
) -> Result<Value, Failure> {
    let card = subject.card;
    let depth = routed["depth"]["chosen"].as_str().unwrap_or("deep");
    // Doc 04 section 4: 3 sub-questions for research, 1 for deep.
    let max_sub_questions = if depth == "research" { 3 } else { 1 };

    // Capped, because the packet has an effort budget and a profile with a
    // thousand terms would spend it on a glossary.
    // A read the store could not serve is not the Planner's failure to recover
    // from, so it degrades to the empty array the packet carried before M9
    // rather than killing a card over a glossary.
    let concepts = repo::concepts_for_packet(store, &ctx.profile_id, 40).unwrap_or_default();

    // Doc 16 section 3.4: what a notebook question may open is the vault and
    // the profile's own cards, and the Planner has to be able to tell that from
    // a profile that has configured nothing at all.
    let board_mode = board["mode"].as_str().unwrap_or("explore").to_string();

    // Doc 05 section 10 separates a retriever doctrine wants from one the
    // profile has told where to read, and the Planner is told the second.
    //
    // It plans assignments, and an assignment naming a connector the fan-out
    // will skip is a sub-question with no source behind it: the card comes back
    // thin and nothing says why. It also decides doc 04 section 10's
    // `no_retriever_enabled`, whose message reads "Enable at least web or local
    // in Profile", which only means anything if what it read is what Profile
    // controls. BN-140.
    let mut retrievers: Vec<Value> = ctx
        .pack
        .retrievers
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "enabled": r.enabled_by_default && ctx.retrievers.configured(&r.id),
                "config_summary": "",
            })
        })
        .collect();

    let memory_enabled: bool = store
        .conn()
        .query_row(
            "SELECT memory_enabled FROM profile WHERE id = ?1",
            [&ctx.profile_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| v != 0)
        .unwrap_or(true);
    if memory_enabled && !retrievers.iter().any(|r| r["id"] == "boards") {
        retrievers.push(json!({ "id": "boards", "enabled": true, "config_summary": "" }));
    }

    let must_exclude: Vec<String> = ctx
        .pack
        .retrievers
        .iter()
        .flat_map(|r| r.must_exclude.iter().cloned())
        .collect();

    let domain = routed["classification"]["domain"].as_str().unwrap_or("unknown");
    let vocabulary = ctx
        .pack
        .domain_vocabulary
        .get(domain)
        .cloned()
        .unwrap_or_default();

    Ok(json!({
        "schema_version": "1.0",
        "run_id": subject.run_id,
        "card_id": card.id,
        "request": {
            "text": subject.question,
            "kind": card.kind,
            "anchor_text": card.anchor_text,
            "anchor_block_ref": card.anchor_block_ref
        },
        "routing": {
            "question_type": routed["classification"]["question_type"].clone(),
            "domain": domain,
            "audience_id": routed["classification"]["audience_id"].clone(),
            "entities": routed["classification"]["entities"].clone(),
            "needs_current_information": routed["classification"]["needs_current_information"].clone(),
            "needs_internal_documents": routed["classification"]["needs_internal_documents"].clone(),
            "needs_structured_data": routed["classification"]["needs_structured_data"].clone(),
            "regulatory_stakes": routed["classification"]["regulatory_stakes"].clone(),
            "depth": depth,
            "router_confidence": routed["confidence"].clone(),
            "early_flags": routed["early_flags"].clone()
        },
        "context": {
            // Doc 04 section 9 puts the board seed and context in scope for the
            // Planner. They were null while the board carried both, so a board
            // opened with a seed answered as though it had none.
            "board_mode": board_mode,
            "board_seed": board["seed_label"].clone(),
            "board_context": board["context"].clone(),
            "ancestors": ancestor_blocks(subject.ancestors),
            "parent_visual_block": null
        },
        // Doc 04 section 4. Empty until M9, because the Concept graph had no
        // write path and entity resolution degraded to literals marked
        // `unknown` exactly as doc 04 says it should when the graph is empty.
        // It is written now, so the Planner reads what the profile knows.
        "concepts": concepts,
        "retrievers": retrievers,
        "doctrine": {
            "must_exclude": must_exclude,
            // Doc 17 section 5: "the learner's sources hint from a path is
            // passed to the Planner as `must_include` locators". Empty outside
            // a lesson, because a mission is what carries them and a board in
            // any other mode is not planned against one.
            "must_include": mission_sources(store, ctx, &board_mode),
            "domain_vocabulary": vocabulary,
            "freshness_classes": {}
        },
        "effort_budget": {
            "max_tokens": 2500,
            "max_sub_questions": max_sub_questions,
            "max_passages_total": 40
        }
    }))
}

/// The ancestors in the shape doc 06 section A4 declares: what was asked, an
/// excerpt of what was answered, and whether it still stands.
fn synth_ancestors(ancestors: &[repo::Ancestor]) -> Value {
    Value::Array(
        ancestors
            .iter()
            .take(3)
            .map(|a| {
                json!({
                    "question": a.question,
                    "answer_excerpt": a.answer.as_deref()
                        .map(|t| truncate_chars(t, 800))
                        .unwrap_or_default(),
                    "stale": a.stale_citations() > 0,
                })
            })
            .collect(),
    )
}

pub(super) fn build_synth_packet(
    routed: &Value,
    plan: Option<&Value>,
    mode: &str,
    subject: &Subject<'_>,
    passages: &[Value],
    ctx: &RunContext<'_>,
) -> Value {
    json!({
        "schema_version": "1.0",
        "run_id": routed["run_id"].clone(),
        "mode": mode,
        // The card's own kind and anchor, not `root` and null. A branch spawned
        // from a highlighted phrase read as a question typed from nothing, so
        // the prompt's "it came from the highlighted phrase" line could never
        // fire and doc 06 section A4's `request.kind` was always the same word.
        "request": {
            "text": subject.question,
            "kind": subject.card.kind,
            "anchor_text": subject.card.anchor_text,
        },
        // Doc 06 section A4: the Synthesizer reads the plan's constraints, so
        // the answer scope the Verifier checks is the one the Planner declared.
        "plan": plan.cloned().unwrap_or(Value::Null),
        "passages": passages,
        // Doc 06 section A2: "Reads the plan, the passages, the ancestors". The
        // field was hardcoded empty, so the prompt loop that reads it never ran
        // and a follow-up was written as though nothing preceded it. A stale
        // ancestor is marked, so the answer does not lean on a value that has
        // since moved.
        "ancestors": synth_ancestors(subject.ancestors),
        "flags": routed["early_flags"].clone(),
        "audience": Value::Null,
        "writing_rules": serde_json::to_value(&ctx.pack.writing_rules).unwrap_or(json!({})),
        "profile": { "role": null, "context": null },
        "standing_instructions": Value::Null,
        "effort_budget": {
            "max_tokens": 3000,
            "answer_max_words": if mode == "fast" { 140 } else { 180 },
            "findings_max": 5
        }
    })
}
