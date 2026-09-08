//! Board bundles: the checklist, the export and the import.
//!
//! Doc 01 section 7 and doc 10 section 11. This is the one export whose
//! recipient is a stranger, so the summary travels beside the bytes.

use serde::Deserialize;
use serde_json::json;

use crate::core::Core;
use crate::pipeline;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 01 section 4.6. The bytes arrive base64 because the boundary is
    // JSON-RPC and a webview has no path to the blob store; the core writes them
    // once, by hash, so a board forked from a bundle never duplicates a picture.
    // Doc 01 section 7. The bytes cross the JSON-RPC boundary as base64 for the
    // same reason an image does: a webview has no path to the file system, and
    // the shell writing the file is what puts the Save dialog where a person
    // expects it.
    r.register("board.export_preflight", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        // Doc 01 section 7 shows the checklist before the file exists, not
        // after: a checklist shown after the export is a receipt.
        let check = tessera_bundle::preflight(&core.store, &p.board_id)
            .map_err(|e| RpcError::core("bundle", e.to_string()))?;
        serde_json::to_value(check).map_err(|e| RpcError::core("bundle", e.to_string()))
    });

    r.register("board.export", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Export {
            board_id: String,
            #[serde(default = "yes")]
            with_history: bool,
            /// Source ids the author cleared on the checklist. Absent means
            /// none, which is the safe reading: a local document travels only
            /// when someone said so.
            #[serde(default)]
            local_documents: Vec<String>,
            /// Page ids the author ticked. Absent means none, for the same
            /// reason: doc 16's vault is the person's own writing.
            #[serde(default)]
            pages: Vec<String>,
            #[serde(default)]
            exported_by: Option<String>,
        }
        let p: Export = params(p)?;
        let options = tessera_bundle::ExportOptions {
            with_history: p.with_history,
            local_documents: p.local_documents.into_iter().collect(),
            pages: p.pages.into_iter().collect(),
            exported_by: p.exported_by,
        };

        let mut archive = std::io::Cursor::new(Vec::new());
        let manifest = tessera_bundle::export(
            &mut core.store,
            &core.registry,
            &p.board_id,
            &options,
            &mut archive,
        )
        .map_err(|e| RpcError::core("bundle", e.to_string()))?;

        Ok(json!({
            "manifest": manifest,
            "bytes": pipeline::base64(&archive.into_inner()),
        }))
    });

    r.register("board.import", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Import {
            data: String,
        }
        let p: Import = params(p)?;
        let bytes = decode_base64(&p.data)
            .ok_or_else(|| RpcError::core("bad_bundle", "That file could not be read as a bundle."))?;
        let profile_id = core.profile_id.clone();
        let outcome = tessera_bundle::import(&mut core.store, &profile_id, std::io::Cursor::new(bytes))
            .map_err(|e| RpcError::core("bundle", e.to_string()))?;
        serde_json::to_value(outcome).map_err(|e| RpcError::core("bundle", e.to_string()))
    });
}
