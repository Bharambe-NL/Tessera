//! The diagnostics export. Doc 10 section 11.
//!
//! "Export diagnostics produces a zip with logs and the last N runs' events
//! with prompt text redacted. No remote reporting."
//!
//! This file exists to be sent to someone else, which makes it the third thing
//! that leaves the profile folder and the one with the least obvious contents.
//! A bundle is a board a person chose to share; a backup goes to their own
//! disk. A diagnostics zip is handed to a stranger debugging a crash, and what
//! it must not carry is everything the other two carry on purpose.
//!
//! So the rule here is the opposite of the bundle's. A bundle names what it
//! includes; this names what survives, and everything else is dropped. A field
//! added to the store tomorrow appears in the next bundle by design and does
//! not appear here, which is the right way round for a file whose whole job is
//! to be safe to send.

use std::collections::BTreeSet;
use std::io::{Seek, Write};

use serde_json::{Value, json};
use tessera_store::Store;
use zip::write::SimpleFileOptions;

use crate::Result;
use crate::rows::query;

/// How many runs a diagnostics export carries. Doc 10 section 11's "last N".
///
/// Fifty rather than everything, because the useful question is what happened
/// recently and a whole history makes the file large enough that people stop
/// sending it.
pub const RECENT_RUNS: i64 = 50;

/// Event payload keys that carry text a person wrote or a document said.
///
/// Named rather than inferred. A rule that dropped every long string would keep
/// whatever happened to be short, and the point of this list is that adding a
/// field to an event does not quietly add it to a file sent to a stranger.
const CARRIES_CONTENT: [&str; 12] = [
    "answer",
    "question",
    "text",
    "topic",
    "reason",
    "evidence",
    "passage_text",
    "prompt",
    "message",
    "title",
    "excerpt",
    "findings",
];

/// What the export carries, so the person sending it can see it first.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiagnosticsSummary {
    pub taken_at: String,
    pub runs: usize,
    pub events: usize,
    pub steps: usize,
    /// Payload fields dropped, by key, so a reader can see what is missing and
    /// ask for it another way rather than concluding it was never there.
    pub redacted: serde_json::Map<String, Value>,
}

