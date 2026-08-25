//! The Visualizer. Doc 06 part B.
//!
//! Turns the Synthesizer's `structured_summary` into one Visual with a block
//! index that binds every block to its citations.
//!
//! Doc 06 section B1: "It never reads the raw passages or the request; it reads
//! structure the Synthesizer already grounded. This is what stops a visual from
//! saying more than the prose." The packet carries no passages and no question,
//! so that property is structural rather than a rule the prompt asks for.
//!
//! Doc 06 section B10: "a card without a visual is acceptable; a visual with an
//! unsupported block is never acceptable." Every failure path here ends at type
//! `none`, never at a block nobody can trace.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_harness::{Agent, AgentContext, Failure, Recovery, sequences};
use tessera_providers::{CompletionRequest, Effort};
use tessera_schema::ids;

use crate::prompts;

pub struct Visualizer;

const SYSTEM: &str = "\
You lay out a summary that has already been written and checked. You are not \
adding to it.

Use only labels that appear in the summary you are given. You may shorten a label, \
never invent one, and never introduce an entity, a value or a relation that is not \
already there. If the summary is too thin for the chosen shape, say so in \
declined_reason and return the type none.";

#[async_trait]
impl Agent for Visualizer {
    fn id(&self) -> &str {
        "visualizer"
    }
    fn packet_schema(&self) -> &'static str {
        ids::COMMON
    }
    fn output_schema(&self) -> &'static str {
        ids::OUT_VISUALIZER
    }
    fn states(&self) -> &'static [&'static str] {
        sequences::VISUALIZER
    }
    fn completion_event(&self) -> Option<&'static str> {
        None // The pipeline emits visual.produced.v1 with the row write.
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure> {
        let summary = &packet["structured_summary"];
        let hint = packet["visual_hint"].as_str().unwrap_or("none");

        step(ctx, "selecting_type")?;
        let Some(visual_type) = select_type(summary, hint) else {
            return Ok(declined(ctx, "The summary carries too little structure to draw."));
        };

        step(ctx, "composing")?;
        let composed = compose(ctx, packet, summary, visual_type).await?;

        step(ctx, "indexing_blocks")?;
        let indexed = match index_blocks(&composed, visual_type, summary) {
            Ok(i) => i,
            // Doc 06 section B10 `untraceable_labels`: retry naming them, then
            // drop. The retry is the harness's, so a second failure lands here.
            Err(untraceable) => {
                if ctx.machine.retries_used() == 0 && ctx.machine.retry().is_ok() {
                    return Err(Failure {
                        kind: "untraceable_labels".into(),
                        detail: format!("labels not present in the summary: {}", untraceable.join(", ")),
                        recovery: Recovery::Retried,
                        evidence: Some(json!({ "untraceable": untraceable })),
                        recoverable: true,
                    });
                }
                return Ok(declined(
                    ctx,
                    "Some labels could not be traced back to the answer, so the diagram was dropped.",
                ));
            }
        };

        step(ctx, "sanitising")?;
        let payload = match visual_type {
            // Doc 06 section B8 point 4. Figures only.
            "figure" => match sanitise_svg(&indexed.payload) {
                Some(clean) => clean,
                None => return Ok(declined(ctx, "The drawing did not survive sanitisation.")),
            },
            _ => indexed.payload,
        };

        step(ctx, "emitting")?;
        step(ctx, "done")?;

        Ok(json!({
            "schema_version": "1.0",
            "agent_id": "visualizer",
            "run_id": ctx.run_id,
            "type": visual_type,
            "title": composed["title"].as_str().unwrap_or("Summary"),
            "payload": payload,
            "block_index": indexed.blocks,
            "declined_reason": Value::Null,
            "confidence": indexed.confidence,
            "caveats": [],
        }))
    }
}

fn step(ctx: &mut AgentContext<'_>, state: &str) -> Result<(), Failure> {
    ctx.machine
        .advance_to(state)
        .map(|_| ())
        .map_err(|e| Failure::new("state_machine", e.to_string(), Recovery::Failed))
}

/// A card without a visual is acceptable. Doc 06 section B10.
fn declined(ctx: &AgentContext<'_>, reason: &str) -> Value {
    json!({
        "schema_version": "1.0",
        "agent_id": "visualizer",
        "run_id": ctx.run_id,
        "type": "none",
        "title": "",
        "payload": {},
        "block_index": [],
        "declined_reason": reason,
        "confidence": 0.0,
        "caveats": []
    })
}

