//! The reads more than one of the files beside this one needs.
//!
//! A row to json helper and the two per card queries a card view and an
//! ancestry walk both run. They stay private to the repository.

use rusqlite::params;
use serde_json::{Value, json};

use crate::Store;
use crate::error::Result;

pub(super) fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

pub(super) fn read_citations(store: &Store, card_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.ordinal, s.title, s.class, s.locator, c.verifier_verdict, s.stale, p.text,
                c.claim_span, c.binding, c.passage_id
         FROM citation c JOIN passage p ON p.id = c.passage_id JOIN source s ON s.id = p.source_id
         WHERE c.card_id = ?1 ORDER BY c.ordinal ASC",
    )?;
    Ok(stmt
        .query_map(params![card_id], |r| {
            Ok(json!({
                "ordinal": r.get::<_, i64>(0)?,
                // Doc 16 section 3.2's carried citation is `{ordinal,
                // passage_id}`, and the passage id was missing here from the
                // day the column was added: `save_card_as_page` read it,
                // found nothing, and wrote an empty string. Every page saved
                // since carried the right number of citations pointing at no
                // evidence, which is the one thing that rule exists to stop.
                "passage_id": r.get::<_, String>(9)?,
                "source_title": r.get::<_, String>(1)?,
                "source_class": r.get::<_, String>(2)?,
                "locator": r.get::<_, String>(3)?,
                "verdict": r.get::<_, String>(4)?,
                "stale": r.get::<_, i64>(5)? != 0,
                // Doc 02 section 10.2 reports citation accuracy per Verifier
                // verdict *and* per ledger check, and the ledger check has to
                // ask the same question the Verifier did. Without the passage
                // the scorer could only ask a different one and call the gap
                // disagreement.
                "passage_text": r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                // And the other half of that question. The Verifier judges a
                // passage against the claim span it was bound to; a ledger check
                // over every citation instead asks whether each one states the
                // answer to the question, which most correct citations on a deep
                // card do not. BN-110: 0.365 was that difference, not the
                // product's citation accuracy.
                "claim_span": parse_json(&r.get::<_, String>(7)?),
                "binding": r.get::<_, String>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

pub(super) fn read_flags(store: &Store, card_id: &str) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, rule_id, severity, reason FROM flag
         WHERE card_id = ?1 AND status = 'open' ORDER BY created_at ASC",
    )?;
    Ok(stmt
        .query_map(params![card_id], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "rule_id": r.get::<_, String>(1)?,
                "severity": r.get::<_, String>(2)?,
                "reason": r.get::<_, String>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
