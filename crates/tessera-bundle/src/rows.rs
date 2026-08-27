//! Reading and writing whole table rows as JSON.
//!
//! A bundle carries thirteen kinds of row. Hand-writing a struct for each would
//! mean thirteen places for a schema addition to be silently dropped, so rows
//! travel as objects keyed by column name and are written back the same way. A
//! column added to the store appears in the next bundle without a code change,
//! and a column an older bundle does not carry keeps its database default.
//!
//! The cost is that the compiler no longer checks field names, so the two
//! places that do care about a particular column, the export redactions and the
//! import merges, name it as a string and are tested for it.

use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{Connection, Row};
use serde_json::{Map, Value, json};

use crate::Result;

/// One row as `{column: value}`, with SQLite's types mapped to JSON's.
pub fn row_to_json(row: &Row<'_>) -> rusqlite::Result<Value> {
    let names: Vec<String> = row
        .as_ref()
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut out = Map::new();
    for (i, name) in names.iter().enumerate() {
        let value = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => json!(n),
            ValueRef::Real(f) => json!(f),
            ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
            // No bundle column is a blob today. Rendering one as its length
            // rather than dropping it means a future one shows up as wrong
            // rather than as absent.
            ValueRef::Blob(b) => json!({ "bytes": b.len() }),
        };
        out.insert(name.clone(), value);
    }
    Ok(Value::Object(out))
}

/// Every row a query returns, as JSON objects.
pub fn query(conn: &Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args, row_to_json)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Insert one JSON row into `table`, skipping columns the table does not have.
///
/// Returns false when a row with this primary key is already there. Doc 01
/// section 7: import never overwrites, so an id that exists is a row this
/// profile already has and the caller counts it as skipped rather than failing.
/// That is what makes importing the same bundle twice a no-op instead of an
/// error, which matters because the second import is usually an accident.
pub fn insert(conn: &Connection, table: &str, key: &str, row: &Value) -> Result<bool> {
    let Some(object) = row.as_object() else {
        return Ok(false);
    };
    let Some(id) = object.get(key).and_then(Value::as_str) else {
        return Ok(false);
    };

    let existing: i64 = conn.query_row(
        // Table and column names come from this crate and never from the
        // archive, so nothing a bundle carries reaches the statement text.
        &format!("SELECT COUNT(*) FROM {table} WHERE {key} = ?1"),
        [id],
        |r| r.get(0),
    )?;
    if existing > 0 {
        return Ok(false);
    }

    let known = columns(conn, table)?;
    let mut names: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    for (column, value) in object {
        if !known.contains(column) {
            continue;
        }
        names.push(column);
        values.push(to_sql(value));
    }

    let placeholders: Vec<String> = (1..=names.len()).map(|i| format!("?{i}")).collect();
    conn.execute(
        &format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            names.join(", "),
            placeholders.join(", ")
        ),
        rusqlite::params_from_iter(values.iter()),
    )?;
    Ok(true)
}

/// The columns a table actually has, so a bundle from a later build drops the
/// fields this one does not know rather than failing the whole import.
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    let mut out = Vec::new();
    for name in names {
        out.push(name?);
    }
    Ok(out)
}

/// JSON back to a SQLite value.
///
/// An object or an array becomes its text, because every json shaped column in
/// the store is declared TEXT and holds serialised json. A float that is really
/// an integer stays an integer, because a STRICT table rejects the wrong one.
fn to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| n.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}
