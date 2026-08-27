//! The Exercise agent. Doc 08.
//!
//! Generates a short exercise from cards that already exist, so a reader can
//! check their understanding of a board.
//!
//! Doc 08 section 1: "It reads Visuals and answers only; it never retrieves and
//! never asks the model for new facts." The packet carries no passages, so that
//! property is structural rather than a rule the prompt asks for, the same way
//! the Visualizer's packet carries no question.
//!
//! Two deterministic checks stand between the draft and the exercise, and both
//! drop items rather than failing the run. Doc 08 section 9: "Always admitted; a
//! low ratio adds a caveat." An exercise with three good items is worth more
//! than no exercise, and an item that cannot be traced is worth less than none.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::prompts;

pub struct Exercise;

const SYSTEM: &str = "\
You write short multiple choice questions from material that has already been \
written and checked. You are not adding to it.

Every correct answer must be stated in the card you took it from. Do not write a \
question whose answer needs a fact the card does not contain, and do not write a \
distractor that is true, because a reader who knows the material would have to \
guess between two right answers.";

#[async_trait]
impl Agent for Exercise {
    fn id(&self) -> &str {
        "exercise"
    }
    fn packet_schema(&self) -> &'static str {
        ids::PACKET_EXERCISE
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_EXERCISE
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::EXERCISE
    }
    fn completion_event(&self) -> Option<&'static str> {
        None // The pipeline emits exercise.generated.v1 with the row write.
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        advance(ctx, "selecting_cards")?;

        let cards = packet["cards"].as_array().cloned().unwrap_or_default();
        if cards.is_empty() {
            // Doc 08 section 10's `no_eligible_cards`: an empty exercise with a
            // reason, not a failure. A board of fast cards has nothing checked
            // to check understanding of.
            return Ok(json!({
                "schema_version": "1.0",
                "agent_id": "exercise",
                "run_id": ctx.run_id,
                "title": "Nothing to check yet",
                "items": [],
                "confidence": 0.0,
                "caveats": [NO_ELIGIBLE_CARDS],
            }));
        }

        advance(ctx, "drafting")?;
        let drafted = self.draft(ctx, packet, &cards).await?;

        advance(ctx, "checking_traceability")?;
        let scope: Vec<&str> = packet["scope"]["card_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let (traceable, untraceable) = partition(&drafted, |item| traceable(item, &cards, &scope));

        advance(ctx, "checking_distractors")?;
        let concepts = packet["concepts"].as_array().cloned().unwrap_or_default();
        let (kept, leaking) = partition(&traceable, |item| !leaks_truth(item, &cards, &concepts));

        advance(ctx, "emitting")?;
        let max_items = packet["effort_budget"]["max_items"].as_u64().unwrap_or(8) as usize;
        let mut items: Vec<Value> = kept;
        items.truncate(max_items);

        // Doc 08 section 9: items passing both checks over items drafted.
        let confidence = if drafted.is_empty() {
            0.0
        } else {
            items.len() as f64 / drafted.len() as f64
        };

        let mut caveats: Vec<String> = Vec::new();
        let dropped = untraceable + leaking;
        if dropped > 0 {
            caveats.push(format!(
                "{dropped} of {} items were dropped: {untraceable} could not be traced to a card, \
                 {leaking} used a distractor that is true elsewhere on this board.",
                drafted.len()
            ));
        }

        Ok(json!({
            "schema_version": "1.0",
            "agent_id": "exercise",
            "run_id": ctx.run_id,
            "title": title_for(packet, &cards),
            "items": items,
            "confidence": confidence,
            "caveats": caveats,
        }))
    }
}