/// Doc 06 section B8 point 1. Deterministic, in this order, with the hint used
/// only when nothing in the summary decides.
fn select_type(summary: &Value, hint: &str) -> Option<&'static str> {
    let len = |k: &str| {
        summary
            .get(k)
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0)
    };

    if len("relations") >= 2 && forms_hierarchy(summary) {
        return Some("tree");
    }
    if len("values") >= 2 {
        return Some("table");
    }
    if len("steps") >= 2 {
        return Some("steps");
    }
    if len("groups") >= 1 {
        return Some("list");
    }
    if len("relations") >= 1 {
        return Some("tree");
    }

    match hint {
        "tree" | "table" | "list" | "steps" | "figure" => {
            // The hint only stands if there is something to put in it.
            if len("entities") >= 2 {
                Some(if hint == "figure" { "list" } else { leak(hint) })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn leak(hint: &str) -> &'static str {
    match hint {
        "tree" => "tree",
        "table" => "table",
        "steps" => "steps",
        _ => "list",
    }
}

/// A hierarchy is a relation set where some node is never a target.
fn forms_hierarchy(summary: &Value) -> bool {
    let Some(relations) = summary.get("relations").and_then(Value::as_array) else {
        return false;
    };
    let targets: std::collections::BTreeSet<&str> =
        relations.iter().filter_map(|r| r["to"].as_str()).collect();
    relations
        .iter()
        .filter_map(|r| r["from"].as_str())
        .any(|from| !targets.contains(from))
}

async fn compose(
    ctx: &mut AgentContext<'_>,
    packet: &Value,
    summary: &Value,
    visual_type: &str,
) -> Result<Value, Failure> {
    let doctrine = &packet["doctrine"];
    let max_nodes = doctrine["max_nodes"].as_u64().unwrap_or(18);
    let max_rows = doctrine["max_rows"].as_u64().unwrap_or(8);

    let prompt = format!(
        "Lay this summary out as a {visual_type}.\n\nSummary:\n{}\n\n\
At most {max_nodes} nodes and at most {max_rows} rows. Labels under five words, \
notes under fifteen.",
        serde_json::to_string_pretty(summary).unwrap_or_else(|_| summary.to_string())
    );

    let schema = payload_schema(visual_type);
    let mut system = format!(
        "{SYSTEM}\n\n{}\n\n{}",
        prompts::HOUSE_STYLE,
        prompts::json_only(&schema)
    );
    if let Some(notice) = ctx.violation_notice() {
        system.push_str("\n\n");
        system.push_str(&notice);
    }

    let completion = ctx
        .call(
            &CompletionRequest::new(ctx.model_for("visualize"), "visualize")
                .system(system)
                .user(prompt)
                .effort(Effort::High)
                .max_tokens(2000)
                .expecting(schema),
        )
        .await?;

    completion.json().map_err(|e| Failure {
        kind: "schema_violation".into(),
        detail: e.to_string(),
        recovery: Recovery::Retried,
        evidence: None,
        recoverable: true,
    })
}

fn payload_schema(visual_type: &str) -> Value {
    let node = json!({
        "type": "object",
        "required": ["label"],
        "additionalProperties": false,
        "properties": {
            "label": { "type": "string" },
            "note": { "type": "string" },
            "children": { "type": "array", "items": {
                "type": "object",
                "required": ["label"],
                "additionalProperties": false,
                "properties": { "label": { "type": "string" }, "note": { "type": "string" } }
            }}
        }
    });

    let payload = match visual_type {
        "tree" => json!({ "type": "object", "required": ["root"], "additionalProperties": false,
                          "properties": { "root": node } }),
        "table" => json!({
            "type": "object", "required": ["columns", "rows"], "additionalProperties": false,
            "properties": {
                "columns": { "type": "array", "items": { "type": "string" } },
                "rows": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } },
                "bottom_line": { "type": "object", "required": ["head", "text"], "additionalProperties": false,
                                 "properties": { "head": { "type": "string" }, "text": { "type": "string" } } }
            }
        }),
        "steps" => json!({
            "type": "object", "required": ["steps"], "additionalProperties": false,
            "properties": { "steps": { "type": "array", "items": {
                "type": "object", "required": ["label"], "additionalProperties": false,
                "properties": { "label": { "type": "string" }, "note": { "type": "string" } }
            }}}
        }),
        _ => json!({
            "type": "object", "required": ["groups"], "additionalProperties": false,
            "properties": { "groups": { "type": "array", "items": {
                "type": "object", "required": ["heading", "items"], "additionalProperties": false,
                "properties": {
                    "heading": { "type": "string" },
                    "items": { "type": "array", "items": {
                        "type": "object", "required": ["name"], "additionalProperties": false,
                        "properties": { "name": { "type": "string" }, "detail": { "type": "string" } }
                    }}
                }
            }}}
        }),
    };

    json!({
        "type": "object",
        "required": ["title", "payload"],
        "additionalProperties": false,
        "properties": { "title": { "type": "string" }, "payload": payload }
    })
}

