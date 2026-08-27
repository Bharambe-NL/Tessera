//! The Tutor. Doc 14.
//!
//! Doc 14 section 1: "Learn mode adds no new answer path. Curated cards are
//! ordinary cards through Router, retrievers, Synthesizer, Visualizer, and
//! Verifier. Check questions are Exercise items with a single-card scope. The
//! tutor's only new job is choosing what to ask and what to open next."
//!
//! So this agent writes no card content and retrieves nothing. It decides, and
//! four deterministic rules from doc 14 section 3.5 stand between its decision
//! and the session:
//!
//! - a check item passes the Exercise agent's own traceability and distractor
//!   checks, reused rather than reimplemented;
//! - `next_if_right` and `next_if_wrong` reference the target card's entities;
//! - at most one card is requested per turn;
//! - no reply carries a citation marker, because the tutor cites nothing and
//!   cards do.
//!
//! The last one is the load bearing one. A tutor that writes `[1]` into a reply
//! is claiming a source for a sentence nobody verified, in a product whose whole
//! argument is that a marker means the Verifier stood behind it.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::exercise;
use crate::prompts;

pub struct Tutor;

const SYSTEM: &str = "\
You are teaching from material this product wrote and checked. You decide what \
to ask and what to open next. You never answer a question about the topic \
yourself.

When the learner asks something the board does not cover, do not tell them the \
answer. Propose a card, and the product will answer it with sources. A reply \
you write carries no citation, because nothing checked it.

Keep every line short enough to read on a phone. One idea a line.";

#[async_trait]
impl Agent for Tutor {
    fn id(&self) -> &str {
        "tutor"
    }
    fn packet_schema(&self) -> &'static str {
        ids::PACKET_TUTOR
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_TUTOR
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::TUTOR
    }
    fn completion_event(&self) -> Option<&'static str> {
        None // The pipeline emits the learn.* event for the stage it ran.
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        advance(ctx, "reading_session")?;
        let stage = packet["stage"].as_str().unwrap_or("intake").to_string();

        advance(ctx, "deciding")?;
        let decided = self.decide(ctx, packet, &stage).await?;

        advance(ctx, "checking_rules")?;
        let (out, dropped) = enforce(decided, packet, &stage);

        advance(ctx, "emitting")?;
        let mut caveats: Vec<String> = Vec::new();
        for reason in &dropped {
            caveats.push(reason.clone());
        }

        // Absent, not null. The output schema makes each stage's field optional,
        // and a `null` plan is a claim that there is a plan and it is nothing.
        // The schema guard said so the first time a turn ran.
        let mut result = json!({
            "schema_version": "1.0",
            "agent_id": "tutor",
            "run_id": ctx.run_id,
            "stage": stage,
            "confidence": if dropped.is_empty() { 1.0 } else { 0.5 },
            "caveats": caveats,
        });
        for (key, value) in [
            ("questions", out.questions),
            ("plan", out.plan),
            ("check", out.check),
            ("reply", out.reply),
            ("open", out.open),
        ] {
            if !value.is_null() {
                result[key] = value;
            }
        }
        Ok(result)
    }
}