/// The state machine step, as a failure the run can report.
///
/// Doc 08 section 9 always admits the exercise, so this is the one thing here
/// that fails the run: a machine that cannot advance means the harness and the
/// agent disagree about where they are, and an exercise built past that point
/// would be built on a guess.
fn advance(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

/// What the model is asked for, narrowed to the shape doc 08 section 5 names.
///
/// The registry schema is what the harness validates the output against. This
/// is what the model is handed, and it is deliberately tighter: `additionalProperties`
/// is off so a model cannot answer with a field nobody reads, and the option
/// count is fixed so the template's choice reaches the prompt as a constraint
/// rather than as a sentence the model may round.
fn draft_schema(options: usize, kinds: &[&str]) -> Value {
    json!({
        "type": "object",
        "required": ["items"],
        "additionalProperties": false,
        "properties": {
            "items": {
                "type": "array",
                "maxItems": 8,
                "items": {
                    "type": "object",
                    "required": ["id", "kind", "prompt", "options", "answer_id", "explanation", "source_card_id"],
                    "additionalProperties": false,
                    "properties": {
                        "id": { "type": "string" },
                        "kind": { "enum": kinds },
                        "prompt": { "type": "string" },
                        "options": {
                            "type": "array",
                            "minItems": options.clamp(2, 6),
                            "maxItems": options.clamp(2, 6),
                            "items": {
                                "type": "object",
                                "required": ["id", "text"],
                                "additionalProperties": false,
                                "properties": {
                                    "id": { "type": "string" },
                                    "text": { "type": "string" }
                                }
                            }
                        },
                        "answer_id": { "type": "string" },
                        "explanation": { "type": "string" },
                        "source_card_id": { "type": "string" },
                        "citation_ordinals": { "type": "array", "items": { "type": "integer" } },
                        "concept_ids": { "type": "array", "items": { "type": "string" } }
                    }
                }
            }
        }
    })
}

/// Doc 08 section 10, said where the reader will see it.
const NO_ELIGIBLE_CARDS: &str =
    "No card on this board has been checked against a source yet, so there is nothing to test.";

impl Exercise {
    async fn draft(
        &self,
        ctx: &mut AgentContext<'_>,
        packet: &Value,
        cards: &[Value],
    ) -> Result<Vec<Value>, Failure> {
        let template = &packet["template"];
        // Doc 17 section 4: when a level is asked for, that level's kinds are
        // the only ones on offer, and the pack says which those are. When none
        // is asked for the template's own kinds stand, which is what a board
        // asking for an exercise of its own has always done.
        let asked = template["level"].as_u64().map(|l| l as u8);
        let kinds: Vec<&str> = match asked.and_then(|level| kinds_for(template, level)) {
            Some(kinds) => kinds,
            None => template["item_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect(),
        };
        let per_card = template["items_per_card_max"].as_u64().unwrap_or(2);
        let options = template["options"].as_u64().unwrap_or(4);

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "Write up to {per_card} question(s) per card, of these kinds: {}.\n",
            kinds.join(", ")
        ));
        prompt.push_str(&format!("Give each question {options} options.\n"));
        if let Some(audience) = packet["audience_id"].as_str() {
            // Doc 08 section 8: audience phrasing when set.
            prompt.push_str(&format!("Phrase every question for the audience: {audience}\n"));
        }
        prompt.push_str(&kind_guidance(&kinds));
        if let Some(level) = asked {
            prompt.push_str(&format!(
                "Every question checks the same understanding at level {level} of four.\n"
            ));
        }
        prompt.push_str("\nThe cards:\n");

        for card in cards {
            prompt.push_str(&format!(
                "\ncard_id: {}\nquestion: {}\nanswer: {}\n",
                card["card_id"].as_str().unwrap_or_default(),
                card["question"].as_str().unwrap_or_default(),
                card["answer"].as_str().unwrap_or_default()
            ));
            for text in finding_texts(card) {
                prompt.push_str(&format!("finding: {text}\n"));
            }
            for label in block_labels(card) {
                prompt.push_str(&format!("visual block: {label}\n"));
            }
            let titles = source_titles(card);
            if !titles.is_empty() {
                prompt.push_str(&format!("sources: {}\n", titles.join("; ")));
            }
        }

        let concepts: Vec<&str> = packet["concepts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["term"].as_str())
            .collect();
        if !concepts.is_empty() {
            prompt.push_str(&format!("\nTerms this board uses: {}\n", concepts.join(", ")));
        }

        let schema = draft_schema(options as usize, &kinds);
        let request = CompletionRequest::new(ctx.model_for("exercise"), "exercise")
            .system(format!("{SYSTEM}\n\n{}", prompts::json_only(&schema)))
            .user(prompt)
            // Doc 08 section 13: one medium call.
            .effort(Effort::Medium)
            .max_tokens(packet["effort_budget"]["max_tokens"].as_u64().unwrap_or(2500) as u32)
            .expecting(schema);

        let completion = ctx.call(&request).await?;
        let parsed = completion.json().map_err(|e| Failure {
            kind: "schema_violation".into(),
            detail: e.to_string(),
            recovery: Recovery::Retried,
            evidence: None,
            recoverable: true,
        })?;

        let mut items = parsed["items"].as_array().cloned().unwrap_or_default();
        for item in &mut items {
            match stamped_level(template, asked, item["kind"].as_str()) {
                Some(level) => item["level"] = json!(level),
                None => {
                    if let Some(object) = item.as_object_mut() {
                        object.remove("level");
                    }
                }
            }
        }
        Ok(items)
    }
}

/// Doc 17 section 4's rung for one item, written down rather than inferred
/// later.
///
/// The level asked for wins, because that is the rung the learner was put on
/// and a pack may list one kind at two levels. Failing that the pack says which
/// level the kind the model chose sits at. Failing that the item carries no
/// level: a pack that declares no ladder has none, and a number invented here
/// would be a fact about nothing that the tutor would then adapt from.
fn stamped_level(template: &Value, asked: Option<u8>, kind: Option<&str>) -> Option<u8> {
    asked.or_else(|| level_of(template, kind?))
}

/// The kinds a level asks for, from the pack's ladder the packet carries.
///
/// Nothing when the pack names no kinds for that level, so the caller falls
/// back to the template's own kinds instead of asking for an empty enum.
fn kinds_for(template: &Value, level: u8) -> Option<Vec<&str>> {
    let kinds: Vec<&str> = template["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["level"].as_u64() == Some(level as u64))?["kinds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    (!kinds.is_empty()).then_some(kinds)
}

/// The level a kind sits at, which is doctrine and never code.
///
/// The lowest level claiming the kind, because a pack may list `trace` at more
/// than one and the ladder reads upwards: the first rung that asks for it is
/// where a learner meets it.
fn level_of(template: &Value, kind: &str) -> Option<u8> {
    template["levels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| {
            t["kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k.as_str() == Some(kind))
        })
        .filter_map(|t| t["level"].as_u64())
        .min()
        .map(|l| l as u8)
}

/// Doc 08 section 8 point 2, said once rather than left to the model to infer.
fn kind_guidance(kinds: &[&str]) -> String {
    let mut out = String::new();
    for kind in kinds {
        out.push_str(match *kind {
            "recall" => "recall: ask for a fact the card states.\n",
            "apply" => "apply: give a short scenario and ask which rule applies.\n",
            "contrast" => "contrast: use two things the card sets against each other.\n",
            "trace" => "trace: ask which source supports a claim. The options are source titles.\n",
            // Doc 17 section 4's two new rungs.
            "explain" => "explain: ask why or how, and let the options be explanations.\n",
            "discriminate" => {
                "discriminate: take two near cases and ask which one differs and why. The wrong \
                 options must not be true of either case, including the neighbouring one.\n"
            }
            _ => "",
        });
    }
    out
}

fn title_for(packet: &Value, cards: &[Value]) -> String {
    // The board's own first question, because an exercise is about a board and
    // the board's first question is what it was opened to answer.
    let first = cards
        .first()
        .and_then(|c| c["question"].as_str())
        .unwrap_or("this board");
    let _ = packet;
    format!("Check your understanding of {}", trim_to(first, 60))
}

fn trim_to(s: &str, n: usize) -> String {
    let s = s.trim().trim_end_matches('?');
    if s.chars().count() <= n {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars().take(n.saturating_sub(1)).collect::<String>().trim_end()
    )
}

fn partition(items: &[Value], keep: impl Fn(&Value) -> bool) -> (Vec<Value>, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0usize;
    for item in items {
        if keep(item) {
            kept.push(item.clone());
        } else {
            dropped += 1;
        }
    }
    (kept, dropped)
}

fn card_by_id<'a>(cards: &'a [Value], card_id: &str) -> Option<&'a Value> {
    cards.iter().find(|c| c["card_id"].as_str() == Some(card_id))
}

