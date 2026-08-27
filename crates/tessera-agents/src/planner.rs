//! The Planner. Doc 04.
//!
//! Turns one routed request into a retrieval plan: sub-questions, each bound to
//! the retrievers that should answer it, with the constraints the Synthesizer
//! must honour. It runs only when the Router set `plan_required`, so never in
//! fast mode.
//!
//! Two properties from the spec shape the code more than the rest:
//!
//! Doc 04 section 8 step 4: retriever assignment is "deterministic rules with
//! model assistance for the query text". The model proposes sub-questions and
//! queries; which retrievers a sub-question gets is computed here, from the
//! routing signals and the doctrine, so a model in a strange mood cannot route
//! a regulatory question away from the regulatory corpus.
//!
//! Doc 04 section 5's harness rules are enforced after the model call rather
//! than trusted: ids not enabled are dropped, doctrine exclusions are merged
//! back in (the Planner may add, never remove), `value_policy` is forced to
//! `cite_only` unless structured is actually assigned, and the budget is
//! rebalanced. The model's plan is a draft; the deterministic pass is the plan.

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::prompts;

pub struct Planner;

const SYSTEM: &str = "\
You plan retrieval for one research question. You do not answer it and you do \
not write anything the reader will see.

Every sub-question must stand on its own. The reader asked it in a \
conversation, so it may say \"that\", \"this\", \"the same figure\", or \"which \
article says so\", and the retriever it goes to sees only the words you write. \
Resolve every such reference against the ancestor questions and answers given \
to you, and name the subject in full. A sub-question that still points at \
something outside itself retrieves nothing.

Take the subject from the ancestors and take what is asked from the request. \
\"Which article says so?\" after an ancestor about a confidence level of 98.4 \
percent becomes \"which article states the 98.4 percent confidence level for \
the internal model\". Carry over only what the request depends on, and add no \
value, figure, or name the ancestors do not contain.

Break the request into sub-questions that partition it without overlap. For a \
deep request return exactly one sub-question that states the request in full. \
For research return two or three that cover: what the rule or definition says, \
what has changed or is current, and what the reader's own position is, as far \
as those apply.

For each sub-question, write one query per suggested retriever, in that \
retriever's own idiom: keyword style for local and web, article style for \
regulatory, a template name with parameters for structured.";

/// Doc 04 section 8 step 6: a sub-question below this many passages is not
/// worth fanning out, so the lowest priority one is dropped instead.
const MIN_PASSAGES_PER_SQ: i64 = 4;