#[derive(Debug)]
struct Indexed {
    payload: Value,
    blocks: Value,
    confidence: f64,
}

/// Doc 06 section B8 point 3. A deterministic walk of the payload building
/// pointers and copying citations from the summary entry each label came from.
///
/// Returns the untraceable labels on failure, so the retry prompt can name them.
fn index_blocks(composed: &Value, visual_type: &str, summary: &Value) -> Result<Indexed, Vec<String>> {
    let payload = composed["payload"].clone();
    let lookup = SummaryIndex::build(summary);

    let mut blocks = Vec::new();
    let mut untraceable = Vec::new();

    let mut add = |ref_path: String, label: &str, structural: bool, blocks: &mut Vec<Value>| {
        match lookup.citations_for(label) {
            Some(ordinals) => blocks.push(json!({
                "ref": ref_path, "label": label, "citation_ordinals": ordinals
            })),
            None if structural => {
                // Doc 07 section B8.3 limits no_claim to structural labels: a
                // group heading, a column name.
                blocks.push(json!({
                    "ref": ref_path, "label": label, "citation_ordinals": [], "no_claim": true
                }));
            }
            None if lookup.knows(label) => blocks.push(json!({
                "ref": ref_path, "label": label, "citation_ordinals": []
            })),
            None => untraceable.push(label.to_string()),
        }
    };

    match visual_type {
        "tree" => {
            if let Some(root) = payload.get("root") {
                let label = root["label"].as_str().unwrap_or_default();
                add("/root".into(), label, false, &mut blocks);
                for (i, child) in root["children"].as_array().into_iter().flatten().enumerate() {
                    let label = child["label"].as_str().unwrap_or_default();
                    add(format!("/root/children/{i}"), label, false, &mut blocks);
                    for (j, gc) in child["children"].as_array().into_iter().flatten().enumerate() {
                        let label = gc["label"].as_str().unwrap_or_default();
                        add(
                            format!("/root/children/{i}/children/{j}"),
                            label,
                            false,
                            &mut blocks,
                        );
                    }
                }
            }
        }
        "table" => {
            for (i, col) in payload["columns"].as_array().into_iter().flatten().enumerate() {
                add(
                    format!("/columns/{i}"),
                    col.as_str().unwrap_or_default(),
                    true,
                    &mut blocks,
                );
            }
            for (r, row) in payload["rows"].as_array().into_iter().flatten().enumerate() {
                for (c, cell) in row.as_array().into_iter().flatten().enumerate() {
                    add(
                        format!("/rows/{r}/{c}"),
                        cell.as_str().unwrap_or_default(),
                        false,
                        &mut blocks,
                    );
                }
            }
        }
        "steps" => {
            for (i, s) in payload["steps"].as_array().into_iter().flatten().enumerate() {
                add(
                    format!("/steps/{i}"),
                    s["label"].as_str().unwrap_or_default(),
                    false,
                    &mut blocks,
                );
            }
        }
        _ => {
            for (g, group) in payload["groups"].as_array().into_iter().flatten().enumerate() {
                add(
                    format!("/groups/{g}/heading"),
                    group["heading"].as_str().unwrap_or_default(),
                    true,
                    &mut blocks,
                );
                for (i, item) in group["items"].as_array().into_iter().flatten().enumerate() {
                    add(
                        format!("/groups/{g}/items/{i}"),
                        item["name"].as_str().unwrap_or_default(),
                        false,
                        &mut blocks,
                    );
                }
            }
        }
    }

    if !untraceable.is_empty() {
        untraceable.sort();
        untraceable.dedup();
        return Err(untraceable);
    }

    // Doc 06 section B9.
    let total = blocks.len().max(1) as f64;
    let cited = blocks
        .iter()
        .filter(|b| b["citation_ordinals"].as_array().is_some_and(|c| !c.is_empty()))
        .count() as f64;
    let confidence = ((cited / total) * 0.6 + 0.2 + 0.2 * 100.0 / 100.0).min(1.0);

    Ok(Indexed {
        payload,
        blocks: json!(blocks),
        confidence: (confidence * 100.0).round() / 100.0,
    })
}

