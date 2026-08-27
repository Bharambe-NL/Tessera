//! The learning record. Doc 17 section 5.
//!
//! "Each lesson ends with a learning record: a page generated from the lesson's
//! verified cards and check outcomes (what was covered, what was checked, what
//! remains), saved to the vault under `vault/learning/<mission>/<date>.md` with
//! citations carried."
//!
//! Generated rather than written, and generated from rows rather than from a
//! model: every line of it is something the session already recorded, so a
//! record cannot say the learner covered a card they never opened or passed a
//! check they never took. Doc 17 section 10 gates that at 1.00 and this is what
//! makes the gate meaningful rather than a measurement of a prompt.
//!
//! The citations are carried, not re-derived. Doc 16 section 3.2 settled that
//! rule for Save as page and it holds here for the same reason: the page is a
//! note about cards, and a figure on it rests on the passage the card cited
//! rather than on the page having repeated it.

use serde_json::{Value, json};
use tessera_store::Store;
use tessera_store::repo;

/// Where doc 17 section 5 puts a record.
pub const FOLDER: &str = "learning";

/// What one lesson produced, ready to become a page.
#[derive(Debug, Clone)]
pub struct Record {
    pub title: String,
    pub folder: String,
    pub body: String,
    /// The `{ordinal, passage_id}` pairs the cards cited, deduplicated and
    /// renumbered from one so the page's own markers mean something.
    pub citations_carried: Value,
    /// Doc 17 section 5's three sections, as counts, so the event says what the
    /// page contains without anybody parsing the markdown back.
    pub covered: usize,
    pub checked: usize,
    pub remains: usize,
    /// One entry per line the record wrote, naming the row it rests on.
    ///
    /// Doc 17 section 10 gates traceability at 1.00, and a gate that read the
    /// markdown back would be measuring a formatter. This is what makes the
    /// number checkable by something that never saw the generator: every line
    /// names a card, a check or a concept, and a scorer holding the session's
    /// own rows can ask whether that row is there.
    pub lines: Vec<Value>,
}