#[async_trait]
impl Agent for Planner {
    fn id(&self) -> &str {
        "planner"
    }
    fn packet_schema(&self) -> &'static str {
        ids::PACKET_PLANNER
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_PLANNER
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::PLANNER
    }
    fn completion_event(&self) -> Option<&'static str> {
        Some("card.planned.v1")
    }

    /// Doc 04 section 7's payload, field for field.
    fn completion_payload(&self, output: &Value) -> Value {
        let retriever_ids: Vec<&str> = output["sub_questions"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|sq| sq["retrievers"].as_array().into_iter().flatten())
            .filter_map(|r| r["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        json!({
            "sub_question_count": output["sub_questions"].as_array().map(Vec::len).unwrap_or(0),
            "retriever_ids": retriever_ids,
            "passages_budget": output["budget"]["passages_total"].clone(),
            "audience_id": output["constraints"]["audience_id"].clone(),
        })
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        step(ctx, "validating")?;

        let enabled: Vec<String> = packet["retrievers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["enabled"].as_bool().unwrap_or(false))
            .filter_map(|r| r["id"].as_str().map(str::to_string))
            .collect();

        // Doc 04 section 10: `no_retriever_enabled` fails with a pointer at the
        // fix, because a plan that can retrieve from nowhere is not a plan.
        //
        // Boards does not count, and neither does the vault. Doc 15 section 2
        // makes a prior card context and never evidence, and doc 16 section 3.3
        // says the same of a page: a profile whose only retrievers are its own
        // memory and its own notes can corroborate itself and learn nothing,
        // which is the exact loop the own_card_sole_support rule exists to
        // block. Both are what a person already had; a retriever is what
        // brings something new.
        if !enabled.iter().any(|id| id != "boards" && id != "vault") {
            return Err(Failure::new(
                "no_retriever_enabled",
                "No retriever is enabled. Enable at least web or local in Profile.",
                Recovery::Failed,
            ));
        }

        // ---------------------------------------------- 8.1 entities -------
        step(ctx, "resolving_entities")?;
        let resolved = resolve_entities(packet);

        // ------------------------------------- 8.2 context freshness gate --
        // Pattern 5. A stale ancestor never silently becomes context: each one
        // earns a re-verify sub-question and a listing the Synthesizer sees.
        let stale = stale_ancestor_citations(packet);

        // ---------------------------------------------- 8.3 decomposition --
        step(ctx, "decomposing")?;
        let depth = packet["routing"]["depth"].as_str().unwrap_or("deep");
        let max_sq = packet["effort_budget"]["max_sub_questions"].as_i64().unwrap_or(1);

        let draft = match decompose(ctx, packet, &resolved, &enabled, &stale).await {
            Ok(d) => d,
            // Doc 04 section 10: the fallback plan is one sub-question equal to
            // the request with every enabled retriever. A weak plan still
            // produces a card; the Verifier catches what the plan missed.
            Err(f) if f.recoverable => {
                ctx.machine.retry().ok();
                fallback_draft(packet)
            }
            Err(f) => return Err(f),
        };

        // ---------------------------------- 8.4 retriever assignment -------
        step(ctx, "assigning_retrievers")?;
        let mut sub_questions = assign_retrievers(&draft, packet, &enabled, max_sq, depth, &stale);
        add_reverification(&mut sub_questions, &stale, &enabled);

        // ---------------------------------------------- 8.5 constraints ----
        step(ctx, "constraining")?;
        let structured_assigned = sub_questions.iter().any(|sq| {
            sq["retrievers"]
                .as_array()
                .is_some_and(|rs| rs.iter().any(|r| r["id"] == "structured"))
        });
        let constraints = constraints(packet, &draft, structured_assigned, &stale);

        // ---------------------------------------------- 8.6 budgeting ------
        step(ctx, "budgeting")?;
        let mut caveats = string_vec(&draft["caveats"]);
        let budget = budget(&mut sub_questions, packet, &mut caveats);

        step(ctx, "emitting")?;
        let confidence = confidence(&resolved, &sub_questions, &stale);
        if confidence < 0.5 {
            // Doc 04 section 9: below 0.5 the Synthesizer states scope limits.
            caveats.push("The plan is uncertain; the answer should state its scope limits.".into());
        }

        step(ctx, "done")?;
        Ok(json!({
            "schema_version": "1.0",
            "agent_id": "planner",
            "run_id": ctx.run_id,
            "sub_questions": sub_questions,
            "constraints": constraints,
            "resolved_entities": resolved,
            "budget": budget,
            "confidence": confidence,
            "caveats": caveats,
        }))
    }
}

