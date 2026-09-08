//! A card read back for a run that re-verifies rather than answers.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::sql::parse_json;
use crate::Store;
use crate::error::Result;

/// A card read back for re-verification, with the current state of the sources
/// it cited. Doc 07 section B8.4's freshness check runs against this.
#[derive(Debug, Clone)]
pub struct CardForVerify {
    pub depth: String,
    pub answer: Option<String>,
    pub findings: Value,
    /// In the shape an agent packet uses, keyed `n` rather than `ordinal`, so
    /// the Verifier reads a re-verified card's citations exactly as it reads a
    /// freshly synthesised one.
    pub citations: Vec<Value>,
    /// One per citation, in the shape the Verifier's packet expects, carrying
    /// what the source looks like now rather than when the card was written.
    pub passages: Vec<Value>,
}

/// Read a card and its citations back, for a run that re-verifies rather than
/// answers. Doc 07 section B3.
///
/// The passages carry `stale` and `stale_reason` from the source rows as they
/// stand now, which is the whole point: a card written months ago is judged
/// against what its sources have since become.
pub fn read_card_for_verify(store: &Store, card_id: &str) -> Result<Option<CardForVerify>> {
    let conn = store.conn();
    let row: Option<(String, Option<String>, String)> = conn
        .query_row(
            "SELECT depth, answer, findings FROM card WHERE id = ?1",
            params![card_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;
    let Some((depth, answer, findings)) = row else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT c.ordinal, c.passage_id, p.text, s.title, s.class, s.locator, s.site_or_issuer,
                s.trust_rank, s.published_at, s.version_ref, s.stale, s.stale_reason, c.claim_span,
                c.binding
           FROM citation c
           JOIN passage p ON p.id = c.passage_id
           JOIN source s ON s.id = p.source_id
          WHERE c.card_id = ?1 ORDER BY c.ordinal ASC",
    )?;
    let rows: Vec<(Value, Value)> = stmt
        .query_map(params![card_id], |r| {
            let stale: i64 = r.get(10)?;
            let passage_id: String = r.get(1)?;
            let citation = json!({
                "n": r.get::<_, i64>(0)?,
                "passage_id": passage_id,
                "claim_span": parse_json(&r.get::<_, String>(12)?),
                "binding": r.get::<_, String>(13)?,
            });
            let passage = json!({
                "passage_id": r.get::<_, String>(1)?,
                "text": r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                "source": {
                    "title": r.get::<_, String>(3)?,
                    "class": r.get::<_, String>(4)?,
                    "locator": r.get::<_, String>(5)?,
                    "issuer": r.get::<_, Option<String>>(6)?,
                    "trust_rank": r.get::<_, i64>(7)?,
                    "published_at": r.get::<_, Option<String>>(8)?,
                    "version_ref": r.get::<_, Option<String>>(9)?,
                    "stale": stale != 0,
                    "stale_reason": r.get::<_, Option<String>>(11)?,
                },
            });
            Ok((citation, passage))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let (citations, passages) = rows.into_iter().unzip();
    Ok(Some(CardForVerify {
        depth,
        answer,
        findings: parse_json(&findings),
        citations,
        passages,
    }))
}