/// Every label the summary contains, and the citation behind it.
struct SummaryIndex {
    by_label: std::collections::BTreeMap<String, Vec<usize>>,
}

impl SummaryIndex {
    fn build(summary: &Value) -> Self {
        let mut by_label: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        let mut note = |label: &str, ordinal: Option<usize>| {
            let entry = by_label.entry(normalise(label)).or_default();
            if let Some(o) = ordinal
                && !entry.contains(&o)
            {
                entry.push(o);
            }
        };

        for e in summary["entities"].as_array().into_iter().flatten() {
            if let Some(s) = e.as_str() {
                note(s, None);
            }
        }
        for r in summary["relations"].as_array().into_iter().flatten() {
            for key in ["from", "to", "kind"] {
                if let Some(s) = r[key].as_str() {
                    note(s, None);
                }
            }
        }
        for v in summary["values"].as_array().into_iter().flatten() {
            let ordinal = v["citation"].as_u64().map(|n| n as usize);
            if let Some(s) = v["label"].as_str() {
                note(s, ordinal);
            }
            if let Some(s) = v["value"].as_str() {
                note(s, ordinal);
            }
            // A value often renders as "2.5 %", so index that form too.
            if let (Some(value), Some(unit)) = (v["value"].as_str(), v["unit"].as_str()) {
                note(&format!("{value} {unit}"), ordinal);
                note(&format!("{value}{unit}"), ordinal);
            }
        }
        for s in summary["steps"].as_array().into_iter().flatten() {
            if let Some(s) = s.as_str() {
                note(s, None);
            }
        }
        for g in summary["groups"].as_array().into_iter().flatten() {
            if let Some(h) = g["heading"].as_str() {
                note(h, None);
            }
            for item in g["items"].as_array().into_iter().flatten() {
                if let Some(s) = item.as_str() {
                    note(s, None);
                }
            }
        }

        Self { by_label }
    }

    fn knows(&self, label: &str) -> bool {
        let n = normalise(label);
        self.by_label.contains_key(&n) || self.by_label.keys().any(|k| k.contains(&n) || n.contains(k))
    }

    /// Non empty citations for a label, if the summary carried any.
    fn citations_for(&self, label: &str) -> Option<Vec<usize>> {
        let n = normalise(label);
        if let Some(c) = self.by_label.get(&n)
            && !c.is_empty()
        {
            return Some(c.clone());
        }
        // A shortened label still traces back: doc 06 section B8 allows
        // shortening, never adding.
        for (key, citations) in &self.by_label {
            if citations.is_empty() {
                continue;
            }
            if key.contains(&n) || n.contains(key) {
                return Some(citations.clone());
            }
        }
        None
    }
}

