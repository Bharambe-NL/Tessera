//! The profile row, the pack it points at, and the counts a person can check.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::error::Result;
use crate::{Store, new_id, now_iso8601};

/// Ensure a profile exists and return its id. First run, doc 11 section 6.
pub fn ensure_profile(store: &Store, pack_id: &str, default_depth: &str, policy: &Value) -> Result<String> {
    if let Some(id) = store
        .conn()
        .query_row("SELECT id FROM profile LIMIT 1", [], |r| r.get::<_, String>(0))
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    let now = now_iso8601();
    store.conn().execute(
        "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                              retriever_config, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '{}', ?5, ?5)",
        params![id, default_depth, pack_id, policy.to_string(), now],
    )?;
    Ok(id)
}

/// The code of the pack this profile last chose, if the row still names one.
///
/// The profile's pack is a choice that has to outlive the process. Before this
/// the core read `general` at every start, so a person who chose finance came
/// back the next morning judged by rules they had switched away from, and
/// nothing on the screen said so.
pub fn active_pack_code(store: &Store) -> Result<Option<String>> {
    Ok(store
        .conn()
        .query_row(
            "SELECT p.code FROM profile pr
               JOIN doctrine_pack p ON p.id = pr.default_doctrine_pack_id
              LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Point the profile at a pack version. Boards keep the version they pinned.
pub fn set_active_pack(store: &Store, profile_id: &str, pack_id: &str) -> Result<()> {
    store.conn().execute(
        "UPDATE profile SET default_doctrine_pack_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![pack_id, now_iso8601(), profile_id],
    )?;
    Ok(())
}

/// Register a doctrine pack version, returning its row id. A pack version is
/// inserted once; boards pin it (doc 01 section 4.17).
pub fn ensure_pack(store: &Store, pack: &Value) -> Result<String> {
    let code = pack.get("code").and_then(Value::as_str).unwrap_or("general");
    let version = pack.get("version").and_then(Value::as_str).unwrap_or("1.0.0");

    if let Some(id) = store
        .conn()
        .query_row(
            "SELECT id FROM doctrine_pack WHERE code = ?1 AND version = ?2",
            params![code, version],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }

    let id = new_id();
    let field = |name: &str| pack.get(name).cloned().unwrap_or(json!([])).to_string();
    store.conn().execute(
        "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy, freshness_classes,
                                    flag_rules, retrievers, exercise_templates, rulings, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            code,
            version,
            field("audiences"),
            field("source_hierarchy"),
            pack.get("freshness_classes")
                .cloned()
                .unwrap_or(json!({}))
                .to_string(),
            field("flag_rules"),
            field("retrievers"),
            field("exercise_templates"),
            field("rulings"),
            now_iso8601(),
        ],
    )?;
    Ok(id)
}

/// Profile, Diagnostics page. Doc 11 section 6.
///
/// Counts rather than a health verdict: what the page is for is telling a user
/// whether the thing they think happened happened, and a green tick that
/// summarises six numbers hides the one that is wrong.
pub fn profile_counts(store: &Store, profile_id: &str) -> Result<Value> {
    let conn = store.conn();
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, params![profile_id], |r| r.get(0))?) };

    Ok(json!({
        "boards": one("SELECT COUNT(*) FROM board WHERE profile_id = ?1 AND status = 'active'")?,
        "boards_trashed": one("SELECT COUNT(*) FROM board WHERE profile_id = ?1 AND status = 'trashed'")?,
        "cards": one(
            "SELECT COUNT(*) FROM card c JOIN board b ON b.id = c.board_id WHERE b.profile_id = ?1",
        )?,
        "open_flags": one(
            "SELECT COUNT(*) FROM flag f JOIN card c ON c.id = f.card_id
             JOIN board b ON b.id = c.board_id WHERE b.profile_id = ?1 AND f.status = 'open'",
        )?,
        "sources": one("SELECT COUNT(*) FROM source WHERE profile_id = ?1")?,
        "sources_stale": one("SELECT COUNT(*) FROM source WHERE profile_id = ?1 AND stale = 1")?,
        "concepts": one("SELECT COUNT(*) FROM concept WHERE profile_id = ?1")?,
        "events": conn.query_row("SELECT COUNT(*) FROM event", [], |r| r.get::<_, i64>(0))?,
    }))
}
