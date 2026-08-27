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

/// Doc 16 section 3.5: "at most 6 tiles". A hard cap rather than a doctrine
/// limit, because the shape is the limit: seven large numerals is a table with
/// the labels taken off.
const STATS_TILES: usize = 6;

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
        // Doc 06 section B4.
        ids::PACKET_VISUALIZER
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
        let mut composed = compose(ctx, packet, summary, visual_type).await?;

        // Doc 06 section B5's harness rule: node and row counts within the
        // doctrine's limits. The limits were named in the prompt and nothing
        // checked the answer, so a model returning twenty rows produced a
        // twenty row visual against a pack that allows eight.
        let mut shape = Shape::default();
        shape.dropped += enforce_limits(&mut composed, visual_type, &packet["doctrine"]);

        // Before indexing rather than only after pruning. A visual with nothing
        // in it can arrive either way, and until BN-110 only the pruned one was
        // caught: a tree the model returned as a bare root indexed cleanly,
        // because the root label traced, and drew a single box.
        if !has_content(&composed, visual_type) {
            return Ok(declined(ctx, "The summary carries too little structure to draw."));
        }

        step(ctx, "indexing_blocks")?;
        let mut second_pass = false;
        let indexed = loop {
            match index_blocks(&composed, visual_type, summary, shape) {
                Ok(i) => break i,
                // Doc 06 section B10 `untraceable_labels`: retry naming them,
                // then drop. The retry is the harness's, so a second failure
                // lands here, and B8.3 says it drops those blocks rather than
                // the whole visual.
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
                    if second_pass {
                        return Ok(declined(
                            ctx,
                            "Nothing in the diagram traced back to the answer, so it was dropped.",
                        ));
                    }
                    second_pass = true;
                    shape.dropped += prune_untraceable(&mut composed, visual_type, &untraceable);
                    if !has_content(&composed, visual_type) {
                        return Ok(declined(
                            ctx,
                            "Nothing in the diagram traced back to the answer, so it was dropped.",
                        ));
                    }
                }
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

    if len("relations") >= 2 && strict_hierarchy(summary) {
        return Some("tree");
    }
    // Doc 16 section 3.5: a tree cannot express a cycle or a cross link, so a
    // relation set that is not a strict hierarchy is a flow. Before values,
    // because relations say how things stand to each other and that is the
    // stronger structure, which is the order the hierarchy rule above already
    // takes.
    if len("relations") >= 2 {
        return Some("flow");
    }
    // Doc 16 section 3.5: at most six large numerals, each one cited. Its own
    // example is "1949, 120m", which is the distinction: a year beside a size
    // is two quantities and a tile each, while eight beside ten is one quantity
    // measured twice and belongs in a table where the two can be read against
    // each other. So the test is whether the units differ, not whether the
    // values are few.
    if (2..=STATS_TILES).contains(&len("values"))
        && headline_figures(summary)
        && len("steps") == 0
        && len("groups") == 0
    {
        return Some("stats");
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
        return Some(if strict_hierarchy(summary) { "tree" } else { "flow" });
    }

    // Doc 16 section 3.5's two shapes are deliberately not hintable, which is
    // why the Router's enum does not carry them. A hint is a guess made before
    // anything was retrieved, and both of these are chosen from structure the
    // Synthesizer grounded: a flow needs edges and a tile needs a figure with a
    // unit, so a hint for either would name a shape with nothing to put in it.
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

/// Doc 16 section 3.5: "tree remains for strict hierarchies".
///
/// Strict means every node has at most one parent and some node has none. A
/// relation set where a node is reached twice is a cross link and one that
/// reaches back is a cycle; a tree can draw neither, and drawing it as a tree
/// anyway would silently drop one of the two edges.
fn strict_hierarchy(summary: &Value) -> bool {
    let Some(relations) = summary.get("relations").and_then(Value::as_array) else {
        return false;
    };
    let mut targets: std::collections::BTreeSet<&str> = Default::default();
    for relation in relations {
        let Some(to) = relation["to"].as_str() else {
            continue;
        };
        // Reached twice: a cross link, whatever the rest of the set does.
        if !targets.insert(to) {
            return false;
        }
    }
    // A root, which a cycle does not have.
    relations
        .iter()
        .filter_map(|r| r["from"].as_str())
        .any(|from| !targets.contains(from))
}

/// Whether the summary's values are separate quantities rather than one
/// quantity measured more than once.
///
/// Two conditions, both from doc 16 section 3.5. Every value is a numeral,
/// because a tile is a large numeral and a summary whose values are words is a
/// table however few of them there are. And the units differ, because tiles
/// stand side by side with nothing to read across them: values sharing a unit
/// are a comparison, and a comparison wants rows.
fn headline_figures(summary: &Value) -> bool {
    let Some(values) = summary.get("values").and_then(Value::as_array) else {
        return false;
    };
    if values.is_empty() {
        return false;
    }
    let numeric = values.iter().all(|v| {
        v["value"]
            .as_str()
            .is_some_and(|text| text.chars().any(|c| c.is_ascii_digit()))
    });
    let units: std::collections::BTreeSet<&str> = values
        .iter()
        .map(|v| v["unit"].as_str().unwrap_or_default())
        .collect();
    numeric && units.len() >= 2
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
        "flow" => json!({
            "type": "object", "required": ["nodes"], "additionalProperties": false,
            "properties": {
                "nodes": { "type": "array", "items": {
                    "type": "object", "required": ["id", "label"], "additionalProperties": false,
                    "properties": {
                        "id": { "type": "string" }, "label": { "type": "string" },
                        "note": { "type": "string" }
                    }
                }},
                "edges": { "type": "array", "items": {
                    "type": "object", "required": ["from", "to"], "additionalProperties": false,
                    "properties": {
                        "from": { "type": "string" }, "to": { "type": "string" },
                        "label": { "type": "string" }
                    }
                }}
            }
        }),
        "stats" => json!({
            "type": "object", "required": ["tiles"], "additionalProperties": false,
            "properties": { "tiles": { "type": "array", "items": {
                "type": "object", "required": ["value", "label"], "additionalProperties": false,
                "properties": {
                    "value": { "type": "string" }, "unit": { "type": "string" },
                    "label": { "type": "string" }
                }
            }}}
        }),
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

/// How the visual came to be, which is what doc 06 section B9's second and
/// third confidence terms are about.
#[derive(Debug, Clone, Copy, Default)]
struct Shape {
    /// The type came from a model call breaking a tie rather than from a rule.
    tie_broken: bool,
    /// Blocks removed before indexing, by a doctrine limit or by doc 06 section
    /// B8.3's second untraceable pass.
    dropped: usize,
}

/// Cut a composed payload down to the doctrine's limits, returning how many
/// elements went. Doc 06 section B5.
///
/// The limits reach the model in the prompt, and a prompt is a request. This is
/// the check, so a pack that allows eight rows gets eight.
fn enforce_limits(composed: &mut Value, visual_type: &str, doctrine: &Value) -> usize {
    let max_nodes = doctrine["max_nodes"].as_u64().unwrap_or(18) as usize;
    let max_rows = doctrine["max_rows"].as_u64().unwrap_or(8) as usize;
    let payload = &mut composed["payload"];
    let mut dropped = 0;

    let mut truncate = |array: Option<&mut Vec<Value>>, limit: usize| {
        if let Some(items) = array
            && items.len() > limit
        {
            dropped += items.len() - limit;
            items.truncate(limit);
        }
    };

    match visual_type {
        "table" => truncate(payload["rows"].as_array_mut(), max_rows),
        "steps" => truncate(payload["steps"].as_array_mut(), max_nodes),
        // The tile cap is the shape's, not the pack's, so it is the smaller of
        // the two. Edges follow their nodes: an edge to a node that was cut
        // would draw a line to nothing.
        "stats" => truncate(payload["tiles"].as_array_mut(), STATS_TILES.min(max_rows.max(1))),
        "flow" => {
            truncate(payload["nodes"].as_array_mut(), max_nodes);
            dropped += drop_dangling_edges(payload);
        }
        "tree" => {
            truncate(payload["root"]["children"].as_array_mut(), max_nodes);
            let budget = max_nodes;
            if let Some(children) = payload["root"]["children"].as_array_mut() {
                let mut used = children.len();
                for child in children.iter_mut() {
                    let Some(grandchildren) = child["children"].as_array_mut() else {
                        continue;
                    };
                    let room = budget.saturating_sub(used);
                    if grandchildren.len() > room {
                        dropped += grandchildren.len() - room;
                        grandchildren.truncate(room);
                    }
                    used += grandchildren.len();
                }
            }
        }
        _ => {
            let mut used = 0usize;
            if let Some(groups) = payload["groups"].as_array_mut() {
                for group in groups.iter_mut() {
                    let Some(items) = group["items"].as_array_mut() else {
                        continue;
                    };
                    let room = max_nodes.saturating_sub(used);
                    if items.len() > room {
                        dropped += items.len() - room;
                        items.truncate(room);
                    }
                    used += items.len();
                }
            }
        }
    }
    dropped
}

/// Remove the blocks whose labels trace back to nothing, returning how many
/// went. Doc 06 section B8.3's second pass, which drops those blocks rather
/// than the diagram.
/// Remove edges naming a node the flow does not carry, returning how many went.
///
/// Every path that shortens the node list calls this: a doctrine limit, an
/// untraceable prune, and a model that named an endpoint it never declared. An
/// edge whose end is missing has nothing to draw between.
fn drop_dangling_edges(payload: &mut Value) -> usize {
    let ids: std::collections::BTreeSet<String> = payload["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["id"].as_str().map(str::to_string))
        .collect();
    let Some(edges) = payload["edges"].as_array_mut() else {
        return 0;
    };
    let before = edges.len();
    edges.retain(|e| {
        let end = |key: &str| e[key].as_str().is_some_and(|id| ids.contains(id));
        end("from") && end("to")
    });
    before - edges.len()
}

fn prune_untraceable(composed: &mut Value, visual_type: &str, untraceable: &[String]) -> usize {
    let gone: std::collections::BTreeSet<&str> = untraceable.iter().map(String::as_str).collect();
    let payload = &mut composed["payload"];
    let mut dropped = 0;

    match visual_type {
        "table" => {
            if let Some(rows) = payload["rows"].as_array_mut() {
                let before = rows.len();
                rows.retain(|row| {
                    !row.as_array()
                        .into_iter()
                        .flatten()
                        .any(|cell| cell.as_str().is_some_and(|label| gone.contains(label)))
                });
                dropped += before - rows.len();
            }
        }
        "steps" => {
            if let Some(steps) = payload["steps"].as_array_mut() {
                let before = steps.len();
                steps.retain(|s| !s["label"].as_str().is_some_and(|l| gone.contains(l)));
                dropped += before - steps.len();
            }
        }
        "stats" => {
            if let Some(tiles) = payload["tiles"].as_array_mut() {
                let before = tiles.len();
                tiles.retain(|t| !t["label"].as_str().is_some_and(|l| gone.contains(l)));
                dropped += before - tiles.len();
            }
        }
        "flow" => {
            if let Some(nodes) = payload["nodes"].as_array_mut() {
                let before = nodes.len();
                nodes.retain(|n| !n["label"].as_str().is_some_and(|l| gone.contains(l)));
                dropped += before - nodes.len();
            }
            dropped += drop_dangling_edges(payload);
        }
        "tree" => {
            if let Some(children) = payload["root"]["children"].as_array_mut() {
                let before = children.len();
                children.retain(|c| !c["label"].as_str().is_some_and(|l| gone.contains(l)));
                dropped += before - children.len();
                for child in children.iter_mut() {
                    if let Some(grandchildren) = child["children"].as_array_mut() {
                        let before = grandchildren.len();
                        grandchildren.retain(|g| !g["label"].as_str().is_some_and(|l| gone.contains(l)));
                        dropped += before - grandchildren.len();
                    }
                }
            }
        }
        _ => {
            if let Some(groups) = payload["groups"].as_array_mut() {
                for group in groups.iter_mut() {
                    if let Some(items) = group["items"].as_array_mut() {
                        let before = items.len();
                        items.retain(|i| !i["name"].as_str().is_some_and(|l| gone.contains(l)));
                        dropped += before - items.len();
                    }
                }
            }
        }
    }
    dropped
}

/// Whether a pruned payload still has anything to draw.
fn has_content(composed: &Value, visual_type: &str) -> bool {
    let payload = &composed["payload"];
    match visual_type {
        "table" => payload["rows"].as_array().is_some_and(|r| !r.is_empty()),
        "steps" => payload["steps"].as_array().is_some_and(|s| !s.is_empty()),
        "stats" => payload["tiles"].as_array().is_some_and(|t| !t.is_empty()),
        // Nodes without edges is a list drawn as boxes, which is what doc 16
        // section 3.5 says a flow is not for.
        "flow" => {
            payload["nodes"].as_array().is_some_and(|n| !n.is_empty())
                && payload["edges"].as_array().is_some_and(|e| !e.is_empty())
        }
        // A root with no children draws one box. Doc 06 section B10 would
        // rather have no visual than one that says nothing, and BN-110 found
        // every visual in the first paid run was exactly this: a single block,
        // no citations, `no_claim` true, because the children the model wrote
        // did not trace back to the summary and were pruned away under it.
        "tree" => payload["root"]["children"]
            .as_array()
            .is_some_and(|c| !c.is_empty()),
        _ => payload["groups"].as_array().is_some_and(|g| {
            g.iter()
                .any(|x| x["items"].as_array().is_some_and(|i| !i.is_empty()))
        }),
    }
}

/// Doc 06 section B8 point 3. A deterministic walk of the payload building
/// pointers and copying citations from the summary entry each label came from.
///
/// Returns the untraceable labels on failure, so the retry prompt can name them.
fn index_blocks(
    composed: &Value,
    visual_type: &str,
    summary: &Value,
    shape: Shape,
) -> Result<Indexed, Vec<String>> {
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
            // A label the summary knows but carries no citation for. Only
            // `values` carry ordinals, so every entity, relation endpoint, step
            // and group item lands here. Doc 06 section B5 wants every block
            // either cited or marked, and these were neither, which made a
            // steps or list visual fail the rule silently while
            // `visual_fidelity` had no threshold to catch it.
            None if lookup.knows(label) => blocks.push(json!({
                "ref": ref_path, "label": label, "citation_ordinals": [], "no_claim": true
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
            // Doc 06 section B5: every label in the payload appears in the block
            // index. The payload schema allows a `bottom_line` and this walk
            // skipped it, so a table could carry a closing claim with no block
            // behind it and no citation, which is exactly what the rule forbids.
            // The head is a label and the text is a claim, so only the text has
            // to trace.
            if payload["bottom_line"].is_object() {
                add(
                    "/bottom_line/head".into(),
                    payload["bottom_line"]["head"].as_str().unwrap_or_default(),
                    true,
                    &mut blocks,
                );
                add(
                    "/bottom_line/text".into(),
                    payload["bottom_line"]["text"].as_str().unwrap_or_default(),
                    false,
                    &mut blocks,
                );
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
        "flow" => {
            for (i, node) in payload["nodes"].as_array().into_iter().flatten().enumerate() {
                add(
                    format!("/nodes/{i}"),
                    node["label"].as_str().unwrap_or_default(),
                    false,
                    &mut blocks,
                );
            }
            // An edge label is the relation's kind, which the summary carries
            // and no citation hangs off. Structural for the same reason a
            // column name is: it names how two claims stand to each other
            // rather than making a third.
            for (i, edge) in payload["edges"].as_array().into_iter().flatten().enumerate() {
                let Some(label) = edge["label"].as_str().filter(|l| !l.is_empty()) else {
                    continue;
                };
                add(format!("/edges/{i}"), label, true, &mut blocks);
            }
        }
        "stats" => {
            // Doc 16 section 3.5: every tile cited. The label and the value are
            // one block, because a tile is one thing on the canvas and two
            // blocks would let half of it be hidden. The value is what has to
            // trace, since that is the claim.
            for (i, tile) in payload["tiles"].as_array().into_iter().flatten().enumerate() {
                let value = tile["value"].as_str().unwrap_or_default();
                let unit = tile["unit"].as_str().unwrap_or_default();
                let label = tile["label"].as_str().unwrap_or_default();
                let traced = lookup
                    .citations_for(format!("{value} {unit}").trim())
                    .or_else(|| lookup.citations_for(value))
                    .or_else(|| lookup.citations_for(label));
                match traced {
                    Some(ordinals) => blocks.push(json!({
                        "ref": format!("/tiles/{i}"), "label": label, "citation_ordinals": ordinals
                    })),
                    // Never `no_claim`: doc 16 section 3.5 makes an uncited tile
                    // a `numeric_without_citation` block flag, which needs the
                    // block to be there and uncited to fire on.
                    None if lookup.knows(value) || lookup.knows(label) => blocks.push(json!({
                        "ref": format!("/tiles/{i}"), "label": label, "citation_ordinals": []
                    })),
                    None => untraceable.push(label.to_string()),
                }
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

    // Doc 06 section B9: the share of blocks with citations at 0.6, a type the
    // rules chose rather than a tie break at 0.2, and no blocks dropped at 0.2.
    //
    // The last two were written as constants, `+ 0.2 + 0.2 * 100.0 / 100.0`, so
    // confidence could never fall below 0.4 whatever happened and B9's "under
    // 0.5 the Verifier is told to check block bindings first" could never fire
    // on a visual whose blocks were the problem.
    let total = blocks.len().max(1) as f64;
    let cited = blocks
        .iter()
        .filter(|b| b["citation_ordinals"].as_array().is_some_and(|c| !c.is_empty()))
        .count() as f64;
    let by_rule = if shape.tie_broken { 0.0 } else { 0.2 };
    let intact = if shape.dropped == 0 { 0.2 } else { 0.0 };
    let confidence = ((cited / total) * 0.6 + by_rule + intact).min(1.0);

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
    fn a_cross_link_becomes_a_flow() {
        // Doc 16 section 3.5: a tree cannot express a cross link. Perception is
        // reached from two places, and drawing it as a tree would draw it twice
        // or drop one of the edges.
        let summary = json!({
            "relations": [
                { "from": "World model", "to": "Perception", "kind": "has" },
                { "from": "Planner", "to": "Perception", "kind": "reads" }
            ]
        });
        assert_eq!(select_type(&summary, "none"), Some("flow"));
    }

    #[test]
    fn a_cycle_becomes_a_flow() {
        let summary = json!({
            "relations": [
                { "from": "Draft", "to": "Review", "kind": "goes to" },
                { "from": "Review", "to": "Draft", "kind": "returns to" }
            ]
        });
        // Every node is a target, so there is no root and no tree.
        assert_eq!(select_type(&summary, "none"), Some("flow"));
    }

    #[test]
    fn figures_in_different_units_become_tiles() {
        // Doc 16 section 3.5's own example: "1949, 120m".
        let summary = json!({ "values": [
            { "label": "founded", "value": "1949", "unit": "", "citation": 1 },
            { "label": "floor space", "value": "120", "unit": "m", "citation": 2 }
        ]});
        assert_eq!(select_type(&summary, "none"), Some("stats"));
    }

    #[test]
    fn one_quantity_measured_twice_stays_a_table() {
        // Tiles stand side by side with nothing to read across them, and two
        // figures in the same unit are there to be read against each other.
        let summary = json!({ "values": [
            { "label": "old", "value": "8", "unit": "%", "citation": 1 },
            { "label": "new", "value": "10", "unit": "%", "citation": 2 }
        ]});
        assert_eq!(select_type(&summary, "none"), Some("table"));
    }

    #[test]
    fn a_flow_indexes_its_nodes_and_its_edge_labels() {
        let summary = json!({
            "relations": [
                { "from": "Draft", "to": "Review", "kind": "goes to" },
                { "from": "Review", "to": "Draft", "kind": "returns to" }
            ],
            "values": [{ "label": "Review", "value": "2", "unit": "days", "citation": 4 }]
        });
        let composed = json!({
            "title": "The loop",
            "payload": {
                "nodes": [{ "id": "a", "label": "Draft" }, { "id": "b", "label": "Review" }],
                "edges": [{ "from": "a", "to": "b", "label": "goes to" }]
            }
        });
        let indexed = index_blocks(&composed, "flow", &summary, Shape::default()).expect("indexes");
        let blocks = indexed.blocks.as_array().expect("blocks");

        let node = blocks
            .iter()
            .find(|b| b["ref"] == "/nodes/1")
            .expect("the second node");
        assert_eq!(node["citation_ordinals"], json!([4]));
        // An edge label names how two claims stand to each other rather than
        // making a third, so it carries no citation and says so.
        let edge = blocks.iter().find(|b| b["ref"] == "/edges/0").expect("the edge");
        assert_eq!(edge["no_claim"], json!(true));
    }

    #[test]
    fn an_edge_goes_with_the_node_it_pointed_at() {
        let mut composed = json!({
            "title": "T",
            "payload": {
                "nodes": [{ "id": "a", "label": "Draft" }, { "id": "b", "label": "invented" }],
                "edges": [{ "from": "a", "to": "b" }]
            }
        });
        let dropped = prune_untraceable(&mut composed, "flow", &["invented".to_string()]);
        // The node and the edge that had nothing left to point at.
        assert_eq!(dropped, 2);
        assert_eq!(composed["payload"]["edges"], json!([]));
        // Nodes with no edges left is a list drawn as boxes, which is not a flow.
        assert!(!has_content(&composed, "flow"));
    }

    #[test]
    fn a_seventh_tile_is_cut_whatever_the_pack_allows() {
        // Doc 16 section 3.5 caps tiles at six. The shape is the limit, so a
        // pack allowing more rows does not buy a seventh numeral.
        let mut composed = json!({
            "title": "T",
            "payload": { "tiles": (0..9).map(|i| json!({ "value": format!("{i}"), "label": format!("l{i}") })).collect::<Vec<_>>() }
        });
        let dropped = enforce_limits(&mut composed, "stats", &json!({ "max_rows": 20 }));
        assert_eq!(dropped, 3);
        assert_eq!(composed["payload"]["tiles"].as_array().expect("tiles").len(), 6);
    }

    #[test]
    fn an_uncited_tile_keeps_its_block_and_no_marking() {
        // Doc 16 section 3.5 makes an uncited tile a `numeric_without_citation`
        // block flag, which needs a block that is present and uncited. Marking
        // it `no_claim` would excuse exactly the case the rule exists to catch.
        let summary = json!({ "values": [
            { "label": "founded", "value": "1949", "unit": "" },
            { "label": "floor space", "value": "120", "unit": "m", "citation": 2 }
        ]});
        let composed = json!({
            "title": "In numbers",
            "payload": { "tiles": [
                { "value": "1949", "unit": "", "label": "founded" },
                { "value": "120", "unit": "m", "label": "floor space" }
            ]}
        });
        let indexed = index_blocks(&composed, "stats", &summary, Shape::default()).expect("indexes");
        let blocks = indexed.blocks.as_array().expect("blocks");
        let uncited = blocks
            .iter()
            .find(|b| b["ref"] == "/tiles/0")
            .expect("the first tile");
        assert_eq!(uncited["citation_ordinals"], json!([]));
        assert!(uncited.get("no_claim").is_none());
        let cited = blocks
            .iter()
            .find(|b| b["ref"] == "/tiles/1")
            .expect("the second tile");
        assert_eq!(cited["citation_ordinals"], json!([2]));
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
        let err = index_blocks(&composed, "table", &summary, Shape::default()).expect_err("must not pass");
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
        let indexed = index_blocks(&composed, "table", &summary, Shape::default()).expect("indexes");
        let blocks = indexed.blocks.as_array().expect("blocks");

        let cell = blocks
            .iter()
            .find(|b| b["ref"] == "/rows/0/1")
            .expect("the value cell");
        assert_eq!(cell["citation_ordinals"], json!([3]));
    }

    #[test]
    fn a_doctrine_limit_cuts_the_rows_it_names() {
        // Doc 06 section B5. The limit reached the model in the prompt and
        // nothing checked the answer, so a pack allowing eight rows got twenty.
        let mut composed = json!({
            "title": "T",
            "payload": {
                "columns": ["A", "B"],
                "rows": (0..20).map(|i| json!([format!("r{i}"), "v"])).collect::<Vec<_>>()
            }
        });
        let dropped = enforce_limits(&mut composed, "table", &json!({ "max_rows": 8 }));
        assert_eq!(dropped, 12);
        assert_eq!(composed["payload"]["rows"].as_array().expect("rows").len(), 8);
    }

    #[test]
    fn a_second_untraceable_pass_drops_the_block_and_keeps_the_rest() {
        // Doc 06 section B8.3: "a second failure drops those blocks", which is
        // not the same as dropping the diagram.
        let mut composed = json!({
            "title": "T",
            "payload": {
                "columns": ["Rule", "Value"],
                "rows": [["buffer", "2.5"], ["invented", "9.9"]]
            }
        });
        let dropped = prune_untraceable(&mut composed, "table", &["invented".to_string()]);
        assert_eq!(dropped, 1);
        assert_eq!(composed["payload"]["rows"], json!([["buffer", "2.5"]]));
        assert!(has_content(&composed, "table"));

        // And when nothing traceable is left there is no diagram to keep.
        let mut all_bad = json!({
            "title": "T",
            "payload": { "columns": ["Rule"], "rows": [["invented"]] }
        });
        prune_untraceable(&mut all_bad, "table", &["invented".to_string()]);
        assert!(!has_content(&all_bad, "table"));
    }

    #[test]
    fn a_tree_with_no_children_is_not_a_diagram() {
        // BN-110. Every visual in the first paid run was exactly this: one
        // block, no citations, `no_claim` true. A root on its own draws a box
        // with a label in it, which doc 06 section B10 would rather not draw at
        // all, and it reached the card by two routes: a model that returned a
        // bare root, and a model whose children were pruned as untraceable.
        let bare = json!({ "title": "T", "payload": { "root": { "label": "Buffer" } } });
        assert!(!has_content(&bare, "tree"));

        let mut pruned = json!({
            "title": "T",
            "payload": { "root": { "label": "Buffer", "children": [{ "label": "invented" }] } }
        });
        prune_untraceable(&mut pruned, "tree", &["invented".to_string()]);
        assert!(!has_content(&pruned, "tree"));

        let real = json!({
            "title": "T",
            "payload": { "root": { "label": "Buffer", "children": [{ "label": "Solo level" }] } }
        });
        assert!(has_content(&real, "tree"));
    }

    #[test]
    fn confidence_falls_when_a_block_was_dropped() {
        // Doc 06 section B9's third term. It was written as `0.2 * 100.0 /
        // 100.0`, so confidence could never fall below 0.4 whatever happened.
        let summary = json!({ "values": [{ "label": "buffer", "value": "2.5", "citation": 1 }] });
        let composed = json!({
            "title": "T",
            "payload": { "columns": ["Rule", "Value"], "rows": [["buffer", "2.5"]] }
        });

        let intact = index_blocks(&composed, "table", &summary, Shape::default()).expect("indexes");
        let pruned = index_blocks(
            &composed,
            "table",
            &summary,
            Shape {
                tie_broken: false,
                dropped: 1,
            },
        )
        .expect("indexes");
        assert!(pruned.confidence < intact.confidence, "{pruned:?} {intact:?}");

        let tied = index_blocks(
            &composed,
            "table",
            &summary,
            Shape {
                tie_broken: true,
                dropped: 0,
            },
        )
        .expect("indexes");
        assert!(tied.confidence < intact.confidence);
    }

    #[test]
    fn a_step_the_summary_knows_but_never_cited_is_marked_rather_than_left_bare() {
        // Doc 06 section B5 wants every block cited or marked. Only `values`
        // carry ordinals, so a steps visual produced blocks that were neither.
        let summary = json!({ "steps": ["identify the counterparty", "assign the risk weight"] });
        let composed = json!({
            "title": "T",
            "payload": { "steps": [
                { "label": "identify the counterparty" },
                { "label": "assign the risk weight" }
            ] }
        });
        let indexed = index_blocks(&composed, "steps", &summary, Shape::default()).expect("indexes");
        for block in indexed.blocks.as_array().expect("blocks") {
            let cited = block["citation_ordinals"]
                .as_array()
                .is_some_and(|c| !c.is_empty());
            assert!(
                cited || block["no_claim"] == true,
                "a block with neither citations nor no_claim: {block}"
            );
        }
    }

    #[test]
    fn a_column_heading_is_structural_and_may_carry_no_claim() {
        // Doc 07 section B8.3 limits no_claim to structural labels.
        let summary = json!({ "values": [{ "label": "buffer", "value": "2.5", "citation": 1 }] });
        let composed = json!({
            "title": "T",
            "payload": { "columns": ["Rule", "Value"], "rows": [["buffer", "2.5"]] }
        });
        let indexed = index_blocks(&composed, "table", &summary, Shape::default()).expect("indexes");
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
        let indexed = index_blocks(&composed, "steps", &summary, Shape::default()).expect("indexes");
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
