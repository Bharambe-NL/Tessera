//! The exercise run. Doc 08.
//!
//! Its own run kind, because nothing is retrieved and no claim is added to the
//! board: this reads the cards that exist and writes questions about them.

use serde_json::{Value, json};
use tessera_harness::{Failure, Recovery, RunAgent, run_agent};
use tessera_store::{Store, repo};

use super::RunContext;

/// Generate an exercise from the cards a board already holds. Doc 08.
///
/// A run of its own, kind `exercise`, because it is not a card: nothing is
/// retrieved, nothing is verified, and no claim is added to the board. Doc 08
/// section 1 is explicit that this reads what exists and never asks for a new
/// fact, and giving it its own run kind is what makes that visible in the log.
pub async fn run_exercise(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    audience_id: Option<&str>,
    level: Option<u8>,
) -> Result<ExerciseOutcome, Failure> {
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);

    // Doc 08 section 8 point 1: cap by budget. Eight items over at most eight
    // cards, because the packet's own budget is eight.
    let cards = repo::cards_for_exercise(store, board_id, 8)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;
    let scope: Vec<String> = cards
        .iter()
        .filter_map(|c| c["card_id"].as_str().map(str::to_string))
        .collect();

    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: None,
            kind: "exercise",
            depth: None,
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;

    // Doc 08 section 10's `no_eligible_cards`, decided before the packet rather
    // than inside the agent, because the packet schema requires at least one
    // card and a schema violation is the wrong way to report an empty board.
    if cards.is_empty() {
        repo::end_run(store, &run_id, "done")?;
        return Ok(ExerciseOutcome {
            exercise_id: None,
            run_id,
            items: 0,
            dropped: 0,
        });
    }

    let template = ctx.pack.exercise_templates.first();
    let template_id = template.map(|t| t.id.as_str()).unwrap_or("default");
    let mut packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "board_id": board_id,
        "scope": { "card_ids": scope },
        "cards": cards,
        "concepts": repo::concepts_for_packet(store, &ctx.profile_id, 20).unwrap_or_default(),
        "template": {
            "id": template_id,
            "item_kinds": template
                .map(|t| t.item_kinds.clone())
                .unwrap_or_else(|| vec!["recall".into(), "apply".into()]),
            // Doc 17 section 4's ladder, carried as the pack wrote it. The
            // agent reads which kinds a level asks for and which level a kind
            // sits at from this, so the mapping stays doctrine.
            "levels": ctx.pack.learning_templates.check_templates,
            "items_per_card_max": template.and_then(|t| t.items_per_card_max).unwrap_or(2),
            "options": level
                .and_then(|l| level_options(ctx, l))
                .map(|o| o as usize)
                .or_else(|| template.and_then(|t| t.options))
                .unwrap_or(4),
        },
        "audience_id": audience_id,
        "effort_budget": { "max_tokens": 2500, "max_items": 8 }
    });
    // Absent rather than null when no level was asked for. The packet schema
    // types this as an integer, and a null is a value that fails at the
    // boundary rather than a field that is not there.
    if let Some(level) = level {
        packet["template"]["level"] = json!(level);
    }

    let drafted = run_agent(
        &tessera_agents::Exercise,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: None,
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    let output = match drafted {
        Ok(o) => o.output,
        Err(f) => {
            repo::end_run(store, &run_id, "failed")?;
            return Err(f);
        }
    };

    let items = output["items"].clone();
    let count = items.as_array().map(Vec::len).unwrap_or(0);
    // Doc 08 section 9: the ratio of items that passed both checks. A caveat
    // means some were dropped, and the count of dropped ones is what the caveat
    // states, so the outcome carries it rather than the prose.
    let dropped = output["caveats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|c| c.split_whitespace().next())
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();

    let exercise_id = repo::write_exercise(
        store,
        repo::NewExercise {
            board_id,
            run_id: &run_id,
            template_id,
            audience_id,
            scope: &scope,
            items: &items,
            produced_by: &json!({ "agent_id": "exercise", "run_id": run_id }),
        },
    )
    .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?;

    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    Ok(ExerciseOutcome {
        exercise_id: Some(exercise_id),
        run_id,
        items: count,
        dropped,
    })
}

/// The option count a level asks for, when the pack's check template names one.
fn level_options(ctx: &RunContext<'_>, level: u8) -> Option<u32> {
    ctx.pack
        .learning_templates
        .check_templates
        .iter()
        .find(|t| t.level == level)
        .and_then(|t| t.options)
}

/// What one exercise run produced. `exercise_id` is absent when the board had
/// no card worth testing, which is an outcome rather than a failure.
#[derive(Debug, Clone)]
pub struct ExerciseOutcome {
    pub exercise_id: Option<String>,
    pub run_id: String,
    pub items: usize,
    pub dropped: usize,
}