/// Build the record for a lesson. Doc 17 section 5.
///
/// `today` is passed in rather than read from the clock, because a record is
/// named after the day it covers and a test that could not say which day it was
/// would be asserting against `now`.
pub fn build(
    store: &Store,
    board_id: &str,
    session: &Value,
    mission: &Value,
    today: &str,
) -> Result<Record, tessera_store::StoreError> {
    // Doc 17 section 5: "the lesson's verified cards". The same eligibility the
    // Exercise agent uses, because "verified" has one meaning in this product
    // and a record listing a blocked card would be a note about something the
    // Verifier refused.
    //
    // The eligibility comes from there and the citations come from the board,
    // because the exercise packet's citations are `{n, source_title}` (what an
    // item may ask about) while a carried citation needs the passage id (what
    // the evidence actually is). Two shapes for two jobs, and reading the wrong
    // one carries a page with no evidence on it, which is what the first
    // version of this did.
    let eligible: Vec<String> = repo::cards_for_exercise(store, board_id, 64)?
        .iter()
        .filter_map(|c| c["card_id"].as_str().map(str::to_string))
        .collect();
    let board = repo::read_board(store, board_id)?;
    let cards: Vec<Value> = board
        .into_iter()
        .flat_map(|b| b.cards)
        .filter(|c| eligible.contains(&c.id))
        .map(|c| {
            json!({
                "card_id": c.id,
                "question": c.question,
                "answer": c.answer.unwrap_or_default(),
                "citations": c.citations,
            })
        })
        .collect();
    let checks = session["checks"].as_array().cloned().unwrap_or_default();

    let mission_name = mission["statement"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("no mission");
    let topic = session["topic"].as_str().unwrap_or("this lesson");

    // Doc 17 section 5's three sections, in the order it names them.
    let mut body = format!("# {topic}\n\n{today}, working on: {mission_name}\n");

    body.push_str("\n## What was covered\n\n");
    if cards.is_empty() {
        body.push_str("Nothing on this board was checked against a source.\n");
    }
    let mut carried: Vec<Value> = Vec::new();
    let mut lines: Vec<Value> = Vec::new();
    let mut ordinal = 0i64;
    for card in &cards {
        let question = card["question"].as_str().unwrap_or_default();
        let answer = card["answer"].as_str().unwrap_or_default();
        // The card's own citation markers renumbered into the page's sequence,
        // so `[1]` on the page names the passage the page carries rather than
        // the ordinal the card happened to use.
        let mut markers: Vec<String> = Vec::new();
        let mut rests_on: Vec<String> = Vec::new();
        for citation in card["citations"].as_array().into_iter().flatten() {
            let Some(passage_id) = citation["passage_id"].as_str() else {
                continue;
            };
            rests_on.push(passage_id.to_string());
            if let Some(seen) = carried
                .iter()
                .find(|c: &&Value| c["passage_id"].as_str() == Some(passage_id))
            {
                markers.push(format!("[{}]", seen["ordinal"].as_i64().unwrap_or(0)));
                continue;
            }
            ordinal += 1;
            carried.push(json!({ "ordinal": ordinal, "passage_id": passage_id }));
            markers.push(format!("[{ordinal}]"));
        }
        let marks = if markers.is_empty() {
            String::new()
        } else {
            format!(" {}", markers.join(""))
        };
        body.push_str(&format!("- **{question}** {answer}{marks}\n"));
        lines.push(json!({
            "section": "covered",
            "card_id": card["card_id"].clone(),
            "passages": rests_on,
        }));
    }

    body.push_str("\n## What was checked\n\n");
    if checks.is_empty() {
        body.push_str("No check was asked.\n");
    }
    for check in &checks {
        let level = check["level"].as_i64().unwrap_or(1);
        let verdict = if check["correct"] == true {
            "right"
        } else {
            "not yet"
        };
        let about = terms_for(store, check).join(", ");
        let about = if about.is_empty() {
            String::new()
        } else {
            format!("{about}, ")
        };
        body.push_str(&format!("- {about}level {level}: {verdict}\n"));
        lines.push(json!({
            "section": "checked",
            "concept_ids": check["concept_ids"].clone(),
            "level": level,
            "correct": check["correct"] == true,
        }));
    }

    // Doc 17 section 5's "what remains": the concepts this lesson checked and
    // did not settle. Derived from the checks rather than from the map, because
    // a record is a note about a lesson and the map moves on.
    let remaining = unsettled(store, &checks);
    body.push_str("\n## What remains\n\n");
    if remaining.is_empty() {
        body.push_str("Nothing from this lesson is still open.\n");
    }
    for (concept_id, term) in &remaining {
        body.push_str(&format!("- {term}\n"));
        lines.push(json!({ "section": "remains", "concept_id": concept_id }));
    }

    Ok(Record {
        title: format!("{topic}, {today}"),
        // Doc 17 section 5's path. The mission's own name, slugged, so a
        // learner reading the folder sees what they were working towards.
        folder: format!("{FOLDER}/{}", crate::vault::slug(mission_name)),
        body,
        citations_carried: Value::Array(carried),
        covered: cards.len(),
        checked: checks.len(),
        remains: remaining.len(),
        lines,
    })
}

/// The terms a check was about, for a line a person can read.
fn terms_for(store: &Store, check: &Value) -> Vec<String> {
    check["concept_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str())
        .filter_map(|id| {
            store
                .conn()
                .query_row(
                    "SELECT term FROM concept WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
        })
        .collect()
}

/// Concepts this lesson asked about and left open, as `(id, term)`.
///
/// Open means the last check on the concept was failed. A concept the learner
/// got right last is not "remaining" from this lesson's point of view, whatever
/// the ladder says about the next rung: doc 17 section 5 asks for what remains,
/// and the next question is not the same as an unfinished one.
///
/// The id travels with the term because the page shows the term and the gate
/// checks the id: a learner reads "capital buffer" and a scorer asks whether a
/// check on that concept was failed.
fn unsettled(store: &Store, checks: &[Value]) -> Vec<(String, String)> {
    let mut last: std::collections::BTreeMap<String, bool> = Default::default();
    for check in checks {
        for id in check["concept_ids"].as_array().into_iter().flatten() {
            if let Some(id) = id.as_str() {
                last.insert(id.to_string(), check["correct"] == true);
            }
        }
    }
    last.into_iter()
        .filter(|(_, passed)| !passed)
        .filter_map(|(id, _)| {
            let term = store
                .conn()
                .query_row(
                    "SELECT term FROM concept WHERE id = ?1",
                    rusqlite::params![&id],
                    |r| r.get::<_, String>(0),
                )
                .ok()?;
            Some((id, term))
        })
        .collect()
}