fn step(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

// ------------------------------------------------------ entity resolution --

/// Doc 04 section 8 step 1: deterministic first. Literal match of Router
/// entities against Concept terms and aliases. Two concepts matching one
/// literal is `multiple`; no concept is `unknown` and the literal stands.
fn resolve_entities(packet: &Value) -> Vec<Value> {
    let concepts = packet["concepts"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();

    for literal in packet["routing"]["entities"].as_array().into_iter().flatten() {
        let Some(text) = literal.as_str() else { continue };
        let lower = text.to_lowercase();

        let matches: Vec<&Value> = concepts
            .iter()
            .filter(|c| {
                c["term"].as_str().is_some_and(|t| t.to_lowercase() == lower)
                    || c["aliases"].as_array().is_some_and(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .any(|a| a.to_lowercase() == lower)
                    })
            })
            .collect();

        let (concept_id, ambiguity) = match matches.as_slice() {
            [] => (Value::Null, "unknown"),
            [one] => (one["concept_id"].clone(), "none"),
            _ => (Value::Null, "multiple"),
        };
        out.push(json!({ "literal": text, "concept_id": concept_id, "ambiguity": ambiguity }));
    }
    out
}

// ------------------------------------------------------- freshness gate ----

fn stale_ancestor_citations(packet: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for ancestor in packet["context"]["ancestors"].as_array().into_iter().flatten() {
        for citation in ancestor["citations"].as_array().into_iter().flatten() {
            if citation["stale"].as_bool().unwrap_or(false) {
                out.push(json!({
                    "card_id": ancestor["card_id"].clone(),
                    "ordinal": citation["ordinal"].clone(),
                    "source_title": citation["source_title"].clone(),
                }));
            }
        }
    }
    out
}

// -------------------------------------------------------- decomposition ----

/// What the model is asked for: sub-questions with purposes and suggested
/// queries. Which retrievers each one actually gets is decided after, in
/// [`assign_retrievers`], so the schema here is narrower than the output.
fn draft_schema(max_sq: i64) -> Value {
    json!({
        "type": "object",
        "required": ["sub_questions"],
        "additionalProperties": false,
        "properties": {
            "sub_questions": {
                "type": "array",
                "minItems": 1,
                "maxItems": max_sq,
                "items": {
                    "type": "object",
                    "required": ["text", "purpose"],
                    "additionalProperties": false,
                    "properties": {
                        "text": { "type": "string" },
                        "purpose": { "type": "string" },
                        "queries": {
                            "type": "object",
                            "description": "One query per retriever id, in that retriever's idiom.",
                            "additionalProperties": { "type": "string" }
                        },
                        "depends_on_previous": { "type": "boolean" }
                    }
                }
            },
            "answer_scope": { "type": "string" },
            "scope_limits": { "type": "array", "items": { "type": "string" } },
            "caveats": { "type": "array", "items": { "type": "string" } }
        }
    })
}

async fn decompose(
    ctx: &mut AgentContext<'_>,
    packet: &Value,
    resolved: &[Value],
    enabled: &[String],
    stale: &[Value],
) -> Result<Value, Failure> {
    let request = &packet["request"];
    let depth = packet["routing"]["depth"].as_str().unwrap_or("deep");
    let max_sq = packet["effort_budget"]["max_sub_questions"].as_i64().unwrap_or(1);

    let mut prompt = String::new();
    prompt.push_str(&format!(
        "Request: {}\n",
        request["text"].as_str().unwrap_or_default()
    ));
    prompt.push_str(&format!("Depth: {depth}\n"));
    prompt.push_str(&format!(
        "Sub-questions to produce: {}\n",
        if depth == "deep" {
            "exactly 1".to_string()
        } else {
            format!("2 to {max_sq}")
        }
    ));
    prompt.push_str(&format!(
        "Question type: {}\n",
        packet["routing"]["question_type"].as_str().unwrap_or("factual")
    ));
    prompt.push_str(&format!(
        "Domain: {}\n",
        packet["routing"]["domain"].as_str().unwrap_or("unknown")
    ));

    if let Some(anchor) = request["anchor_text"].as_str() {
        prompt.push_str(&format!("Highlighted phrase it came from: {anchor}\n"));
    }
    if let Some(block) = packet["context"]["parent_visual_block"].as_object() {
        prompt.push_str(&format!(
            "It was asked from a diagram block labelled: {}\n",
            block.get("label").and_then(Value::as_str).unwrap_or("")
        ));
    }
    for ancestor in packet["context"]["ancestors"]
        .as_array()
        .into_iter()
        .flatten()
        .take(3)
    {
        prompt.push_str(&format!(
            "Ancestor question: {}\nAncestor answer, opening: {}\n",
            ancestor["question"].as_str().unwrap_or(""),
            ancestor["answer_excerpt"].as_str().unwrap_or("")
        ));
    }

    // The entities as resolved, with definitions where a concept matched, both
    // readings where two did. Doc 04 section 8 step 1: the prompt must pick or
    // present both, never silently choose.
    if !resolved.is_empty() {
        prompt.push_str("Entities:\n");
        let concepts = packet["concepts"].as_array().cloned().unwrap_or_default();
        for entity in resolved {
            let literal = entity["literal"].as_str().unwrap_or("");
            match entity["ambiguity"].as_str().unwrap_or("unknown") {
                "none" => {
                    let def = concepts
                        .iter()
                        .find(|c| c["concept_id"] == entity["concept_id"])
                        .and_then(|c| c["definition"].as_str())
                        .unwrap_or("");
                    prompt.push_str(&format!("- {literal}: {def}\n"));
                }
                "multiple" => {
                    prompt.push_str(&format!(
                        "- {literal} is ambiguous; both meanings apply until the request says otherwise:\n"
                    ));
                    for c in concepts.iter().filter(|c| {
                        c["term"]
                            .as_str()
                            .is_some_and(|t| t.eq_ignore_ascii_case(literal))
                    }) {
                        prompt.push_str(&format!(
                            "    - {}\n",
                            c["definition"].as_str().unwrap_or("no definition")
                        ));
                    }
                }
                _ => prompt.push_str(&format!("- {literal}: no definition on file\n")),
            }
        }
    }

    if !stale.is_empty() {
        prompt.push_str(
            "Some values cited by the ancestor cards are stale. Add a sub-question that \
re-verifies them against the current source.\n",
        );
        for s in stale {
            prompt.push_str(&format!(
                "- {} (ancestor citation {})\n",
                s["source_title"].as_str().unwrap_or("a source"),
                s["ordinal"]
            ));
        }
    }

    prompt.push_str(&format!("Available retrievers: {}\n", enabled.join(", ")));
    if let Some(notice) = ctx.violation_notice() {
        prompt.push('\n');
        prompt.push_str(&notice);
    }

    let schema = draft_schema(max_sq);
    let request = CompletionRequest::new(ctx.model_for("plan"), "plan")
        .system(format!("{SYSTEM}\n\n{}", prompts::json_only(&schema)))
        .user(prompt)
        // Doc 04 section 13: one or two medium calls, 3 to 4 s, packet budget
        // 2,500 tokens.
        .effort(Effort::Medium)
        .max_tokens(packet["effort_budget"]["max_tokens"].as_u64().unwrap_or(2500) as u32)
        .expecting(schema);

    let completion = ctx.call(&request).await?;
    completion.json().map_err(|e| Failure {
        kind: "schema_violation".into(),
        detail: e.to_string(),
        recovery: Recovery::Retried,
        evidence: None,
        recoverable: true,
    })
}

/// Doc 04 section 10's `schema_violation` fallback: one sub-question equal to
/// the request, every enabled retriever assigned by [`assign_retrievers`].
fn fallback_draft(packet: &Value) -> Value {
    json!({
        "sub_questions": [{
            "text": packet["request"]["text"].clone(),
            "purpose": "Answer the request as asked.",
            "queries": {}
        }],
        "answer_scope": "",
        "caveats": ["The decomposition fell back to the request itself."]
    })
}

// -------------------------------------------------- retriever assignment ---

/// Doc 04 section 8 step 4, the deterministic half.
///
/// Regulatory questions always include the regulatory retriever;
/// `needs_internal_documents` adds local; `needs_current_information` adds web;
/// `needs_structured_data` adds structured. Doc 05 section 8.5 adds boards on
/// every sub-question when the profile has memory on, which reaches this code
/// as `boards` being among the enabled retrievers. Only enabled retrievers
/// survive, and a sub-question left with none gets every enabled one rather
/// than none, because a sub-question that can retrieve nothing answers nothing.
fn assign_retrievers(
    draft: &Value,
    packet: &Value,
    enabled: &[String],
    max_sq: i64,
    depth: &str,
    stale: &[Value],
) -> Vec<Value> {
    let routing = &packet["routing"];
    let is_enabled = |id: &str| enabled.iter().any(|e| e == id);

    // Retrieval is not gated on classification (BN-036). Every enabled
    // evidence retriever runs on every sub-question: deciding where to search
    // before any evidence exists is guessing at retrieval's own job, and the
    // Synthesizer weighs what comes back by trust rank, not by who fetched it.
    // The one exception is structured, which is not a search but a query
    // against a table the user registered, so it joins on its signal and it is
    // the only assignment that changes `value_policy`.
    let mut wanted: Vec<&str> = enabled
        .iter()
        .map(String::as_str)
        .filter(|id| *id != "boards" && *id != "structured")
        .collect();
    if routing["needs_structured_data"].as_bool().unwrap_or(false) && is_enabled("structured") {
        wanted.push("structured");
    }
    if is_enabled("boards") {
        // Doc 05 section 8.5: on every sub-question when memory is on.
        wanted.push("boards");
    }

    let per_sq_cap = packet["effort_budget"]["max_passages_total"]
        .as_i64()
        .unwrap_or(40)
        / max_sq.max(1);

    let mut out = Vec::new();
    let sub_questions = draft["sub_questions"].as_array().cloned().unwrap_or_default();
    let take = if depth == "deep" { 1 } else { max_sq as usize };

    for (i, sq) in sub_questions.into_iter().take(take).enumerate() {
        let text = sq["text"].as_str().unwrap_or_default().to_string();
        let queries = sq["queries"].as_object().cloned().unwrap_or_default();

        let ids: Vec<&str> = wanted.clone();

        let retrievers: Vec<Value> = ids
            .iter()
            .map(|id| {
                let query = queries
                    .get(*id)
                    .and_then(Value::as_str)
                    .filter(|q| !q.trim().is_empty())
                    .unwrap_or(&text);
                let mut filters = Map::new();
                // Doc 04 section 8 step 2: a stale re-verification pins the
                // version it is re-verifying against.
                if *id == "regulatory" && !stale.is_empty() {
                    filters.insert("version_ref".into(), Value::Null);
                }
                json!({
                    "id": id,
                    "query": query,
                    "filters": Value::Object(filters),
                    "max_passages": (per_sq_cap / ids.len().max(1) as i64).max(1),
                })
            })
            .collect();

        out.push(json!({
            "sq_id": format!("sq-{}", i + 1),
            "text": text,
            "purpose": sq["purpose"].clone(),
            "retrievers": retrievers,
            "entity_refs": entity_refs_for(&text, packet),
            "depends_on": if sq["depends_on_previous"].as_bool().unwrap_or(false) && i > 0 {
                json!([format!("sq-{}", i)])
            } else {
                json!([])
            },
        }));
    }
    out
}

/// Add the sub-question that re-checks a stale ancestor's values.
///
/// Doc 04 section 8 step 2 gives every stale ancestor citation a sub-question
/// that re-verifies it. The system prompt asks the model for one, and a model
/// that forgets would leave the card standing on a value nobody checked, so the
/// plan carries it whether or not the draft did. This is the freshness gate
/// doing its own work rather than trusting a prompt to have been obeyed.
///
/// Nothing is added when no ancestor is stale, so an ordinary follow-up plans
/// exactly as it did before.
fn add_reverification(sub_questions: &mut Vec<Value>, stale: &[Value], enabled: &[String]) {
    if stale.is_empty() {
        return;
    }
    // A draft that already re-verifies needs no second one.
    if sub_questions.iter().any(|sq| {
        let text = sq["text"].as_str().unwrap_or_default().to_lowercase();
        text.contains("verif") || text.contains("current")
    }) {
        return;
    }

    let titles: Vec<String> = stale
        .iter()
        .filter_map(|c| c["source_title"].as_str())
        .map(str::to_string)
        .collect();
    let subject = match titles.first() {
        Some(title) if titles.len() == 1 => format!("in {title}"),
        Some(title) => format!("in {title} and the other sources the earlier cards cited"),
        None => "in the sources the earlier cards cited".to_string(),
    };
    let text = format!("Check which values are current {subject}.");

    let per_sq_cap = 6i64;
    let retrievers: Vec<Value> = enabled
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "query": text,
                "filters": Value::Object(Map::new()),
                "max_passages": (per_sq_cap / enabled.len().max(1) as i64).max(1),
            })
        })
        .collect();

    sub_questions.push(json!({
        "sq_id": format!("sq-{}", sub_questions.len() + 1),
        "text": text,
        "purpose": "re-verify a value an earlier card cited",
        "retrievers": retrievers,
        "entity_refs": [],
        "depends_on": [],
    }));
}