fn block_labels(card: &Value) -> Vec<String> {
    card["visual"]["block_index"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b["label"].as_str().map(str::to_string))
        .collect()
}

/// Doc 06 section A7's finding shape: text plus the ordinals supporting it.
///
/// Doc 08 section 4's packet example writes `"findings": []` and does not say
/// the element type. The first version of this read them as strings and the
/// schema guard rejected the packet the first time a card with findings reached
/// it, which is the guard doing what doc 12 principle 1 built it for.
fn finding_texts(card: &Value) -> Vec<String> {
    card["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["text"].as_str().map(str::to_string))
        .collect()
}

fn source_titles(card: &Value) -> Vec<String> {
    card["citations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["source_title"].as_str().map(str::to_string))
        .collect()
}

/// Everything a card says, as one lowercase run with punctuation flattened.
///
/// Doc 08 section 5 checks the correct option "normalised", because a model that
/// writes `2.5%` where the card wrote `2.5 %` has still taken the answer from
/// the card, and a check that failed on the space would drop a good item.
fn normalised(card: &Value) -> String {
    let mut text = String::new();
    for part in [
        card["answer"].as_str().unwrap_or_default(),
        card["question"].as_str().unwrap_or_default(),
    ] {
        text.push(' ');
        text.push_str(part);
    }
    for finding in finding_texts(card) {
        text.push(' ');
        text.push_str(&finding);
    }
    for label in block_labels(card) {
        text.push(' ');
        text.push_str(&label);
    }
    for title in source_titles(card) {
        text.push(' ');
        text.push_str(&title);
    }
    flatten(&text)
}

fn flatten(s: &str) -> String {
    let lowered: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn option_text<'a>(item: &'a Value, option_id: &str) -> Option<&'a str> {
    item["options"]
        .as_array()?
        .iter()
        .find(|o| o["id"].as_str() == Some(option_id))?["text"]
        .as_str()
}

/// Doc 08 section 5's traceability rule, in full.
///
/// `answer_id` names an option; `source_card_id` is in scope; the correct
/// option's text appears normalised in that card; and every citation ordinal the
/// item claims exists on it.
pub fn traceable(item: &Value, cards: &[Value], scope: &[&str]) -> bool {
    let Some(card_id) = item["source_card_id"].as_str() else {
        return false;
    };
    if !scope.contains(&card_id) {
        return false;
    }
    let Some(card) = card_by_id(cards, card_id) else {
        return false;
    };

    let Some(answer_id) = item["answer_id"].as_str() else {
        return false;
    };
    let Some(answer) = option_text(item, answer_id) else {
        return false;
    };

    let flat_answer = flatten(answer);
    if flat_answer.is_empty() || !normalised(card).contains(&flat_answer) {
        return false;
    }

    let ordinals: Vec<i64> = card["citations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["n"].as_i64())
        .collect();
    item["citation_ordinals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .all(|n| ordinals.contains(&n))
}

/// Doc 08 section 5's distractor rule, with doc 17 section 4's addition.
///
/// A distractor that is a true statement from another card in scope is a second
/// right answer, and a reader who knows the material has to guess between them.
/// Doc 08 section 12 measures this as "distractor truth leakage 0".
///
/// Doc 17 section 4 adds one line for level 4: "not a true statement about a
/// neighbouring concept". A discriminate item is built out of two near cases,
/// so the neighbour is on the page by design, and a distractor that quietly
/// restates what the neighbour is gives the reader a second thing they can
/// defend. The concepts the packet carries are what a neighbour is measured
/// against, and the one the item names is not its own neighbour.
pub fn leaks_truth(item: &Value, cards: &[Value], concepts: &[Value]) -> bool {
    let Some(answer_id) = item["answer_id"].as_str() else {
        return true;
    };
    let source = item["source_card_id"].as_str().unwrap_or_default();

    let mut truths: Vec<String> = cards
        .iter()
        .filter(|c| c["card_id"].as_str() != Some(source))
        .map(normalised)
        .collect();
    if item["level"].as_u64() == Some(4) {
        truths.extend(neighbour_definitions(item, concepts));
    }

    for option in item["options"].as_array().into_iter().flatten() {
        if option["id"].as_str() == Some(answer_id) {
            continue;
        }
        let text = flatten(option["text"].as_str().unwrap_or_default());
        // A one word distractor is a word, not a statement. Checking it against
        // every other card would drop "yes" and "no" from every board that
        // happens to contain either.
        if text.split_whitespace().count() < 3 {
            continue;
        }
        if truths.iter().any(|other| other.contains(&text)) {
            return true;
        }
    }
    false
}

/// What the packet says is true of the concepts this item does not check.
///
/// A concept with no definition contributes nothing: its term alone is a name,
/// and a distractor naming the neighbour is what a discriminate item is for.
fn neighbour_definitions(item: &Value, concepts: &[Value]) -> Vec<String> {
    let own: Vec<&str> = item["concept_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    concepts
        .iter()
        .filter(|c| !own.contains(&c["concept_id"].as_str().unwrap_or_default()))
        .filter_map(|c| c["definition"].as_str())
        .map(flatten)
        .filter(|d| !d.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, answer: &str) -> Value {
        json!({
            "card_id": id,
            "question": "what is the buffer?",
            "answer": answer,
            "findings": [],
            "citations": [{ "n": 1, "source_title": "CRR" }],
        })
    }

    fn item(source: &str, correct: &str, distractor: &str) -> Value {
        json!({
            "id": "i1",
            "kind": "recall",
            "prompt": "what is it?",
            "options": [{ "id": "a", "text": correct }, { "id": "b", "text": distractor }],
            "answer_id": "a",
            "explanation": "the card states it",
            "source_card_id": source,
        })
    }

    #[test]
    fn an_answer_the_card_states_is_traceable_and_one_it_does_not_is_not() {
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        let scope = vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV"];

        assert!(traceable(
            &item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "2.5 per cent", "four per cent"),
            &cards,
            &scope
        ));
        assert!(!traceable(
            &item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "seven per cent", "four per cent"),
            &cards,
            &scope
        ));
    }

    #[test]
    fn punctuation_and_case_do_not_decide_traceability() {
        // A model that writes `2.5%` where the card wrote `2.5 per cent` has
        // still taken the answer from the card.
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The Buffer is 2.5 per cent!")];
        let scope = vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV"];
        assert!(traceable(
            &item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "the buffer is 2.5, per cent", "no"),
            &cards,
            &scope
        ));
    }

    #[test]
    fn a_card_outside_the_scope_is_not_traceable() {
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        assert!(!traceable(
            &item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "2.5 per cent", "no"),
            &cards,
            &[]
        ));
    }

    #[test]
    fn a_citation_the_card_does_not_have_is_not_traceable() {
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        let scope = vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV"];
        let mut bad = item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "2.5 per cent", "no");
        bad["citation_ordinals"] = json!([1, 9]);
        assert!(!traceable(&bad, &cards, &scope));
    }

    #[test]
    fn a_distractor_that_is_true_elsewhere_leaks() {
        // Doc 08 section 12: "distractor truth leakage 0". Two right answers is
        // a question a reader who knows the material has to guess at.
        let cards = vec![
            card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent."),
            card("01BX5ZZKBKACTAV9WEVGEMMVRZ", "The leverage ratio is 3 per cent."),
        ];
        assert!(leaks_truth(
            &item(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "2.5 per cent",
                "the leverage ratio is 3 per cent"
            ),
            &cards,
            &[]
        ));
        assert!(!leaks_truth(
            &item(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "2.5 per cent",
                "the buffer was withdrawn in 2019"
            ),
            &cards,
            &[]
        ));
    }

    #[test]
    fn a_one_word_distractor_is_a_word_and_not_a_statement() {
        // Checking every short option against every other card would drop "yes"
        // and "no" from any board containing either.
        let cards = vec![
            card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent."),
            card("01BX5ZZKBKACTAV9WEVGEMMVRZ", "No, the ratio did not change."),
        ];
        assert!(!leaks_truth(
            &item("01ARZ3NDEKTSV4RRFFQ69G5FAV", "2.5 per cent", "no"),
            &cards,
            &[]
        ));
    }
    /// The pack's ladder, as the packet carries it.
    fn ladder() -> Value {
        json!({
            "id": "t",
            "item_kinds": ["recall", "apply"],
            "levels": [
                { "level": 1, "kinds": ["recall"] },
                { "level": 2, "kinds": ["explain"] },
                { "level": 3, "kinds": ["apply", "trace"] },
                { "level": 4, "kinds": ["discriminate", "contrast"] },
            ],
        })
    }

    #[test]
    fn the_pack_says_which_kinds_a_level_asks_for_and_which_level_a_kind_sits_at() {
        let template = ladder();
        assert_eq!(kinds_for(&template, 2), Some(vec!["explain"]));
        assert_eq!(kinds_for(&template, 4), Some(vec!["discriminate", "contrast"]));
        assert_eq!(level_of(&template, "recall"), Some(1));
        assert_eq!(level_of(&template, "discriminate"), Some(4));
        // A kind no level claims has no level, and a level the pack never
        // declared has no kinds. Both are nothing rather than a guess.
        assert_eq!(level_of(&template, "sketch"), None);
        assert_eq!(kinds_for(&json!({ "levels": [] }), 1), None);
    }

    #[test]
    fn the_rung_asked_for_wins_over_the_one_the_kind_sits_at() {
        // A pack may list one kind at two levels, and `trace` at 3 and 4 is the
        // shape that makes the difference visible: the learner was put on rung
        // 4, and the tutor's next check moves from where they stood.
        let template = json!({
            "levels": [
                { "level": 3, "kinds": ["apply", "trace"] },
                { "level": 4, "kinds": ["discriminate", "trace"] },
            ],
        });
        assert_eq!(stamped_level(&template, Some(4), Some("trace")), Some(4));
        // Nothing asked, so the pack's lowest rung claiming the kind stands.
        assert_eq!(stamped_level(&template, None, Some("trace")), Some(3));
        // A kind no level claims, and a pack with no ladder at all.
        assert_eq!(stamped_level(&template, None, Some("sketch")), None);
        assert_eq!(stamped_level(&json!({}), None, Some("recall")), None);
    }

    #[test]
    fn a_level_four_distractor_that_is_true_of_a_neighbouring_concept_leaks() {
        // Doc 17 section 4: "not a true statement about a neighbouring
        // concept". A discriminate item puts the neighbour on the page by
        // design, so a distractor restating what the neighbour is gives the
        // reader a second answer they can defend.
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        let concepts = vec![
            json!({
                "concept_id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
                "term": "leverage ratio",
                "definition": "The leverage ratio is capital over total exposure, and it is not risk weighted.",
            }),
            json!({
                "concept_id": "01D78XYFJ1PRM1WPBCBT3VHMNV",
                "term": "capital buffer",
                "definition": "The buffer is 2.5 per cent of risk weighted assets.",
            }),
        ];

        let mut leaking = item(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2.5 per cent",
            "capital over total exposure",
        );
        leaking["level"] = json!(4);
        leaking["kind"] = json!("discriminate");
        leaking["concept_ids"] = json!(["01D78XYFJ1PRM1WPBCBT3VHMNV"]);
        assert!(leaks_truth(&leaking, &cards, &concepts));

        // The same distractor at level 1 is only measured against the cards,
        // which is doc 08's rule and all it ever was.
        let mut level_one = leaking.clone();
        level_one["level"] = json!(1);
        assert!(!leaks_truth(&level_one, &cards, &concepts));

        // A statement about the concept the item itself checks is not a
        // neighbour's truth. It is the card's, and the card rule already has it.
        let mut own = leaking.clone();
        own["concept_ids"] = json!(["01BX5ZZKBKACTAV9WEVGEMMVRZ"]);
        assert!(!leaks_truth(&own, &cards, &concepts));
    }

    #[test]
    fn naming_the_neighbour_is_what_a_discriminate_item_is_for() {
        // The neighbour's term alone is a name, not a claim about it, and a
        // rule that dropped items for naming the thing they contrast would drop
        // every level 4 item there is.
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        let concepts = vec![json!({
            "concept_id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "term": "leverage ratio",
            "definition": "The leverage ratio is capital over total exposure.",
        })];
        let mut named = item(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2.5 per cent",
            "the leverage ratio decides it instead",
        );
        named["level"] = json!(4);
        assert!(!leaks_truth(&named, &cards, &concepts));
    }

    #[test]
    fn a_neighbour_with_no_definition_says_nothing_that_can_leak() {
        let cards = vec![card("01ARZ3NDEKTSV4RRFFQ69G5FAV", "The buffer is 2.5 per cent.")];
        let concepts = vec![json!({
            "concept_id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "term": "leverage ratio",
            "definition": Value::Null,
        })];
        let mut level_four = item(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2.5 per cent",
            "capital over total exposure",
        );
        level_four["level"] = json!(4);
        assert!(!leaks_truth(&level_four, &cards, &concepts));
    }
}
