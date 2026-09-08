//! The small shared pieces: the lesson constants, base64 for a content block,
//! the failure shim and a truncation that does not split a character in half.
//!
//! None of these is about one stage of the pipeline, and each is read from a
//! file other than the one that would otherwise own it.

use serde_json::json;
use tessera_harness::Failure;
use tessera_store::{Store, repo};

use super::CardOutcome;

/// Doc 17 section 5: "the research retriever enabled ... plus the vault and
/// boards retrievers". Local is left out on purpose: a lesson is about a topic
/// rather than about the learner's documents, and doc 17 names three.
pub const LESSON_RETRIEVERS: &[&str] = &["web", "vault", "boards"];

/// Doc 17 section 5's larger fetch budget. Doc 05 section 8.1's eight is what a
/// card gets; a lesson reads more widely because it is building somebody's
/// understanding of a topic rather than answering one question.
pub const LESSON_FETCH_BUDGET: usize = 16;

/// The board mode a lesson runs in. Doc 14 section 2.
pub const LEARN: &str = "learn";

/// Base64 for one content block. Doc 10 section 7: the encoded copy is for one
/// call and is never persisted.
pub fn base64(bytes: &[u8]) -> String {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(SET[(n >> 18) as usize & 63] as char);
        out.push(SET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            SET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            SET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub(super) fn fail(
    store: &mut Store,
    card_id: &str,
    board_id: &str,
    run_id: &str,
    f: Failure,
) -> Result<CardOutcome, Failure> {
    let detail = serde_json::to_value(&f).unwrap_or(json!({ "type": f.kind.clone() }));
    repo::fail_card(store, card_id, board_id, &detail)?;
    repo::end_run(store, run_id, "failed")?;
    Err(f)
}

/// Cut to a character count without splitting a character in half.
pub(super) fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}