fn normalise(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Doc 06 section B8 point 4 and doc 01 section 4.3.1: an allowlist of svg
/// elements and attributes, strip everything else, reject if the result is
/// empty. Sanitisation is a Step with its own event.
pub fn sanitise_svg(payload: &Value) -> Option<Value> {
    let svg = payload["svg"].as_str()?;
    let cleaned = strip_svg(svg);
    if cleaned.trim().is_empty() || !cleaned.contains("<svg") {
        return None;
    }
    let mut out = payload.clone();
    out["svg"] = json!(cleaned);
    Some(out)
}

fn strip_svg(svg: &str) -> String {
    let mut s = svg.to_string();
    for (open, close) in [
        ("<script", "</script>"),
        ("<foreignObject", "</foreignObject>"),
        ("<style", "</style>"),
    ] {
        while let Some(start) = s.to_lowercase().find(&open.to_lowercase()) {
            let end = s.to_lowercase()[start..]
                .find(&close.to_lowercase())
                .map(|e| start + e + close.len())
                .unwrap_or(s.len());
            s.replace_range(start..end, "");
        }
    }
    // Event handlers and external references.
    let handlers = regex::Regex::new(r#"(?i)\s+on\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#);
    if let Ok(re) = handlers {
        s = re.replace_all(&s, "").to_string();
    }
    let external = regex::Regex::new(r#"(?i)\s+(href|xlink:href|src)\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)"#);
    if let Ok(re) = external {
        s = re.replace_all(&s, "").to_string();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hierarchy_of_relations_becomes_a_tree() {
        let summary = json!({
            "relations": [
                { "from": "World model", "to": "Perception", "kind": "has" },
                { "from": "World model", "to": "Dynamics", "kind": "has" }
            ]
        });
        assert_eq!(select_type(&summary, "none"), Some("tree"));
    }

    #[test]
    fn two_values_become_a_table() {
        let summary = json!({ "values": [
            { "label": "old", "value": "8" }, { "label": "new", "value": "10" }
        ]});
        assert_eq!(select_type(&summary, "none"), Some("table"));
    }

    #[test]
    fn an_empty_summary_declines_rather_than_inventing_a_shape() {
        // Doc 06 section B10: a card without a visual is acceptable.
        assert_eq!(select_type(&json!({}), "tree"), None);
        assert_eq!(select_type(&json!({ "entities": ["one"] }), "table"), None);
    }

    #[test]
    fn a_label_absent_from_the_summary_is_untraceable() {
        // Doc 06 section B8 point 3: the composition is retried naming them, and
        // a second failure drops the visual. This is what stops a diagram from
        // saying more than the prose.
        let summary = json!({ "values": [{ "label": "buffer", "value": "2.5", "citation": 1 }] });
        let composed = json!({
            "title": "Thresholds",
            "payload": { "columns": ["Rule", "Value"], "rows": [["buffer", "2.5"], ["leverage floor", "3"]] }
        });
        let err = index_blocks(&composed, "table", &summary).expect_err("must not pass");
        assert!(err.iter().any(|l| l == "leverage floor"), "got {err:?}");
    }

    #[test]
    fn a_cited_value_carries_its_citation_into_the_block() {
        let summary =
            json!({ "values": [{ "label": "buffer", "value": "2.5", "unit": "%", "citation": 3 }] });
        let composed = json!({
            "title": "Buffer",
            "payload": { "columns": ["Rule", "Value"], "rows": [["buffer", "2.5 %"]] }
        });
        let indexed = index_blocks(&composed, "table", &summary).expect("indexes");
        let blocks = indexed.blocks.as_array().expect("blocks");

        let cell = blocks
            .iter()
            .find(|b| b["ref"] == "/rows/0/1")
            .expect("the value cell");
        assert_eq!(cell["citation_ordinals"], json!([3]));
    }

    #[test]
    fn a_column_heading_is_structural_and_may_carry_no_claim() {
        // Doc 07 section B8.3 limits no_claim to structural labels.
        let summary = json!({ "values": [{ "label": "buffer", "value": "2.5", "citation": 1 }] });
        let composed = json!({
            "title": "T",
            "payload": { "columns": ["Rule", "Value"], "rows": [["buffer", "2.5"]] }
        });
        let indexed = index_blocks(&composed, "table", &summary).expect("indexes");
        let blocks = indexed.blocks.as_array().expect("blocks");
        let heading = blocks.iter().find(|b| b["ref"] == "/columns/0").expect("heading");
        assert_eq!(heading["no_claim"], true);
    }

    #[test]
    fn every_block_carries_a_json_pointer() {
        let summary = json!({ "steps": ["Encode", "Predict", "Plan"] });
        let composed = json!({
            "title": "Loop",
            "payload": { "steps": [{ "label": "Encode" }, { "label": "Predict" }, { "label": "Plan" }] }
        });
        let indexed = index_blocks(&composed, "steps", &summary).expect("indexes");
        for b in indexed.blocks.as_array().expect("blocks") {
            let r = b["ref"].as_str().expect("ref");
            assert!(r.starts_with('/'), "`{r}` is not a JSON pointer");
        }
    }

    #[test]
    fn a_script_never_survives_sanitisation() {
        // Doc 01 section 4.3.1: no script, no foreignObject, no external refs.
        let payload = json!({
            "svg": r#"<svg viewBox="0 0 10 10"><script>fetch('x')</script><path d="M0 0" onclick="x()"/><image href="http://evil.example/a.png"/></svg>"#
        });
        let clean = sanitise_svg(&payload).expect("survives with content");
        let svg = clean["svg"].as_str().expect("svg");
        assert!(!svg.contains("<script"));
        assert!(!svg.contains("onclick"));
        assert!(!svg.contains("http://evil.example"));
        assert!(svg.contains("<path"), "the drawing itself survives");
    }

    #[test]
    fn an_svg_that_is_only_script_is_rejected_rather_than_emptied() {
        let payload = json!({ "svg": "<script>alert(1)</script>" });
        assert!(sanitise_svg(&payload).is_none());
    }
}
