//! Scripted replies for the stages a mock has to answer.
//!
//! Two callers: the dev server that Playwright drives, and the eval's grounded
//! mock. They used to answer different stages, which meant a leg that ran under
//! one could not run under the other, and it meant two fixtures drifting apart
//! with nothing comparing them. Doc 02 section 10.1 scores the product against a
//! scripted provider; two scripts would score two products.
//!
//! Every reply here is built from the prompt it was given. That is the same
//! contract the grounded mock's other arms keep: quote what you were handed,
//! invent nothing. A fixture that made up a check question would measure the
//! fixture, and one that made up a concept would measure nothing at all.

use serde_json::{Value, json};
use tessera_providers::{CompletionRequest, ContentBlock};

/// Everything the request said, system prompt included.
pub fn prompt_of(request: &CompletionRequest) -> String {
    let mut text = request.system.clone().unwrap_or_default();
    for message in &request.messages {
        for block in &message.content {
            if let ContentBlock::Text { text: t } = block {
                text.push('\n');
                text.push_str(t);
            }
        }
    }
    text
}

/// The value after a `key: ` line, if the prompt carries one.
fn line_after(prompt: &str, key: &str) -> Option<String> {
    prompt
        .lines()
        .find_map(|l| l.trim().strip_prefix(key))
        .map(str::to_string)
}

/// The topic the Tutor and the Learning Planner are both told at the top.
fn topic_of(prompt: &str) -> String {
    line_after(prompt, "The learner wants to understand: ").unwrap_or_default()
}

/// Doc 14's Tutor, one turn at a time.
///
/// Which stage this is comes from the prompt, because the Tutor writes a
/// different instruction for each and the mock reads the one it was given.
pub fn tutor(prompt: &str) -> Value {
    let topic = topic_of(prompt);

    if prompt.contains("tappable options") {
        return json!({
            "questions": [
                { "q": "How much do you already know?",
                  "options": ["Nothing", "The basics", "A fair amount"] },
                { "q": "What do you need it for?",
                  "options": ["Curiosity", "Work", "An exam"] }
            ]
        });
    }

    if prompt.contains("Plan three to five cards") {
        // The questions are the topic asked three ways. A fixture that wrote a
        // syllabus would be writing the curriculum the Tutor is supposed to.
        return json!({
            "plan": {
                "title": topic,
                "cards": [
                    { "question": format!("what is {topic}?"), "why": "the foundation" },
                    { "question": format!("how does {topic} work?"), "why": "the mechanism" },
                    { "question": format!("where does {topic} apply?"), "why": "the landscape" }
                ]
            }
        });
    }

    if prompt.contains("multiple choice question") {
        let card_id = line_after(prompt, "card_id: ").unwrap_or_default();
        let answer = line_after(prompt, "answer: ").unwrap_or_default();
        // The card's first sentence, which is what doc 08 section 5's
        // traceability rule looks for in the card it names.
        let claim = answer
            .split_once(". ")
            .map(|(first, _)| first.to_string())
            .unwrap_or(answer);
        return json!({
            "check": {
                "item": {
                    "id": "c1",
                    "kind": "recall",
                    "prompt": "What does this card say?",
                    "options": [
                        { "id": "a", "text": claim },
                        { "id": "b", "text": "The card does not say." },
                        { "id": "c", "text": "The card defers to a later source." }
                    ],
                    "answer_id": "a",
                    "explanation": "The card opens with it.",
                    "source_card_id": card_id
                },
                "next_if_right": format!("how does {topic} work?"),
                "next_if_wrong": format!("what is {topic} made of?")
            }
        });
    }

    // Naming the topic rather than answering about it: doc 14 section 3.5 has
    // the tutor cite nothing and answer nothing itself, and the topic is the
    // one thing on this turn's prompt it is allowed to repeat.
    json!({
        "reply": format!("The cards on this board are what I am teaching about {topic}."),
        "open": null
    })
}

/// Doc 17 section 7's one model call: decomposing a topic into ideas.
///
/// The ideas are the topic's own parts, split where the learner joined them.
/// A fixture that named the prerequisites of a subject would be answering the
/// question the eval asks a real model, and scoring itself on the answer.
pub fn learning_plan(prompt: &str) -> Value {
    let topic = topic_of(prompt);
    let known: Vec<String> = line_after(prompt, "Ideas already on their map: ")
        .map(|line| {
            line.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let mut concepts: Vec<Value> = Vec::new();
    for part in topic
        .split(" and ")
        .flat_map(|p| p.split(','))
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        if known.contains(&part.to_lowercase()) {
            continue;
        }
        concepts.push(json!({ "term": part, "why": "The learner named it." }));
    }

    // One edge, and only when the topic named two parts in order: the first
    // thing someone says is usually the thing they start from.
    let edges = if concepts.len() >= 2 {
        vec![json!({
            "from_term": concepts[0]["term"].clone(),
            "to_term": concepts[1]["term"].clone()
        })]
    } else {
        Vec::new()
    };

    json!({ "concepts": concepts, "edges": edges })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tutors_plan_is_the_topic_it_was_given() {
        let prompt = "The learner wants to understand: liquidity risk\nPlan three to five cards";
        let out = tutor(prompt);
        assert_eq!(out["plan"]["title"], "liquidity risk");
        assert!(
            out["plan"]["cards"][0]["question"]
                .as_str()
                .is_some_and(|q| q.contains("liquidity risk"))
        );
    }

    #[test]
    fn a_check_quotes_the_card_it_names() {
        let prompt = "The learner wants to understand: buffers\nWrite one multiple choice question\n\
                      card_id: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n\
                      answer: The buffer is 2.5 per cent. It applies at consolidated level.";
        let out = tutor(prompt);
        assert_eq!(
            out["check"]["item"]["options"][0]["text"],
            "The buffer is 2.5 per cent"
        );
        assert_eq!(
            out["check"]["item"]["source_card_id"],
            "01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[test]
    fn a_decomposition_names_the_parts_the_learner_said() {
        let prompt = "The learner wants to understand: liquidity risk and capital buffers\n\
                      Ideas already on their map: capital buffers\n";
        let out = learning_plan(prompt);
        let terms: Vec<&str> = out["concepts"]
            .as_array()
            .expect("concepts")
            .iter()
            .filter_map(|c| c["term"].as_str())
            .collect();
        assert_eq!(terms, vec!["liquidity risk"], "the map's own idea came back");
        // One part left, so there is nothing to order.
        assert!(out["edges"].as_array().expect("edges").is_empty());
    }
}