/// The concept ids of resolved entities the sub-question mentions, and the
/// literal itself where nothing matched.
fn entity_refs_for(text: &str, packet: &Value) -> Vec<String> {
    let lower = text.to_lowercase();
    packet["routing"]["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|e| lower.contains(&e.to_lowercase()))
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------ constraints --

fn constraints(packet: &Value, draft: &Value, structured_assigned: bool, stale: &[Value]) -> Value {
    let mut must_include: Vec<String> = Vec::new();
    if let Some(anchor) = packet["request"]["anchor_text"].as_str() {
        must_include.push(anchor.to_string());
    }
    if let Some(block) = packet["context"]["parent_visual_block"].as_object()
        && let Some(label) = block.get("label").and_then(Value::as_str)
    {
        must_include.push(label.to_string());
    }

    // Doctrine exclusions are the floor. The model's own scope limits go on
    // top; nothing the doctrine excluded can be argued back in. Doc 04
    // section 5's harness rule, enforced by construction.
    let mut must_exclude = string_vec(&packet["doctrine"]["must_exclude"]);
    for limit in draft["scope_limits"].as_array().into_iter().flatten() {
        if let Some(s) = limit.as_str()
            && !must_exclude.iter().any(|e| e == s)
        {
            must_exclude.push(s.to_string());
        }
    }

    let answer_scope = draft["answer_scope"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "What the sources say about: {}",
                packet["request"]["text"].as_str().unwrap_or_default()
            )
        });

    json!({
        "must_include": must_include,
        "must_exclude": must_exclude,
        "answer_scope": answer_scope,
        "audience_id": packet["routing"]["audience_id"].clone(),
        // Doc 04 section 5: cite_only unless structured is assigned. This is
        // the rule that keeps a computed number out of a card unless it came
        // from a structured query the Verifier can check.
        "value_policy": if structured_assigned { "cite_or_query" } else { "cite_only" },
        "stale_ancestor_citations": stale
            .iter()
            .map(|s| json!({ "card_id": s["card_id"].clone(), "ordinal": s["ordinal"].clone() }))
            .collect::<Vec<_>>(),
    })
}

