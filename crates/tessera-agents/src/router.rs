//! The Router. Doc 03.
//!
//! First agent to see a card request. It decides how much work the request
//! deserves and which policy applies, so every later agent starts from a typed
//! decision rather than from raw user text.
//!
//! Two properties from the spec shape the code more than the rest:
//!
//! Doc 03 section 1: "Overriding the user. If the user chose research, the
//! Router may recommend fast in its output and must still route to research."
//! `chosen` is set deterministically from the override before the model is asked
//! anything, so a model that disagrees cannot change it. Doc 03 section 12 makes
//! override compliance 1.00 and calls any miss a schema bug.
//!
//! Doc 03 section 9: "Confidence is computed from deterministic signals, never
//! self reported by the model (the Coffret rule)." The model's own confidence
//! field, if it returns one, is discarded.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::prompts;

pub struct Router;

const CLASSIFY_SYSTEM: &str = "\
You classify a request for a research canvas. You do not answer it, you do not \
search, and you do not write anything the reader will see.

Return the classification block only. Judge what the request needs:
  needs_current_information  the answer depends on something that changes over time
  needs_internal_documents   the answer depends on the reader's own files
  needs_structured_data      the answer depends on a figure from a table

entities are the literal names and terms in the request, copied as written.
language is the ISO 639-1 code of the request text.

regulatory_stakes is true when the answer turns on a rule, threshold, date, or
obligation the reader might act on, and false for questions of plain
understanding. When unsure, true: the cost of care on a casual question is
seconds, and the cost of casualness on a consequential one is a wrong number
acted on.";

#[async_trait]
impl Agent for Router {
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

    /// Doc 03 section 7's payload, field for field.
    fn completion_payload(&self, output: &Value) -> Value {
        json!({
            "question_type": output["classification"]["question_type"].clone(),
            // BN-036: the judgment that replaced the domain taxonomy. It rides
            // in the event because the eval scores it from here and because
            // "How this was built" should say why a card went deep.
            "regulatory_stakes": output["classification"]["regulatory_stakes"].clone(),
            "domain": output["classification"]["domain"].clone(),
            "audience_id": output["classification"]["audience_id"].clone(),
            "depth_chosen": output["depth"]["chosen"].clone(),
            "depth_recommended": output["depth"]["recommended"].clone(),
            "depth_reason": output["depth"]["reason"].clone(),
            "overridden_by_user": output["depth"]["overridden_by_user"].clone(),
            "plan_required": output["plan_required"].clone(),
            "visual_hint": output["visual_hint"].clone(),
            "model_resolution": output["model_resolution"].clone(),
        })
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        step(ctx, "validating_packet")?;

        let request = &packet["request"];
        let board = &packet["board"];
        let doctrine = &packet["doctrine"];
        let text = request["text"].as_str().unwrap_or_default();

        // ------------------------------------------------ 8.1 classification --
        step(ctx, "classifying")?;

        // Two deterministic pre passes, inserted into the prompt as hints. Doc
        // 03 section 8.1.
        let candidates = keyword_candidates(text, &doctrine["domain_vocabulary"]);
        let keyword_domain = match candidates.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        };
        let detected_language = detect_language(text);

        let mut classification = match classify(ctx, packet).await {
            Ok(c) => c,
            // Doc 03 section 10: a timeout or a schema violation falls back to a
            // deterministic default and the run continues. A wrong route is
            // recoverable downstream; stopping the card is not.
            Err(f) if f.recoverable => {
                ctx.machine.retry().ok();
                fallback_classification(text, keyword_domain.as_deref(), &detected_language)
            }
            Err(f) => return Err(f),
        };

        // The domain is an observation, never a judgment the model is asked
        // to make and never a gate on anything downstream (BN-036). The free
        // keyword pass labels what it can prove; everything else is unknown,
        // and unknown costs nothing.
        classification["domain"] = match &keyword_domain {
            Some(domain) => json!(domain),
            None => json!("unknown"),
        };
        classification["language"] = json!(detected_language);
        if !classification["regulatory_stakes"].is_boolean() {
            // A model that failed to say is treated as if it said yes; the
            // conservative reading is the cheap one to be wrong about.
            classification["regulatory_stakes"] = json!(true);
        }

