//! Sources: the key that makes two retrievals one row, and the library listing.

use rusqlite::params;
use serde_json::{Value, json};

use crate::Store;
use crate::error::Result;

/// Doc 01 section 4.7: the dedupe key is a normalised locator.
/// The key that makes two retrievals of one thing a single Source.
///
/// Doc 01 section 4.8 keys `source` uniqueness on this and doc 05 section 12
/// wants zero duplicate sources for mirrored pages. Public and living here on
/// purpose: this crate owns the uniqueness constraint, and a second copy of the
/// rule in the retrievers would drift from it the first time either changed.
///
/// Scheme, case, a leading `www.`, a trailing slash, a query string and a
/// fragment are all noise. A tracking parameter is the common way one page
/// arrives four times.
pub fn normalise_locator(locator: &str) -> String {
    let lower = locator.trim().to_lowercase().replace('\\', "/");
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let without_www = without_scheme.strip_prefix("www.").unwrap_or(without_scheme);
    let without_query = without_www.split(['?', '#']).next().unwrap_or(without_www);
    without_query.trim_end_matches('/').to_string()
}

/// Library, Sources tab. Doc 09 section 9.
///
/// "title, issuer, class, trust rank, cited on n cards, last verified, stale
/// state". The card count is the column that decides whether a source can be
/// removed: doc 09 section 5 allows Remove on a source only if it is uncited.
pub fn list_sources(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.class, s.site_or_issuer, s.locator, s.trust_rank,
                s.last_verified_at, s.stale, s.stale_reason, s.freshness_class, s.version_ref,
                -- A citation names a passage, and a passage names its source,
                -- so the count of cards citing a source is two joins rather
                -- than a column the citation does not carry.
                (SELECT COUNT(DISTINCT c.card_id) FROM citation c
                 JOIN passage pg ON pg.id = c.passage_id
                 WHERE pg.source_id = s.id) AS cards
         FROM source s
         WHERE s.profile_id = ?1
         ORDER BY s.stale DESC, s.trust_rank, s.title
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "class": r.get::<_, String>(2)?,
                "issuer": r.get::<_, Option<String>>(3)?,
                "locator": r.get::<_, String>(4)?,
                "trust_rank": r.get::<_, i64>(5)?,
                "last_verified_at": r.get::<_, Option<String>>(6)?,
                "stale": r.get::<_, i64>(7)? == 1,
                "stale_reason": r.get::<_, Option<String>>(8)?,
                "freshness_class": r.get::<_, String>(9)?,
                "version_ref": r.get::<_, Option<String>>(10)?,
                "cards": r.get::<_, i64>(11)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