fn advance(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

impl Tutor {
    async fn decide(
        &self,
        ctx: &mut AgentContext<'_>,
        packet: &Value,
        stage: &str,
    ) -> Result<Value, Failure> {
        let mut prompt = String::new();
        prompt.push_str(&format!(
            "The learner wants to understand: {}\n",
            packet["session"]["topic"].as_str().unwrap_or_default()
        ));

        match stage {
            "intake" => {
                // Doc 14 section 6 question 2, resolved as proposed: a role
                // already on the profile is not asked for again.
                if let Some(role) = packet["profile"]["role"].as_str() {
                    prompt.push_str(&format!(
                        "You already know they work as: {role}. Do not ask what they do.\n"
                    ));
                }
                prompt.push_str(
                    "Ask two or three questions that change what you would teach. Each takes \
                     three tappable options and no free text.\n",
                );
                for template in packet["doctrine"]["intake_questions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    prompt.push_str(&format!("A question this domain usually asks: {template}\n"));
                }
            }
            "building" => {
                let shapes: Vec<&str> = packet["doctrine"]["curriculum_shapes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                prompt.push_str(&format!(
                    "Plan three to five cards, ordered from foundation to detail, following these \
                     shapes where they fit: {}.\nEach card is a question the product will answer \
                     with sources, plus why it is there.\n",
                    shapes.join(", ")
                ));
                answers(&mut prompt, packet);
            }
            "checking" => {
                prompt.push_str(
                    "Write one multiple choice question about the target card below, and two \
                     follow-up questions: one to open if they get it right and one if they get it \
                     wrong. Both must be about something the target card names.\n",
                );
                cards(&mut prompt, packet, packet["target_card_id"].as_str());
            }
            _ => {
                if let Some(message) = packet["learner_message"].as_str() {
                    prompt.push_str(&format!("\nThe learner said: {message}\n"));
                }
                prompt.push_str(
                    "\nReply in two sentences at most, using only what the cards below say. If \
                     they asked something the cards do not cover, say so and put the question you \
                     would open in `open`.\n",
                );
                cards(&mut prompt, packet, None);
            }
        }

        let schema = stage_schema(stage);
        let request = CompletionRequest::new(ctx.model_for("tutor"), "tutor")
            .system(format!("{SYSTEM}\n\n{}", prompts::json_only(&schema)))
            .user(prompt)
            .effort(Effort::Medium)
            .max_tokens(packet["effort_budget"]["max_tokens"].as_u64().unwrap_or(1500) as u32)
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
}

fn answers(prompt: &mut String, packet: &Value) {
    for pair in packet["session"]["intake"].as_array().into_iter().flatten() {
        let (Some(q), Some(a)) = (pair["q"].as_str(), pair["a"].as_str()) else {
            continue;
        };
        prompt.push_str(&format!("They answered \"{q}\" with: {a}\n"));
    }
}

fn cards(prompt: &mut String, packet: &Value, only: Option<&str>) {
    prompt.push_str("\nThe cards on this board:\n");
    for card in packet["cards"].as_array().into_iter().flatten() {
        let id = card["card_id"].as_str().unwrap_or_default();
        if only.is_some_and(|target| target != id) {
            continue;
        }
        prompt.push_str(&format!(
            "\ncard_id: {id}\nquestion: {}\nanswer: {}\n",
            card["question"].as_str().unwrap_or_default(),
            card["answer"].as_str().unwrap_or_default()
        ));
        let labels: Vec<&str> = card["visual_labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !labels.is_empty() {
            prompt.push_str(&format!("visual labels: {}\n", labels.join(", ")));
        }
    }
}

/// What the model is asked for, narrowed to the one stage that is running.
///
/// The output schema covers every stage because one row holds them all. Asking a
/// model for every field at once invites it to fill in the ones that do not
/// apply, so each stage is handed only its own shape.
fn stage_schema(stage: &str) -> Value {
    match stage {
        "intake" => json!({
            "type": "object", "required": ["questions"], "additionalProperties": false,
            "properties": { "questions": {
                "type": "array", "minItems": 2, "maxItems": 3,
                "items": {
                    "type": "object", "required": ["q", "options"], "additionalProperties": false,
                    "properties": {
                        "q": { "type": "string" },
                        "options": { "type": "array", "minItems": 3, "maxItems": 3,
                                     "items": { "type": "string" } } } } } }
        }),
        "building" => json!({
            "type": "object", "required": ["plan"], "additionalProperties": false,
            "properties": { "plan": {
                "type": "object", "required": ["title", "cards"], "additionalProperties": false,
                "properties": {
                    "title": { "type": "string" },
                    "cards": {
                        "type": "array", "minItems": 3, "maxItems": 5,
                        "items": {
                            "type": "object", "required": ["question", "why"],
                            "additionalProperties": false,
                            "properties": {
                                "question": { "type": "string" },
                                "why": { "type": "string" },
                                "visual_hint": { "type": "string" } } } } } } }
        }),
        "checking" => json!({
            "type": "object", "required": ["check"], "additionalProperties": false,
            "properties": { "check": {
                "type": "object", "required": ["item", "next_if_right", "next_if_wrong"],
                "additionalProperties": false,
                "properties": {
                    "item": {
                        "type": "object",
                        "required": ["id", "kind", "prompt", "options", "answer_id", "explanation", "source_card_id"],
                        "additionalProperties": false,
                        "properties": {
                            "id": { "type": "string" },
                            "kind": { "enum": ["recall", "apply", "contrast", "trace"] },
                            "prompt": { "type": "string" },
                            "options": { "type": "array", "minItems": 3, "maxItems": 4, "items": {
                                "type": "object", "required": ["id", "text"], "additionalProperties": false,
                                "properties": { "id": { "type": "string" }, "text": { "type": "string" } } } },
                            "answer_id": { "type": "string" },
                            "explanation": { "type": "string" },
                            "source_card_id": { "type": "string" },
                            "citation_ordinals": { "type": "array", "items": { "type": "integer" } } } },
                    "next_if_right": { "type": "string" },
                    "next_if_wrong": { "type": "string" } } } }
        }),
        _ => json!({
            "type": "object", "required": ["reply"], "additionalProperties": false,
            "properties": {
                "reply": { "type": "string" },
                "open": { "type": ["string", "null"] } }
        }),
    }
}

// ------------------------------------------------------------- the rules --

#[derive(Default)]
struct Decision {
    questions: Value,
    plan: Value,
    check: Value,
    reply: Value,
    open: Value,
}

/// Doc 14 section 3.5's harness rules, applied to what the model answered.
///
/// Each one drops the offending part rather than failing the turn. Doc 14
/// section 3.7 always admits: "the learner sees every decision as a choice,
/// never as an automatic action", so a turn that lost its check still offers a
/// reply, and the learner is never left staring at nothing.
fn enforce(decided: Value, packet: &Value, stage: &str) -> (Decision, Vec<String>) {
    let mut out = Decision {
        questions: decided["questions"].clone(),
        plan: decided["plan"].clone(),
        check: Value::Null,
        reply: Value::Null,
        open: Value::Null,
    };
    let mut dropped: Vec<String> = Vec::new();

    // Rule 4, first because it is the load bearing one: the tutor cites nothing.
    if let Some(reply) = decided["reply"].as_str() {
        if has_citation_marker(reply) {
            dropped.push(
                "The tutor's reply carried a citation marker. It was dropped: only a card cites.".into(),
            );
        } else {
            out.reply = json!(reply);
        }
    }

    // Rule 3: at most one card per turn, and the session's own budget.
    let requested = packet["budget"]["cards_requested"].as_u64().unwrap_or(0);
    let max = packet["budget"]["cards_max"].as_u64().unwrap_or(8);
    if let Some(open) = decided["open"].as_str().filter(|o| !o.trim().is_empty()) {
        if requested >= max {
            dropped.push(format!(
                "This session has already opened {requested} cards, so the proposed card was not \
                 requested. The learner can still ask for another."
            ));
        } else {
            out.open = json!(open);
        }
    }

    if stage != "checking" {
        return (out, dropped);
    }

    // Rules 1 and 2, on the check.
    let check = decided["check"].clone();
    let item = check["item"].clone();
    let cards = packet_cards(packet);
    let scope: Vec<&str> = cards.iter().filter_map(|c| c["card_id"].as_str()).collect();

    // Rule 1: the Exercise agent's own two checks, reused. A check item is an
    // Exercise item (doc 14 section 1), so it is held to an Exercise item's
    // standard by the same code rather than by a second opinion.
    if !exercise::traceable(&item, &cards, &scope) {
        dropped.push("The check question could not be traced to a card, so it was dropped.".into());
        return (out, dropped);
    }
    if exercise::leaks_truth(&item, &cards) {
        dropped.push("The check question had a second right answer, so it was dropped.".into());
        return (out, dropped);
    }

    // Rule 2: the next questions reference the target card's entities.
    let target = item["source_card_id"].as_str().unwrap_or_default();
    let Some(card) = cards.iter().find(|c| c["card_id"].as_str() == Some(target)) else {
        dropped.push("The check named a card that is not on this board.".into());
        return (out, dropped);
    };

    let mut kept = check.clone();
    for field in ["next_if_right", "next_if_wrong"] {
        let question = check[field].as_str().unwrap_or_default();
        if !overlaps(question, card) {
            dropped.push(format!(
                "The {} question was about something this card does not mention, so it was dropped.",
                if field == "next_if_right" {
                    "next"
                } else {
                    "remedial"
                }
            ));
            kept[field] = Value::Null;
        }
    }
    out.check = kept;

    (out, dropped)
}

fn packet_cards(packet: &Value) -> Vec<Value> {
    packet["cards"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            // The Exercise checks read `findings` and `visual.block_index`; a
            // tutor packet carries labels flat, so they are shaped here rather
            // than the checks learning a second shape.
            let labels: Vec<Value> = c["visual_labels"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| json!({ "label": l }))
                .collect();
            json!({
                "card_id": c["card_id"],
                "question": c["question"],
                "answer": c["answer"],
                "findings": [],
                "visual": { "block_index": labels },
                "citations": c["citations"].as_array().cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// Doc 14 section 3.5's deterministic overlap check.
///
/// A next question has to be about the card it follows. "Reference the target
/// card's entities" is tested as a shared content word: a question that shares
/// nothing but "the" and "what" is about something else, and opening it would
/// take the learner off the path they are on.
pub fn overlaps(question: &str, card: &Value) -> bool {
    let mut card_text = String::new();
    for part in [card["question"].as_str(), card["answer"].as_str()] {
        card_text.push(' ');
        card_text.push_str(part.unwrap_or_default());
    }
    for block in card["visual"]["block_index"].as_array().into_iter().flatten() {
        card_text.push(' ');
        card_text.push_str(block["label"].as_str().unwrap_or_default());
    }

    let subject: std::collections::BTreeSet<String> = content_words(&card_text);
    let asked = content_words(question);
    asked.iter().any(|word| subject.contains(word))
}

/// Words that carry the subject. The closed class is dropped, because every
/// question in English shares it.
fn content_words(text: &str) -> std::collections::BTreeSet<String> {
    const CLOSED: [&str; 32] = [
        "the", "a", "an", "and", "or", "but", "if", "of", "to", "in", "on", "for", "with", "at", "by",
        "from", "is", "are", "was", "were", "be", "been", "it", "its", "this", "that", "what", "which",
        "how", "why", "when", "does",
    ];
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| w.chars().count() > 2 && !CLOSED.contains(&w.as_str()))
        .collect()
}

/// Doc 14 section 3.5: no reply may contain a citation marker.
pub fn has_citation_marker(text: &str) -> bool {
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '[' {
            let mut j = i + 1;
            let mut digits = 0;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == ',' || bytes[j] == ' ') {
                if bytes[j].is_ascii_digit() {
                    digits += 1;
                }
                j += 1;
            }
            if digits > 0 && j < bytes.len() && bytes[j] == ']' {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Value {
        json!({
            "card_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "question": "What is the capital conservation buffer?",
            "answer": "The capital conservation buffer is 2.5 per cent of risk weighted assets.",
            "findings": [],
            "visual": { "block_index": [{ "label": "Buffer" }] },
            "citations": [{ "n": 1, "source_title": "CRR" }]
        })
    }

    fn packet(stage: &str) -> Value {
        json!({
            "stage": stage,
            "session": { "topic": "capital rules" },
            "cards": [{
                "card_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "question": "What is the capital conservation buffer?",
                "answer": "The capital conservation buffer is 2.5 per cent of risk weighted assets.",
                "visual_labels": ["Buffer"],
                "citations": [{ "n": 1, "source_title": "CRR" }]
            }],
            "target_card_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "budget": { "cards_requested": 0, "cards_max": 8 }
        })
    }

    fn good_check() -> Value {
        json!({
            "check": {
                "item": {
                    "id": "c1",
                    "kind": "recall",
                    "prompt": "How large is the buffer?",
                    "options": [
                        { "id": "a", "text": "2.5 per cent" },
                        { "id": "b", "text": "the card gives a range" },
                        { "id": "c", "text": "the card defers to a later rule" }
                    ],
                    "answer_id": "a",
                    "explanation": "The card states it.",
                    "source_card_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"
                },
                "next_if_right": "How is the capital conservation buffer calculated?",
                "next_if_wrong": "What are risk weighted assets?"
            }
        })
    }

    #[test]
    fn a_reply_carrying_a_citation_marker_is_dropped() {
        // Doc 14 section 3.5's load bearing rule. A marker means the Verifier
        // stood behind the sentence, and nothing checked this one.
        assert!(has_citation_marker("The buffer is 2.5 per cent [1]."));
        assert!(has_citation_marker("Both apply [1, 2]."));
        assert!(!has_citation_marker("The buffer is 2.5 per cent."));
        // Not every bracket is a marker.
        assert!(!has_citation_marker("Article 92 [see the card] covers it."));

        let (out, dropped) = enforce(
            json!({ "reply": "The buffer is 2.5 per cent [1]." }),
            &packet("reading"),
            "reading",
        );
        assert!(out.reply.is_null());
        assert_eq!(dropped.len(), 1);
    }

    #[test]
    fn a_reply_without_a_marker_survives() {
        let (out, dropped) = enforce(
            json!({ "reply": "The card says it is 2.5 per cent." }),
            &packet("reading"),
            "reading",
        );
        assert_eq!(out.reply, "The card says it is 2.5 per cent.");
        assert!(dropped.is_empty());
    }

    #[test]
    fn a_check_that_passes_both_exercise_rules_survives_with_its_next_questions() {
        let (out, dropped) = enforce(good_check(), &packet("checking"), "checking");
        assert!(dropped.is_empty(), "{dropped:?}");
        assert_eq!(
            out.check["next_if_right"],
            "How is the capital conservation buffer calculated?"
        );
        assert_eq!(out.check["next_if_wrong"], "What are risk weighted assets?");
    }

    #[test]
    fn a_check_whose_answer_is_not_in_the_card_is_dropped() {
        // Doc 14 section 1: a check question is an Exercise item, so it is held
        // to an Exercise item's standard by the same code.
        let mut bad = good_check();
        bad["check"]["item"]["options"][0]["text"] = json!("seven per cent");
        let (out, dropped) = enforce(bad, &packet("checking"), "checking");
        assert!(out.check.is_null());
        assert!(dropped[0].contains("traced"));
    }

    #[test]
    fn a_next_question_about_something_else_is_dropped_and_the_check_stays() {
        // Doc 14 section 3.5's overlap rule. Opening a question the card does
        // not mention takes the learner off the path they are on, and losing
        // the whole check over it would cost them the question too.
        let mut bad = good_check();
        bad["check"]["next_if_wrong"] = json!("How do songbirds migrate?");
        let (out, dropped) = enforce(bad, &packet("checking"), "checking");
        assert_eq!(
            out.check["next_if_right"],
            "How is the capital conservation buffer calculated?"
        );
        assert!(out.check["next_if_wrong"].is_null());
        assert_eq!(dropped.len(), 1);
        assert!(dropped[0].contains("remedial"));
    }

    #[test]
    fn overlap_ignores_the_words_every_question_shares() {
        // "What is the ...?" shares four words with every card on every board.
        assert!(!overlaps("What is this?", &card()));
        assert!(overlaps("What is a risk weighted asset?", &card()));
    }

    #[test]
    fn a_session_at_its_card_budget_stops_opening() {
        // Doc 14 section 3.5: at most eight per session without the learner
        // choosing another. The tutor stops proposing; the learner can still ask.
        let mut spent = packet("reading");
        spent["budget"]["cards_requested"] = json!(8);
        let (out, dropped) = enforce(
            json!({ "reply": "Here is where that goes next.", "open": "What comes after?" }),
            &spent,
            "reading",
        );
        assert!(out.open.is_null());
        assert!(!out.reply.is_null(), "the reply went with the card");
        assert!(dropped[0].contains("already opened"));
    }
}
