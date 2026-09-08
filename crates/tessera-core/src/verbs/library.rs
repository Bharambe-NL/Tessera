//! The Library's two tabs: sources and concepts.
//!
//! Doc 09 section 9. Both are reads over what the profile has accumulated.

use serde::Deserialize;
use serde_json::json;
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Doc 09 section 9's two Library tabs.
    r.register("library.sources", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Query {
            #[serde(default = "default_library_limit")]
            limit: i64,
        }
        let p: Query = params(p)?;
        let sources =
            repo::list_sources(&core.store, &core.profile_id, p.limit.clamp(1, 1000)).map_err(store_error)?;
        Ok(json!({ "sources": sources }))
    });

    r.register("library.concepts", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Query {
            #[serde(default = "default_library_limit")]
            limit: i64,
        }
        let p: Query = params(p)?;
        let concepts = repo::list_concepts(&core.store, &core.profile_id, p.limit.clamp(1, 1000))
            .map_err(store_error)?;
        Ok(json!({ "concepts": concepts }))
    });
}
