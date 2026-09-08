//! What the handlers share: the params they decode, the defaults they fill in,
//! and the four functions that turn a failure into an RPC error.
//!
//! These were private to `core.rs` while the handlers lived there. They are
//! `pub(super)` here, which is the same reach: visible to every verb module and
//! to nothing outside `verbs`.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::core::CoreError;
use crate::rpc::RpcError;

/// What a new session is called until its first question renames it, which is
/// the same rule doc 01 section 4.1 gives every other board.
pub(super) const NOTEBOOK_TITLE: &str = "A question of my notes";

/// The palette token a sticky takes when the caller names none. Doc 01 section
/// 4.5 stores a token name rather than a colour, so the board's own palette
/// decides what amber looks like and a theme change does not repaint every
/// sticky one by one.
pub(super) const NOTE_COLOUR: &str = "amber";

/// Where a sticky lands when the caller names no place: to the right of the
/// first column, in board coordinates. The caller that knows better, which is
/// the canvas, sends its own.
pub(super) const NOTE_PLACE: [(&str, i64); 4] = [("x", 560), ("y", 80), ("w", 220), ("h", 140)];

/// The default sticky place, as the `{x, y, w, h}` object doc 01 section 4.5
/// stores.
pub(super) fn default_place() -> Value {
    Value::Object(
        NOTE_PLACE
            .iter()
            .map(|(key, value)| ((*key).to_string(), json!(value)))
            .collect(),
    )
}

#[derive(Deserialize)]
pub(super) struct BoardCreate {
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) depth: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct BoardRef {
    pub(super) board_id: String,
}

#[derive(Deserialize)]
pub(super) struct Ask {
    pub(super) board_id: String,
    pub(super) question: String,
    #[serde(default)]
    pub(super) depth: Option<String>,
    /// The alias the user chose from the chat window's model control, absent
    /// when they left it on auto. Doc 01 section 5's `model_override`, pinned
    /// to the synthesize stage.
    #[serde(default)]
    pub(super) model: Option<String>,
    /// Doc 09 section 5's Branch verb, in its three forms: absent for a root
    /// card, present alone for a follow-up, present with an anchor for a branch.
    #[serde(default)]
    pub(super) parent_card_id: Option<String>,
    #[serde(default)]
    pub(super) anchor_text: Option<String>,
    #[serde(default)]
    pub(super) anchor_block_ref: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct CardRef {
    pub(super) board_id: String,
    pub(super) card_id: String,
}

pub(super) fn default_board_status() -> String {
    "active".to_string()
}

/// Enough rows for the queue to be worth scrolling, few enough that a profile
/// with thousands of flags does not hand the webview all of them at once.
pub(super) fn default_flag_limit() -> i64 {
    200
}

/// `with_history` defaults to on. Doc 01 section 7: "events.jsonl is on by
/// default", and dropping history is the deliberate act, not keeping it.
pub(super) fn yes() -> bool {
    true
}

pub(super) fn default_library_limit() -> i64 {
    500
}

/// A bound on one pasted image, so a gesture cannot fill the profile folder.
pub(super) const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Decode one base64 image. `None` on anything that is not base64, which the
/// boundary reports as a bad image rather than storing a blob nobody can read.
pub(super) fn decode_base64(s: &str) -> Option<Vec<u8>> {
    const SET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, c) in SET.iter().enumerate() {
        lookup[*c as usize] = i as u8;
    }

    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in s.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let value = lookup[byte as usize];
        if value == 255 {
            return None;
        }
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// A page the vault would not take, in the words a person can act on.
pub(super) fn page_error(e: crate::vault::SaveError) -> RpcError {
    match e {
        crate::vault::SaveError::Refused(crate::vault::TITLE_TAKEN_BY_A_PAGE) => RpcError::core(
            "page_title_taken",
            "Another page already has this title. Pick a different one.",
        ),
        crate::vault::SaveError::Refused(crate::vault::NO_TITLE_GIVEN) => {
            RpcError::core("page_untitled", "Give the page a title, then save it.")
        }
        crate::vault::SaveError::Refused(_) => {
            RpcError::core("page_missing", "That page is not in this vault.")
        }
        crate::vault::SaveError::Store(e) => store_error(e),
    }
}

pub(super) fn core_error(e: CoreError) -> RpcError {
    // House style: say what happened and how to fix it. Doc 11 section 9.
    match &e {
        CoreError::Provider(p) => RpcError::core(p.kind(), provider_message(p)),
        other => RpcError::core("core", other.to_string()),
    }
}

pub(super) fn provider_message(e: &tessera_providers::ProviderError) -> String {
    use tessera_providers::ProviderError as P;
    match e {
        P::NoKey { .. } => "No model key. Add one in Profile to answer cards.".into(),
        P::Auth { provider } => format!("The {provider} key was rejected. Check it in Profile."),
        P::RateLimited { provider, .. } => format!("{provider} is rate limiting. Try again shortly."),
        other => other.to_string(),
    }
}

pub(super) fn store_error(e: tessera_store::StoreError) -> RpcError {
    RpcError::core("store", e.to_string())
}
