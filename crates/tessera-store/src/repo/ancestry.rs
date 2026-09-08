//! The walk up a card's parents, which two packets read.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::sql::read_citations;
use crate::Store;
use crate::error::Result;

/// One card in a follow-up's ancestry, in the shape both packets want.
///
/// Doc 03 section 4 gives the Router the immediate parent; doc 04 section 4
/// gives the Planner up to three ancestors. Both read the same rows, so they
/// read them through one query.
#[derive(Debug, Clone)]
pub struct Ancestor {
    pub card_id: String,
    pub question: String,
    pub answer: Option<String>,
    pub depth: String,
    pub confidence: Option<f64>,
    pub answered_at: Option<String>,
    pub citations: Vec<Value>,
}

impl Ancestor {
    pub fn stale_citations(&self) -> usize {
        self.citations
            .iter()
            .filter(|c| c["stale"] == json!(true))
            .count()
    }
}

/// Walk up from a card through `parent_card_id`, nearest first.
///
/// Doc 04 section 4 caps the chain at three, and the cap is enforced here
/// rather than by the caller because an unbounded walk on a deep board would
/// put an entire thread into a prompt.
///
/// A cycle would hang this, and `parent_card_id` is a plain foreign key that
/// nothing stops from pointing at a descendant, so the walk also refuses to
/// visit a card twice.
pub fn ancestor_chain(store: &Store, card_id: &str, limit: usize) -> Result<Vec<Ancestor>> {
    let conn = store.conn();
    let mut chain = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(card_id.to_string());

    let mut next: Option<String> = conn
        .query_row(
            "SELECT parent_card_id FROM card WHERE id = ?1",
            params![card_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();

    while let Some(id) = next {
        if chain.len() >= limit || !seen.insert(id.clone()) {
            break;
        }
        let row = conn
            .query_row(
                "SELECT id, question, answer, depth, confidence, updated_at, parent_card_id, status
                 FROM card WHERE id = ?1",
                params![&id],
                |r| {
                    Ok((
                        Ancestor {
                            card_id: r.get(0)?,
                            question: r.get(1)?,
                            answer: r.get(2)?,
                            depth: r.get(3)?,
                            confidence: r.get(4)?,
                            // An unanswered card has no answered_at to give, and
                            // a timestamp on a card that never answered would
                            // read as freshness it does not have.
                            answered_at: match r.get::<_, String>(7)?.as_str() {
                                "done" | "flagged" => Some(r.get(5)?),
                                _ => None,
                            },
                            citations: Vec::new(),
                        },
                        r.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?;

        let Some((mut ancestor, parent)) = row else { break };
        ancestor.citations = read_citations(store, &ancestor.card_id)?;
        chain.push(ancestor);
        next = parent;
    }

    Ok(chain)
}