// --------------------------------------------------------------- budgeting --

/// Doc 04 section 8 step 6. The cap is `max_passages_total`; below four
/// passages per sub-question the lowest priority one is dropped, with a caveat.
/// Priority is packet order, because the decomposition prompt asks for the
/// partition in importance order.
fn budget(sub_questions: &mut Vec<Value>, packet: &Value, caveats: &mut Vec<String>) -> Value {
    let cap = packet["effort_budget"]["max_passages_total"]
        .as_i64()
        .unwrap_or(40);

    let total = |sqs: &[Value]| -> i64 {
        sqs.iter()
            .flat_map(|sq| sq["retrievers"].as_array().into_iter().flatten())
            .filter_map(|r| r["max_passages"].as_i64())
            .sum()
    };

    while sub_questions.len() > 1 && (cap / sub_questions.len() as i64) < MIN_PASSAGES_PER_SQ {
        let dropped = sub_questions.pop();
        if let Some(sq) = dropped {
            caveats.push(format!(
                "Dropped a sub-question to stay within the retrieval budget: {}",
                sq["text"].as_str().unwrap_or("")
            ));
        }
    }

    let passages_total = total(sub_questions).min(cap);
    json!({
        "passages_total": passages_total,
        // Doc 10 section 14's rough shape: a passage is a few hundred tokens by
        // the time it is fenced and attributed.
        "estimated_tokens": passages_total * 300,
    })
}