/// Write a diagnostics zip for `store` to `sink`.
pub fn diagnostics<W: Write + Seek>(store: &Store, sink: W) -> Result<DiagnosticsSummary> {
    let conn = store.conn();

    // Doc 10 section 11's diagnostics page, as data: runs, failures by type,
    // latency percentiles, spend by provider.
    let runs = query(
        conn,
        "SELECT id, board_id, card_id, kind, depth, status, started_at, ended_at, cost,
                doctrine_pack_version
         FROM run ORDER BY started_at DESC LIMIT ?1",
        &[&RECENT_RUNS],
    )?;
    let recent: BTreeSet<String> = runs
        .iter()
        .filter_map(|r| r["id"].as_str().map(str::to_string))
        .collect();

    // Steps without their task packets. Doc 01 section 7 already treats
    // `Step.task_packet` as the field that carries prompt text, and this is the
    // same field for the same reason.
    let steps: Vec<Value> = query(
        conn,
        "SELECT id, run_id, agent_id, sequence, model_call, status, failure, started_at, ended_at
         FROM step ORDER BY started_at DESC LIMIT ?1",
        &[&(RECENT_RUNS * 12)],
    )?
    .into_iter()
    .filter(|s| s["run_id"].as_str().is_some_and(|id| recent.contains(id)))
    .collect();

    let mut redacted = serde_json::Map::new();
    let events: Vec<Value> = query(
        conn,
        "SELECT event_id, monotonic_index, event_type, payload, source, emitter_id, emitter_type,
                run_id, board_id, card_id, trust_level, timestamp
         FROM event ORDER BY monotonic_index DESC LIMIT ?1",
        &[&(RECENT_RUNS * 40)],
    )?
    .into_iter()
    .map(|mut e| {
        e["payload"] = redact_payload(&parsed(&e["payload"]), &mut redacted);
        e
    })
    .collect();

    let summary = DiagnosticsSummary {
        taken_at: tessera_store::now_iso8601(),
        runs: runs.len(),
        events: events.len(),
        steps: steps.len(),
        redacted,
    };

    let mut zip = zip::ZipWriter::new(sink);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("summary.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&summary)?.as_bytes())?;

    zip.start_file("health.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&health(store, &runs))?.as_bytes())?;

    for (name, rows) in [
        ("runs.jsonl", &runs),
        ("steps.jsonl", &steps),
        ("events.jsonl", &events),
    ] {
        zip.start_file(name, options)?;
        for row in rows {
            zip.write_all(serde_json::to_string(row)?.as_bytes())?;
            zip.write_all(b"\n")?;
        }
    }

    zip.finish()?;
    Ok(summary)
}

/// A json column read back as a string, parsed into the object it holds.
///
/// `payload` is declared TEXT and holds serialised json, so a row read comes
/// back as one long string. The first version of this redacted the value it was
/// given, matched no keys because there were none to match, and wrote the whole
/// payload into the export untouched: every answer, every passage, every reason
/// a flag gave. The export was safe against exactly the shape it never saw.
///
/// A string that is not json stays a string, which is what an event payload
/// would have to be for this to matter, and there are none.
fn parsed(value: &Value) -> Value {
    match value.as_str() {
        Some(text) => serde_json::from_str(text).unwrap_or_else(|_| value.clone()),
        None => value.clone(),
    }
}

/// Drop every key that carries content, counting what went.
///
/// Recursive, because a payload nests: a flag's evidence is an object and a
/// verify event carries a list of them. A shallow pass would clear the top
/// level and ship the passage text one layer down, which is the failure this
/// whole file exists to avoid.
fn redact_payload(payload: &Value, counts: &mut serde_json::Map<String, Value>) -> Value {
    match payload {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if CARRIES_CONTENT.contains(&key.as_str()) {
                    let seen = counts.get(key).and_then(Value::as_u64).unwrap_or(0);
                    counts.insert(key.clone(), json!(seen + 1));
                    continue;
                }
                out.insert(key.clone(), redact_payload(value, counts));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(|i| redact_payload(i, counts)).collect()),
        // A string that is itself json is descended into rather than passed
        // through. `Step.model_call` and several payload fields carry a nested
        // document, and a pass that stopped at the quote mark would ship it.
        Value::String(text) if looks_like_json(text) => match serde_json::from_str::<Value>(text) {
            Ok(inner) => redact_payload(&inner, counts),
            Err(_) => Value::String(text.clone()),
        },
        // A bare string at a key this list does not name is a status, an id or
        // an enum. Dropping those would leave a file that says nothing.
        other => other.clone(),
    }
}

/// Whether a string is worth trying to parse as json.
///
/// Cheap, and wrong only in the direction that costs a parse attempt: a string
/// that starts with a brace and is not json comes back as itself.
fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

/// The numbers doc 10 section 11's diagnostics page shows.
fn health(store: &Store, runs: &[Value]) -> Value {
    let mut latencies: Vec<i64> = Vec::new();
    let mut failures = serde_json::Map::new();
    let mut spend = serde_json::Map::new();

    for run in runs {
        if run["status"].as_str() == Some("failed") {
            let kind = run["kind"].as_str().unwrap_or("unknown").to_string();
            let seen = failures.get(&kind).and_then(Value::as_u64).unwrap_or(0);
            failures.insert(kind, json!(seen + 1));
        }
        if let (Some(started), Some(ended)) = (run["started_at"].as_str(), run["ended_at"].as_str())
            && let (Ok(a), Ok(b)) = (
                chrono::DateTime::parse_from_rfc3339(started),
                chrono::DateTime::parse_from_rfc3339(ended),
            )
        {
            latencies.push((b - a).num_milliseconds());
        }
        let cost: Value = run["cost"]
            .as_str()
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or(Value::Null);
        for (provider, used) in cost["by_provider"].as_object().into_iter().flatten() {
            let seen = spend.get(provider).and_then(Value::as_i64).unwrap_or(0);
            spend.insert(provider.clone(), json!(seen + used.as_i64().unwrap_or_default()));
        }
    }

    latencies.sort_unstable();
    let percentile = |p: f64| -> Value {
        if latencies.is_empty() {
            // BN-019's rule: a number with nothing behind it says so.
            return Value::Null;
        }
        let i = ((latencies.len() as f64 - 1.0) * p).round() as usize;
        json!(latencies[i])
    };

    let counts: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
        .unwrap_or(0);

    json!({
        "runs_examined": runs.len(),
        "failures_by_kind": failures,
        "latency_ms": { "p50": percentile(0.5), "p95": percentile(0.95) },
        "spend_by_provider": spend,
        "events_in_log": counts,
        "schema_version": tessera_store::SCHEMA_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_payload_loses_its_content_at_every_depth() {
        // The failure this guards: a shallow pass clears the top level and
        // ships the passage text one layer down, in a file whose whole job is
        // to be safe to hand to a stranger.
        let payload = json!({
            "card_id": "01M113",
            "answer": "The buffer is 2.5 %.",
            "flags": [
                { "rule_id": "stale_source", "reason": "A cited source changed.",
                  "evidence": { "passage_text": "the private document said this" } }
            ],
            "counts": { "citations": 3 }
        });
        let mut counts = serde_json::Map::new();
        let out = redact_payload(&payload, &mut counts);

        let text = out.to_string();
        assert!(!text.contains("2.5"), "{text}");
        assert!(!text.contains("private document"), "{text}");
        assert!(!text.contains("A cited source changed"), "{text}");

        // And what is left is still worth reading.
        assert_eq!(out["card_id"], "01M113");
        assert_eq!(out["flags"][0]["rule_id"], "stale_source");
        assert_eq!(out["counts"]["citations"], 3);

        // The summary says what went, so a reader asks for it rather than
        // concluding it never existed.
        assert_eq!(counts["answer"], 1);
        assert_eq!(counts["reason"], 1);
        // `evidence` goes whole, so the `passage_text` inside it is never
        // reached and never counted separately. That is the right way round:
        // doc 01 line 310 says evidence is "the passage, the number, the stale
        // date", so descending into it to pick fields would be looking for
        // reasons to keep some of a field that exists to carry content.
        assert_eq!(counts["evidence"], 1);
        assert!(!counts.contains_key("passage_text"));
    }

    #[test]
    fn a_percentile_over_nothing_is_absent_rather_than_zero() {
        // BN-019's rule, in the one place a diagnostics reader would most
        // readily believe a zero: a latency of 0 ms reads as a fast product.
        let store = Store::open_in_memory().expect("store");
        let out = health(&store, &[]);
        assert!(out["latency_ms"]["p50"].is_null());
        assert!(out["latency_ms"]["p95"].is_null());
    }
}