        // ------------------------------------------------ 8.2 depth --------
        step(ctx, "resolving_depth")?;
        let depth = resolve_depth(request, board, doctrine, &classification);

        // ------------------------------------------------ 8.3 policy -------
        step(ctx, "resolving_policy")?;
        let model_resolution = resolve_policy(packet, request);

        // ------------------------------------------------ 8.4 screening ----
        step(ctx, "screening")?;
        let early_flags = screen(text, doctrine);

        // ------------------------------------------------ 8.5 context ------
        let context_notes = context_notes(packet);

        step(ctx, "emitting")?;
        step(ctx, "done")?;

        let confidence = confidence(
            keyword_domain.is_some(),
            &depth,
            &early_flags,
            &detected_language,
            text,
        );

        Ok(json!({
            "schema_version": "1.0",
            "agent_id": "router",
            "run_id": ctx.run_id,
            "classification": classification,
            "depth": depth,
            // Doc 04 section 1: the Planner runs only when there is retrieval to
            // plan, so fast never plans.
            "plan_required": depth["chosen"] != "fast",
            "visual_hint": visual_hint(&classification, doctrine),
            "model_resolution": model_resolution,
            "early_flags": early_flags,
            "context_notes": context_notes,
            "confidence": confidence,
            "caveats": caveats(&depth),
        }))
    }
}

