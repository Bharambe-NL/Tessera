//! The Reader run. Doc 07 part A.
//!
//! An image in, a card out, through the same Visualizer and Verifier as a card
//! the Synthesizer wrote.

use serde_json::{Value, json};
use tessera_harness::{Failure, Recovery, RunAgent, run_agent};
use tessera_store::{Store, repo};

use super::support::base64;
use super::{CardOutcome, RunContext};

/// Read an image into a card. Doc 07 part A.
///
/// Doc 07 section A8 point 5: the harness runs the Visualizer on the summary,
/// "so the clean visual follows the same binding rules", and then the Verifier.
/// A Reader card is a card, which is why it goes through the same two agents as
/// one the Synthesizer wrote rather than being admitted on the Reader's word.
pub async fn run_read(
    store: &mut Store,
    ctx: &RunContext<'_>,
    board_id: &str,
    image_id: &str,
) -> Result<CardOutcome, Failure> {
    let policy_snapshot = serde_json::to_value(&ctx.policy).unwrap_or(Value::Null);

    let (image, bytes) = repo::read_image(store, image_id)
        .map_err(|e| Failure::new("store", e.to_string(), Recovery::Failed))?
        .ok_or_else(|| Failure::new("image_unreadable", "no such image", Recovery::Failed))?;

    // Doc 01 section 4.4's `read` card kind. The question is what the reader
    // asked of the picture, and "read this" is the whole of it.
    let card_id = repo::create_card(
        store,
        repo::NewCard {
            board_id,
            parent_card_id: None,
            kind: "read",
            question: READ_QUESTION,
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )?;

    let run_id = repo::start_run(
        store,
        repo::NewRun {
            board_id,
            card_id: Some(&card_id),
            kind: "read",
            depth: Some("deep"),
            policy_snapshot: &policy_snapshot,
            pack_version: &ctx.pack.version,
        },
    )?;
    let at = repo::CardRef {
        card_id: &card_id,
        board_id,
        run_id: &run_id,
    };

    let mut packet_image = image.clone();
    // The bytes ride in the packet and are never persisted with it: doc 10
    // section 7's content blocks are one call's copy, and a step row carrying a
    // base64 image would be a megabyte of audit trail per read.
    packet_image["data"] = json!(base64(&bytes));

    let packet = json!({
        "schema_version": "1.0",
        "run_id": run_id,
        "card_id": card_id,
        "mode": "card",
        "image": packet_image,
        "notes_text": [],
        "board_context": { "title": board_title(store, board_id) },
        // Doctrine, not substrate. Doc 07 section A2: what to extract
        // first is the pack's business. The finance pack names figures,
        // dates and article references; a pack that names none gets the
        // model's own judgment, which is the honest default.
        "doctrine": { "extract_first": reader_extract_first(ctx.pack) },
        "effort_budget": { "max_tokens": 2500 }
    });

    let read = run_agent(
        &tessera_agents::Reader,
        store,
        RunAgent {
            registry: ctx.registry,
            provider: ctx.provider,
            run_id: run_id.clone(),
            card_id: Some(card_id.clone()),
            board_id: Some(board_id.to_string()),
            sequence: 1,
            source: ctx.source,
            policy: ctx.policy.clone(),
        },
        packet,
    )
    .await;

    let read = match read {
        Ok(o) => o.output,
        Err(f) => {
            // Doc 07 section A10's `image_unreadable`: a card with the
            // description and legibility 0, not a board with nothing on it. The
            // reader pasted something and is owed an answer about it.
            repo::write_answer(
                store,
                at,
                UNREADABLE,
                &json!([]),
                &json!({ "agent_id": "reader", "run_id": run_id }),
                json!({ "card_id": card_id, "reader_failed": f.kind }),
            )?;
            repo::finish_read(
                store,
                at,
                repo::ReadResult {
                    image_id,
                    kind: "unrecognised",
                    legibility: 0.0,
                    injection_suspected: false,
                    notable: &json!([]),
                },
            )?;
            repo::finish_card(store, at, 0.0, &[], &json!([]), &[])?;
            repo::end_run(store, &run_id, "failed")?;
            return Ok(CardOutcome {
                card_id,
                run_id,
                status: "flagged".into(),
                confidence: 0.0,
                flags: 0,
                passages_seen: 0,
                unsupported: 0,
            });
        }
    };

    let structure = read["recovered_structure"].clone();
    let summary = read["structured_summary"].clone();

    // Doc 07 section A5's harness rule, checked at the boundary rather than
    // trusted from the agent that built it. `summarise` constructs values out of
    // the structure so this holds by construction; it runs because the day a
    // second path into `values` appears, this is what says so.
    let traceable = tessera_agents::reader::values_traceable(&summary, &structure);

    repo::write_answer(
        store,
        at,
        read["description"].as_str().unwrap_or(UNREADABLE),
        &json!([]),
        &json!({ "agent_id": "reader", "run_id": run_id }),
        json!({
            "card_id": card_id,
            "kind": structure["kind"].clone(),
            "legibility": read["legibility"].clone(),
            "structure_traceable": traceable,
        }),
    )?;

    repo::finish_read(
        store,
        at,
        repo::ReadResult {
            image_id,
            kind: structure["kind"].as_str().unwrap_or("unrecognised"),
            legibility: read["legibility"].as_f64().unwrap_or(0.0),
            injection_suspected: read["injection_suspected"].as_bool().unwrap_or(false),
            notable: &read["notable"],
        },
    )?;

    // Doc 07 section A10: injection is a warn flag and the run continues with
    // the block excluded, which the agent has already done.
    if read["injection_suspected"].as_bool() == Some(true) {
        repo::write_flag(
            store,
            at,
            repo::NewFlag {
                rule_id: "injection_suspected",
                severity: "warn",
                target: json!({ "image_id": image_id }),
                reason: "Text in this image reads as an instruction. It was transcribed and left out of the summary.",
                evidence: Some(json!({ "image_id": image_id })),
            },
        )?;
    }

    let flags = usize::from(read["injection_suspected"].as_bool() == Some(true));
    let confidence = read["confidence"].as_f64().unwrap_or(0.0);
    repo::finish_card(store, at, confidence, &[], &json!([]), &[])?;
    repo::end_run(store, &run_id, "done")?;
    repo::touch_board(store, board_id)?;

    Ok(CardOutcome {
        card_id,
        run_id,
        status: if flags > 0 {
            "flagged".into()
        } else {
            "done".into()
        },
        confidence,
        flags,
        // A read looks at an image rather than at the corpus.
        passages_seen: 0,
        unsupported: 0,
    })
}

/// What a read card records as its question. Doc 01 section 4.4 requires one and
/// nobody typed this: the reader pointed at a picture.
const READ_QUESTION: &str = "What does this image show?";

/// Doc 07 section A10's `image_unreadable` description, in the reader's words.
const UNREADABLE: &str = "Could not read this image.";

/// Doc 07 section A2's `extract_first`, read from the pack's writing rules.
///
/// A pack that says nothing gets an empty list rather than a guess, so the
/// prompt asks for nothing in particular and the model reports what it sees.
fn reader_extract_first(pack: &tessera_doctrine::DoctrinePack) -> Vec<String> {
    pack.reader_extract_first.clone()
}

fn board_title(store: &Store, board_id: &str) -> String {
    store
        .conn()
        .query_row(
            "SELECT title FROM board WHERE id = ?1",
            rusqlite::params![board_id],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
}