// -------------------------------------------------------------- confidence --

/// Doc 04 section 9, deterministic signals only, same rule as the Router:
/// confidence is never self reported by the model.
///
/// The self consistency signal (+0.3 in the spec) needs a second cheap call and
/// is not implemented in this build; the score simply cannot reach it, which
/// biases plans toward stating their scope limits. Recorded in the caveat rule
/// at 0.5 rather than hidden.
fn confidence(resolved: &[Value], sub_questions: &[Value], stale: &[Value]) -> f64 {
    let mut score: f64 = 0.0;
    if !resolved.is_empty() && resolved.iter().all(|e| e["ambiguity"] == "none") {
        score += 0.3;
    }
    let well_sourced = sub_questions.iter().all(|sq| {
        sq["retrievers"]
            .as_array()
            .is_some_and(|rs| rs.len() >= 2 || rs.iter().any(|r| r["id"] == "regulatory"))
    });
    if !sub_questions.is_empty() && well_sourced {
        score += 0.2;
    }
    if stale.is_empty() {
        score += 0.2;
    }
    score.min(1.0)
}

fn string_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet() -> Value {
        json!({
            "schema_version": "1.0",
            "run_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "card_id": "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "request": { "text": "How does CAR3 change trading book treatment?", "kind": "root", "anchor_text": null, "anchor_block_ref": null },
            "routing": {
                "question_type": "regulatory", "domain": "capital", "audience_id": null,
                "entities": ["CAR3", "trading book"],
                "needs_current_information": true, "needs_internal_documents": true,
                "needs_structured_data": false, "depth": "research",
                "router_confidence": 0.8, "early_flags": []
            },
            "context": { "board_seed": null, "board_context": null, "ancestors": [], "parent_visual_block": null },
            "concepts": [
                { "concept_id": "01ARZ3NDEKTSV4RRFFQ69G5FA1", "term": "trading book", "definition": "Positions held for trading.", "aliases": [] }
            ],
            "retrievers": [
                { "id": "regulatory", "enabled": true, "config_summary": "" },
                { "id": "local", "enabled": true, "config_summary": "" },
                { "id": "web", "enabled": true, "config_summary": "" },
                { "id": "structured", "enabled": false, "config_summary": "" }
            ],
            "doctrine": { "must_exclude": ["Sensitive"], "domain_vocabulary": [], "freshness_classes": {} },
            "effort_budget": { "max_tokens": 2500, "max_sub_questions": 3, "max_passages_total": 40 }
        })
    }

    #[test]
    fn entities_resolve_deterministically() {
        let resolved = resolve_entities(&packet());
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0]["literal"], "CAR3");
        assert_eq!(resolved[0]["ambiguity"], "unknown");
        assert_eq!(resolved[1]["literal"], "trading book");
        assert_eq!(resolved[1]["ambiguity"], "none");
        assert_eq!(resolved[1]["concept_id"], "01ARZ3NDEKTSV4RRFFQ69G5FA1");
    }

    #[test]
    fn two_concepts_with_one_term_are_marked_multiple() {
        let mut p = packet();
        p["concepts"].as_array_mut().unwrap().push(json!({
            "concept_id": "01ARZ3NDEKTSV4RRFFQ69G5FA2",
            "term": "trading book", "definition": "A ledger someone trades from.", "aliases": []
        }));
        let resolved = resolve_entities(&p);
        assert_eq!(resolved[1]["ambiguity"], "multiple");
        assert!(resolved[1]["concept_id"].is_null(), "neither is silently chosen");
    }

    #[test]
    fn a_disabled_retriever_is_never_assigned() {
        let p = packet();
        let draft = fallback_draft(&p);
        let sqs = assign_retrievers(
            &draft,
            &p,
            &["regulatory".into(), "local".into(), "web".into()],
            3,
            "research",
            &[],
        );
        for sq in &sqs {
            for r in sq["retrievers"].as_array().unwrap() {
                assert_ne!(r["id"], "structured", "structured is disabled in the packet");
            }
        }
    }

    #[test]
    fn a_regulatory_domain_always_gets_the_regulatory_retriever() {
        let p = packet();
        let draft = fallback_draft(&p);
        let sqs = assign_retrievers(
            &draft,
            &p,
            &["regulatory".into(), "web".into()],
            3,
            "research",
            &[],
        );
        for sq in &sqs {
            assert!(
                sq["retrievers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["id"] == "regulatory"),
                "doc 04 section 8 step 4"
            );
        }
    }

    #[test]
    fn deep_yields_exactly_one_sub_question_whatever_the_model_returned() {
        let p = packet();
        let draft = json!({
            "sub_questions": [
                { "text": "one", "purpose": "p", "queries": {} },
                { "text": "two", "purpose": "p", "queries": {} }
            ]
        });
        let sqs = assign_retrievers(&draft, &p, &["web".into()], 1, "deep", &[]);
        assert_eq!(sqs.len(), 1, "doc 04 section 8 step 3");
    }

    #[test]
    fn boards_joins_every_sub_question_when_enabled() {
        // Doc 05 v0.2 section 8.5: the Planner adds this retriever to every
        // sub-question when the profile has memory on.
        let p = packet();
        let draft = fallback_draft(&p);
        let enabled = vec!["regulatory".to_string(), "boards".to_string()];
        let sqs = assign_retrievers(&draft, &p, &enabled, 3, "research", &[]);
        for sq in &sqs {
            assert!(
                sq["retrievers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["id"] == "boards"),
                "memory is on, so the boards retriever joins"
            );
        }
    }

    #[test]
    fn value_policy_is_cite_only_unless_structured_is_assigned() {
        let p = packet();
        let draft = fallback_draft(&p);
        let c = constraints(&p, &draft, false, &[]);
        assert_eq!(c["value_policy"], "cite_only");
        let c = constraints(&p, &draft, true, &[]);
        assert_eq!(c["value_policy"], "cite_or_query");
    }

    #[test]
    fn doctrine_exclusions_survive_whatever_the_model_says() {
        let p = packet();
        // A draft that tries to narrow the exclusions to nothing.
        let draft =
            json!({ "sub_questions": [], "scope_limits": ["only the trading book"], "answer_scope": "x" });
        let c = constraints(&p, &draft, false, &[]);
        let excludes = string_vec(&c["must_exclude"]);
        assert!(
            excludes.contains(&"Sensitive".to_string()),
            "the Planner may add, never remove"
        );
        assert!(excludes.contains(&"only the trading book".to_string()));
    }

    #[test]
    fn a_stale_ancestor_is_listed_for_the_synthesizer() {
        let mut p = packet();
        p["context"]["ancestors"] = json!([{
            "card_id": "01ARZ3NDEKTSV4RRFFQ69G5FA9",
            "question": "What was the buffer?",
            "answer_excerpt": "The buffer is 2.2 %.",
            "citations": [
                { "ordinal": 1, "source_title": "CAR3 v1", "source_class": "regulatory", "stale": true },
                { "ordinal": 2, "source_title": "A memo", "source_class": "local_document", "stale": false }
            ]
        }]);
        let stale = stale_ancestor_citations(&p);
        assert_eq!(stale.len(), 1, "only the stale citation is listed");
        assert_eq!(stale[0]["ordinal"], 1);

        let c = constraints(&p, &fallback_draft(&p), false, &stale);
        assert_eq!(c["stale_ancestor_citations"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_budget_drops_the_last_sub_question_when_the_cap_is_tight() {
        let mut p = packet();
        p["effort_budget"]["max_passages_total"] = json!(10);
        let draft = json!({ "sub_questions": [
            { "text": "one", "purpose": "p", "queries": {} },
            { "text": "two", "purpose": "p", "queries": {} },
            { "text": "three", "purpose": "p", "queries": {} }
        ]});
        let mut sqs = assign_retrievers(&draft, &p, &["web".into()], 3, "research", &[]);
        let mut caveats = Vec::new();
        let b = budget(&mut sqs, &p, &mut caveats);
        assert_eq!(
            sqs.len(),
            2,
            "10 passages cannot feed three sub-questions four each"
        );
        assert_eq!(caveats.len(), 1);
        assert!(b["passages_total"].as_i64().unwrap() <= 10);
    }

    #[test]
    fn confidence_is_deterministic_and_never_reaches_one_without_the_second_call() {
        let resolved = vec![json!({ "literal": "x", "ambiguity": "none", "concept_id": "c" })];
        let sqs = vec![json!({ "retrievers": [{ "id": "regulatory" }] })];
        let c = confidence(&resolved, &sqs, &[]);
        assert!(
            (c - 0.7).abs() < 1e-9,
            "0.3 entities + 0.2 retrievers + 0.2 no stale = {c}"
        );
    }
}