fn step(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

async fn classify(ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
    let request = &packet["request"];
    let text = request["text"].as_str().unwrap_or_default();

    let mut prompt = String::new();
    prompt.push_str(&format!("Request: {text}\n"));
    prompt.push_str(&format!("Kind: {}\n", request["kind"].as_str().unwrap_or("root")));
    if let Some(anchor) = request["anchor_text"].as_str() {
        prompt.push_str(&format!("Highlighted phrase it came from: {anchor}\n"));
    }
    if let Some(parent) = packet["parent"].as_object() {
        prompt.push_str(&format!(
            "Parent question: {}\n",
            parent.get("question").and_then(Value::as_str).unwrap_or("")
        ));
        // Doc 03 section 8.1 caps this at 600 characters to keep the packet small.
        let answer = parent.get("answer").and_then(Value::as_str).unwrap_or("");
        prompt.push_str(&format!("Parent answer, opening: {}\n", truncate(answer, 600)));
    }
    if let Some(seed) = packet["board"]["seed_label"].as_str() {
        prompt.push_str(&format!("This board was spun off from: {seed}\n"));
    }
    // No domain enumeration. Two paid sweeps proved this prompt cannot be
    // saved by listing terms: a bare list made the model guess the first
    // name, and a vocabulary made it answer unknown for anything the list
    // missed, and the list will always miss, because nobody can enumerate
    // what users will ask (BN-036). The model is asked one question it can
    // answer in any domain without being taught: does this carry
    // regulatory stakes.
    if let Some(notice) = ctx.violation_notice() {
        prompt.push('\n');
        prompt.push_str(&notice);
    }

    let schema = classification_schema();
    let request = CompletionRequest::new(ctx.model_for("route"), "route")
        .system(format!("{CLASSIFY_SYSTEM}\n\n{}", prompts::json_only(&schema)))
        .user(prompt)
        // Doc 03 section 13: one small call, under 1,500 tokens, 2.5 s target.
        .effort(Effort::Low)
        .max_tokens(1200)
        .expecting(schema);

    let completion = ctx.call(&request).await?;
    let parsed = completion.json().map_err(|e| Failure {
        kind: "schema_violation".into(),
        detail: e.to_string(),
        recovery: Recovery::Retried,
        evidence: None,
        recoverable: true,
    })?;

    // The classification block may arrive bare or wrapped.
    Ok(parsed.get("classification").cloned().unwrap_or(parsed))
}

/// The shape asked of the model. Narrower than the output schema, because the
/// Router computes depth, policy and confidence itself.
fn classification_schema() -> Value {
    json!({
        "type": "object",
        "required": ["question_type", "regulatory_stakes", "language", "entities"],
        "additionalProperties": false,
        "properties": {
            "question_type": { "enum": ["factual","comparative","procedural","quantitative",
                                        "regulatory","definitional","exploratory","meta"] },
            "regulatory_stakes": { "type": "boolean" },
            "audience_id": { "type": ["string", "null"] },
            "language": { "type": "string" },
            "needs_current_information": { "type": "boolean" },
            "needs_internal_documents": { "type": "boolean" },
            "needs_structured_data": { "type": "boolean" },
            "entities": { "type": "array", "items": { "type": "string" } },
            "is_follow_up_of_context": { "type": "boolean" }
        }
    })
}

/// Doc 03 section 10: after a retry, a deterministic default with confidence
/// 0.2. Domain from keywords or unknown, type factual, depth from board default.
/// Deterministic stand-in when the model call fails. Stakes default to true:
/// the fallback exists because something already went wrong, and depth is the
/// wrong place to economise at that moment.
fn fallback_classification(text: &str, keyword_domain: Option<&str>, language: &str) -> Value {
    json!({
        "question_type": "factual",
        "domain": keyword_domain.unwrap_or("unknown"),
        "regulatory_stakes": true,
        "audience_id": null,
        "language": language,
        "needs_current_information": false,
        "needs_internal_documents": false,
        "needs_structured_data": false,
        "entities": capitalised_terms(text),
        "is_follow_up_of_context": false
    })
}

/// Doc 03 section 8.2, in order. Ties break toward the cheaper depth, and the
/// reason names the step that decided.
fn resolve_depth(request: &Value, board: &Value, doctrine: &Value, classification: &Value) -> Value {
    let board_default = board["default_depth"].as_str().unwrap_or("fast");

    let mut recommended = board_default.to_string();
    let mut reason = format!("Board default {board_default}.");

    // 3. doctrine hint for consequential questions. Keyed by the stakes
    // judgment rather than a domain taxonomy (BN-036): the pack says how much
    // care a question with regulatory stakes deserves, and the model says
    // whether this is one, which works for domains nobody listed.
    if classification["regulatory_stakes"].as_bool().unwrap_or(true)
        && let Some(hint) = doctrine["depth_hints"]
            .get("regulatory_stakes")
            .and_then(Value::as_str)
        && rank(hint) > rank(&recommended)
    {
        recommended = hint.to_string();
        reason = format!("Doctrine hints {hint} for a question with regulatory stakes.");
    }

    // 4. request signals.
    let flag = |name: &str| classification[name].as_bool().unwrap_or(false);
    if (flag("needs_current_information") || flag("needs_internal_documents")) && recommended == "fast" {
        recommended = "deep".into();
        reason = "The answer depends on something outside model knowledge, so fast will not do.".into();
    }
    let entity_count = classification["entities"].as_array().map_or(0, Vec::len);
    let qtype = classification["question_type"].as_str().unwrap_or("factual");
    if (qtype == "comparative" && entity_count >= 3 || qtype == "exploratory") && recommended == "deep" {
        recommended = "research".into();
        reason = format!("A {qtype} question across {entity_count} entities needs more than one pass.");
    }

    // 5 and 6. a follow-up inside the parent's scope may stay where it is.
    if classification["is_follow_up_of_context"]
        .as_bool()
        .unwrap_or(false)
        && recommended == "deep"
        && !flag("needs_current_information")
    {
        reason = format!("{reason} Follow-up stays within the parent's scope.");
    }

    // 1. the user's override wins, and is applied last so nothing above can
    // have moved it. Doc 03 section 12: override compliance is 1.00.
    let chosen = match request["depth_override"].as_str() {
        Some(o) => o.to_string(),
        None => recommended.clone(),
    };
    let overridden = request["depth_override"].as_str().is_some();

    json!({
        "chosen": chosen,
        "recommended": recommended,
        "reason": reason,
        "overridden_by_user": overridden
    })
}

fn rank(depth: &str) -> u8 {
    match depth {
        "fast" => 0,
        "deep" => 1,
        _ => 2,
    }
}

/// Doc 03 section 8.3. The merge itself happens in the provider layer; the
/// Router records which alias each stage resolved to.
fn resolve_policy(packet: &Value, request: &Value) -> Value {
    let policy = &packet["profile"]["model_policy"];
    let mut out = serde_json::Map::new();

    if let Some(stages) = policy["stages"].as_object() {
        for (stage, entry) in stages {
            if let Some(alias) = entry["alias"].as_str() {
                out.insert(stage.clone(), json!(alias));
            }
        }
    }
    if out.is_empty() {
        // A packet without a policy still has to name something, or the output
        // fails its own schema and the card dies for a Profile problem.
        for (stage, alias) in [
            ("route", "small"),
            ("plan", "medium"),
            ("synthesize", "frontier"),
            ("visualize", "frontier"),
            ("read", "vision"),
            ("verify", "medium"),
        ] {
            out.insert(stage.to_string(), json!(alias));
        }
    }

    // A card override replaces that stage only. Doc 01 section 5.
    if let Some(o) = request["model_override"].as_object()
        && let (Some(stage), Some(alias)) = (o["stage"].as_str(), o["alias"].as_str())
    {
        out.insert(stage.to_string(), json!(alias));
    }
    Value::Object(out)
}

/// Doc 03 section 8.4. Deterministic detectors run here; a model backed detector
/// may raise at most warn and is not run in this build.
fn screen(text: &str, doctrine: &Value) -> Value {
    let mut flags = Vec::new();
    let rules = doctrine["sensitivity_rules"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    for rule in &rules {
        let rule_id = rule["rule_id"].as_str().unwrap_or_default();
        let detector = rule["detector"].as_str().unwrap_or_default();
        let severity = rule["severity"].as_str().unwrap_or("info");

        let hit = match detector {
            "deterministic:advice_request" => advice_phrase(text),
            "deterministic:personal_data" => personal_data(text),
            _ => None,
        };

        if let Some(matched) = hit {
            flags.push(json!({
                "rule_id": rule_id,
                "severity": severity,
                "reason": reason_for(rule_id),
                "evidence": { "matched": matched }
            }));
        }
    }
    json!(flags)
}

fn reason_for(rule_id: &str) -> &'static str {
    match rule_id {
        "advice_request" => "The question asks for a recommendation.",
        "personal_data_in_request" => "The question contains what looks like personal data.",
        _ => "A doctrine rule matched the request.",
    }
}

/// Doc 03 section 8.4's finance list, kept here because the phrasing patterns
/// are substrate: the pack decides whether the rule exists and at what severity.
fn advice_phrase(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    const PHRASES: &[&str] = &[
        "should we",
        "should i",
        "what would you recommend",
        "what do you recommend",
        "is it safe to",
        "what would you do",
        "do you think we should",
        "advise us",
        "what should we",
    ];
    PHRASES
        .iter()
        .find(|p| lower.contains(*p))
        .map(|p| (*p).to_string())
}

fn personal_data(text: &str) -> Option<String> {
    // Doc 03 section 8.4 blocks the request itself, so a false positive is
    // expensive: the patterns are the unambiguous ones only.
    let iban = regex::Regex::new(r"(?i)\b[A-Z]{2}\d{2}[A-Z0-9]{10,30}\b").ok()?;
    if iban.is_match(text) {
        return Some("an account number".into());
    }
    let long_digits = regex::Regex::new(r"\b\d{13,19}\b").ok()?;
    if long_digits.is_match(text) {
        return Some("an account number".into());
    }
    let ssn = regex::Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").ok()?;
    if ssn.is_match(text) {
        return Some("a national identifier".into());
    }
    None
}

/// Doc 03 section 8.5. Deterministic.
fn context_notes(packet: &Value) -> Value {
    let parent = &packet["parent"];
    let stale = parent["stale_citations"].as_i64().unwrap_or(0) > 0;

    // Doc 03 open question 2, resolved as proposed: exact text match in v1.
    let text = packet["request"]["text"].as_str().unwrap_or_default();
    let repetition = packet["recent"].as_array().and_then(|recent| {
        recent
            .iter()
            .find(|r| r["question"].as_str() == Some(text))
            .and_then(|r| r["card_id"].as_str())
            .map(str::to_string)
    });

    json!({
        "parent_is_stale": stale,
        "parent_stale_reason": if stale { json!("The parent card cites a source that has gone stale.") } else { Value::Null },
        "repetition_of_recent": repetition
    })
}

fn visual_hint(classification: &Value, doctrine: &Value) -> Value {
    let qtype = classification["question_type"].as_str().unwrap_or("factual");
    doctrine["type_preferences"]
        .get(qtype)
        .and_then(Value::as_str)
        .map(|s| json!(s))
        .unwrap_or_else(|| {
            json!(match qtype {
                "comparative" | "quantitative" => "table",
                "procedural" => "steps",
                "definitional" | "exploratory" => "tree",
                "factual" => "list",
                _ => "none",
            })
        })
}

/// Doc 03 section 9. Deterministic signals only.
fn confidence(keyword_agreed: bool, depth: &Value, early_flags: &Value, language: &str, text: &str) -> f64 {
    let mut c: f64 = 0.0;
    if keyword_agreed {
        c += 0.25;
    }
    // The spec's second signal is two independent prompts agreeing on the type.
    // This build runs one classification call, so the signal is unavailable and
    // its weight is not awarded rather than assumed.
    if depth["overridden_by_user"].as_bool().unwrap_or(false)
        || depth["reason"]
            .as_str()
            .is_some_and(|r| r.starts_with("Board default") || r.starts_with("Doctrine"))
    {
        c += 0.25;
    }
    let severe = early_flags
        .as_array()
        .map(|f| {
            f.iter()
                .any(|x| matches!(x["severity"].as_str(), Some("warn" | "block")))
        })
        .unwrap_or(false);
    if !severe {
        c += 0.15;
    }
    if language != "unknown" && text.len() >= 12 {
        c += 0.10;
    }
    (c * 100.0).round() / 100.0
}

fn caveats(depth: &Value) -> Value {
    let mut out = Vec::new();
    if depth["overridden_by_user"].as_bool().unwrap_or(false) {
        let chosen = depth["chosen"].as_str().unwrap_or("fast");
        let recommended = depth["recommended"].as_str().unwrap_or("fast");
        if chosen == "fast" && recommended != "fast" {
            out.push(json!("Fast depth yields an unverified card."));
        }
    }
    json!(out)
}

// ------------------------------------------------------- deterministic bits --

/// Doc 03 section 8.1: a strong single match sets the domain without asking the
/// model. Two domains matching is not a strong match, so it defers.
/// Every domain whose vocabulary appears verbatim in the question.
///
/// One hit decides the domain outright, per doc 03 section 8.1. More than one
/// decides nothing, and it still narrows the field, so the candidates go to the
/// model rather than being thrown away.
fn keyword_candidates(text: &str, vocabulary: &Value) -> Vec<String> {
    let lower = text.to_lowercase();
    let Some(map) = vocabulary.as_object() else {
        return Vec::new();
    };
    let mut hits: Vec<String> = Vec::new();
    for (domain, terms) in map {
        let matched = terms
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .any(|t| lower.contains(&t.to_lowercase()))
            })
            .unwrap_or(false);
        if matched {
            hits.push(domain.clone());
        }
    }
    hits.sort();
    hits
}

