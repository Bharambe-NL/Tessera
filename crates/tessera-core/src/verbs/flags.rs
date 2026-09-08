//! The verification queue: what the Verifier raised and what was decided.
//!
//! Doc 09 section 6. The queue reads open flags across every board, so it is a
//! profile query rather than a board one.

use serde::Deserialize;
use serde_json::json;
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 09 section 6: the queue reads open flags across every board, so it is
    // a profile query rather than a board one.
    r.register("flag.list", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Query {
            #[serde(default = "default_flag_limit")]
            limit: i64,
        }
        let p: Query = params(p)?;
        let flags =
            repo::open_flags(&core.store, &core.profile_id, p.limit.clamp(1, 500)).map_err(store_error)?;
        Ok(json!({ "flags": flags }))
    });

    // Doc 09 section 6's row actions and its bulk decisions, which are the same
    // call with more ids.
    r.register("flag.decide", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Decide {
            flag_ids: Vec<String>,
            decision: String,
            #[serde(default)]
            note: Option<String>,
        }
        let p: Decide = params(p)?;
        if !matches!(p.decision.as_str(), "accept" | "dismiss" | "rerun" | "edit") {
            return Err(RpcError::core(
                "unknown_decision",
                "A flag is accepted, dismissed, rerun or edited.",
            ));
        }
        match repo::decide_flags(&mut core.store, &p.flag_ids, &p.decision, p.note.as_deref())
            .map_err(store_error)?
        {
            Some(review_id) => Ok(json!({ "review_id": review_id, "decided": p.flag_ids.len() })),
            None => Err(RpcError::core(
                "no_open_flag",
                "Those flags were decided already. Reload the queue to see where they went.",
            )),
        }
    });
}