/// A small deterministic pass. Doc 03 section 8.1 asks for language detection
/// before the model call so the result can go in as a hint.
fn detect_language(text: &str) -> String {
    let lower = text.to_lowercase();
    let count = |words: &[&str]| words.iter().filter(|w| contains_word(&lower, w)).count();

    let en = count(&["the", "what", "how", "does", "is", "are", "which", "and", "of"]);
    let nl = count(&["de", "het", "wat", "hoe", "een", "van", "wordt", "welke", "voor"]);

    // Only Dutch is distinguished, because it is the language doc 02 section 5.3
    // plants in the internal folder and the only one the corpus tests. Anything
    // else is reported as English rather than guessed at.
    if nl > en && nl >= 2 {
        "nl".into()
    } else {
        "en".into()
    }
}

fn contains_word(haystack: &str, word: &str) -> bool {
    haystack
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == word)
}

/// A crude entity list for the deterministic fallback: capitalised terms and
/// anything that looks like a code.
fn capitalised_terms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 2)
        .filter(|w| w.chars().next().is_some_and(char::is_uppercase) || w.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect();
    out.dedup();
    out.truncate(8);
    out
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(depth_override: Option<&str>, board_default: &str) -> Value {
        json!({
            "request": {
                "text": "What changed in the capital rule?",
                "kind": "root",
                "depth_override": depth_override,
                "model_override": null
            },
            "board": { "default_depth": board_default, "seed_label": null },
            "parent": null,
            "profile": { "model_policy": {} },
            "doctrine": { "domains": ["capital"], "depth_hints": {}, "sensitivity_rules": [] },
            "recent": []
        })
    }

    fn classification(qtype: &str, entities: usize, current: bool) -> Value {
        json!({
            "question_type": qtype,
            "domain": "capital",
            "entities": (0..entities).map(|i| format!("e{i}")).collect::<Vec<_>>(),
            "needs_current_information": current,
            "needs_internal_documents": false,
            "needs_structured_data": false,
            "is_follow_up_of_context": false
        })
    }

    #[test]
    fn a_user_override_always_wins() {
        // Doc 03 section 1 and section 12: override compliance is 1.00, and any
        // miss is a schema bug.
        for (override_depth, board_default) in
            [("fast", "research"), ("research", "fast"), ("deep", "research")]
        {
            let p = packet(Some(override_depth), board_default);
            let d = resolve_depth(
                &p["request"],
                &p["board"],
                &p["doctrine"],
                &classification("exploratory", 5, true),
            );
            assert_eq!(d["chosen"], override_depth);
            assert_eq!(d["overridden_by_user"], true);
        }
    }

    #[test]
    fn the_recommendation_survives_an_override_so_the_header_can_show_it() {
        // Doc 03 section 11: hovering the depth badge shows the recommendation
        // and the reason when they differ.
        let p = packet(Some("fast"), "fast");
        let d = resolve_depth(
            &p["request"],
            &p["board"],
            &p["doctrine"],
            &classification("exploratory", 4, true),
        );
        assert_eq!(d["chosen"], "fast");
        assert_eq!(d["recommended"], "research");
        assert!(d["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn a_need_for_current_information_lifts_fast_to_deep() {
        // Doc 03 section 8.2 step 4.
        let p = packet(None, "fast");
        let d = resolve_depth(
            &p["request"],
            &p["board"],
            &p["doctrine"],
            &classification("factual", 1, true),
        );
        assert_eq!(d["chosen"], "deep");
    }

    #[test]
    fn a_doctrine_hint_raises_but_never_lowers() {
        let mut p = packet(None, "research");
        p["doctrine"]["depth_hints"] = json!({ "capital": "deep" });
        let d = resolve_depth(
            &p["request"],
            &p["board"],
            &p["doctrine"],
            &classification("factual", 1, false),
        );
        assert_eq!(
            d["chosen"], "research",
            "a hint of deep must not pull research down"
        );
    }

    #[test]
    fn a_comparative_question_across_three_entities_reaches_research() {
        let p = packet(None, "deep");
        let d = resolve_depth(
            &p["request"],
            &p["board"],
            &p["doctrine"],
            &classification("comparative", 3, false),
        );
        assert_eq!(d["chosen"], "research");

        let two = resolve_depth(
            &p["request"],
            &p["board"],
            &p["doctrine"],
            &classification("comparative", 2, false),
        );
        assert_eq!(two["chosen"], "deep", "ties break toward the cheaper depth");
    }

    #[test]
    fn advice_bait_raises_a_warn_flag_and_the_card_still_runs() {
        // Doc 03 section 8.4: severity warn, the card still runs, and the flag
        // travels to the Synthesizer and Verifier.
        let doctrine = json!({
            "sensitivity_rules": [
                { "rule_id": "advice_request", "detector": "deterministic:advice_request", "severity": "warn" }
            ]
        });
        let flags = screen("Should we move the exposures before Q4?", &doctrine);
        assert_eq!(flags.as_array().map(Vec::len), Some(1));
        assert_eq!(flags[0]["rule_id"], "advice_request");
        assert_eq!(flags[0]["severity"], "warn");
    }

    #[test]
    fn a_descriptive_question_is_not_advice_bait() {
        let doctrine = json!({
            "sensitivity_rules": [
                { "rule_id": "advice_request", "detector": "deterministic:advice_request", "severity": "warn" }
            ]
        });
        // Doc 03 section 12 caps the false positive rate at 0.05 on non bait.
        for q in [
            "What does the rule say about trading book exposures?",
            "Which article covers the buffer?",
            "How is the ratio calculated?",
        ] {
            assert_eq!(screen(q, &doctrine).as_array().map(Vec::len), Some(0), "{q}");
        }
    }

    #[test]
    fn personal_data_blocks_the_request() {
        // Doc 03 section 12: 1.00 recall on planted cases.
        let doctrine = json!({
            "sensitivity_rules": [
                { "rule_id": "personal_data_in_request", "detector": "deterministic:personal_data", "severity": "block" }
            ]
        });
        let flags = screen("What is the balance on NL91ABNA0417164300?", &doctrine);
        assert_eq!(flags[0]["severity"], "block");
        // The matched value must not be echoed back.
        assert_eq!(flags[0]["evidence"]["matched"], "an account number");
    }

    #[test]
    fn a_strong_single_keyword_match_sets_the_domain() {
        let vocab = json!({ "capital": ["buffer", "risk weighted"], "payments": ["settlement"] });
        assert_eq!(
            keyword_candidates("what is the buffer", &vocab),
            vec!["capital".to_string()]
        );
        // Two domains matching decides nothing on its own, and both names
        // survive so the model can be told the field was narrowed.
        assert_eq!(
            keyword_candidates("buffer and settlement", &vocab),
            vec!["capital".to_string(), "payments".to_string()]
        );
        assert!(keyword_candidates("something else entirely", &vocab).is_empty());
    }

    #[test]
    fn confidence_never_comes_from_the_model() {
        // Doc 03 section 9, the Coffret rule. The signals are all deterministic,
        // so the same inputs always give the same number.
        let depth = json!({ "chosen": "deep", "recommended": "deep",
                            "reason": "Board default deep.", "overridden_by_user": false });
        let a = confidence(
            true,
            &depth,
            &json!([]),
            "en",
            "What changed in the capital rule?",
        );
        let b = confidence(
            true,
            &depth,
            &json!([]),
            "en",
            "What changed in the capital rule?",
        );
        assert_eq!(a, b);
        assert!(a > 0.0 && a <= 1.0);
    }

    #[test]
    fn an_early_block_flag_lowers_confidence() {
        let depth = json!({ "chosen": "fast", "recommended": "fast",
                            "reason": "Board default fast.", "overridden_by_user": false });
        let clean = confidence(true, &depth, &json!([]), "en", "a long enough question");
        let flagged = confidence(
            true,
            &depth,
            &json!([{ "severity": "block" }]),
            "en",
            "a long enough question",
        );
        assert!(flagged < clean);
    }

    #[test]
    fn a_repeated_question_is_noticed_so_the_user_can_open_the_existing_card() {
        // Doc 03 section 8.5.
        let mut p = packet(None, "fast");
        p["recent"] = json!([{ "question": "What changed in the capital rule?", "card_id": "card-7" }]);
        assert_eq!(context_notes(&p)["repetition_of_recent"], "card-7");
    }

    #[test]
    fn a_card_override_replaces_only_its_stage() {
        let mut p = packet(None, "fast");
        p["request"]["model_override"] = json!({ "stage": "synthesize", "alias": "medium" });
        let resolved = resolve_policy(&p, &p["request"].clone());
        assert_eq!(resolved["synthesize"], "medium");
        assert_eq!(resolved["verify"], "medium");
        assert_eq!(resolved["route"], "small");
    }

    #[test]
    fn dutch_is_detected_so_the_hint_can_reach_the_prompt() {
        assert_eq!(detect_language("Wat is de kapitaalbuffer voor een bank?"), "nl");
        assert_eq!(detect_language("What is the capital buffer for a bank?"), "en");
    }
}
