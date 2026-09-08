//! Pages: the vault, in three reads and two writes.
//!
//! Doc 16 section 3.7's rail item. The explorer lists, the editor reads one and
//! writes it back, and an unresolved link creates the page it names.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_store::repo;

use crate::core::Core;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    r.register("page.list", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Limit {
            #[serde(default = "default_pages")]
            limit: i64,
        }
        fn default_pages() -> i64 {
            200
        }
        let p: Limit = params(p)?;
        let pages = repo::list_pages(&core.store, &core.profile_id, p.limit).map_err(store_error)?;
        let rows: Vec<Value> = pages
            .iter()
            .map(|page| {
                json!({
                    "id": page.id,
                    "title": page.title,
                    "file_path": page.file_path,
                    "updated_at": page.updated_at,
                    // Doc 16 section 3.2: a page saved from a card carries the
                    // card's citations, and the explorer says which pages those
                    // are without opening them.
                    "from_card": page.source_card_id.is_some(),
                    "citations_carried": page
                        .citations_carried
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or(0),
                })
            })
            .collect();
        Ok(json!({ "pages": rows }))
    });

    r.register("page.get", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            page_id: String,
        }
        let p: Which = params(p)?;
        let page = repo::read_page(&core.store, &p.page_id)
            .map_err(store_error)?
            .ok_or_else(|| RpcError::core("page_missing", "That page is not in this vault."))?;

        // Doc 16 section 2.1: backlinks are a query over PageLink, never a scan
        // over bodies.
        let backlinks: Vec<Value> = repo::backlinks(&core.store, "page", &p.page_id)
            .map_err(store_error)?
            .into_iter()
            .map(|b| {
                json!({
                    "page_id": b.page_id,
                    "title": b.page_title,
                    "display_text": b.display_text,
                    "position": b.position,
                })
            })
            .collect();

        Ok(json!({
            "id": page.id,
            "title": page.title,
            "body": page.body,
            "file_path": page.file_path,
            "updated_at": page.updated_at,
            "source_card_id": page.source_card_id,
            "citations_carried": page.citations_carried,
            "links": repo::page_links(&core.store, &p.page_id).map_err(store_error)?,
            "backlinks": backlinks,
        }))
    });

    r.register("page.write", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Write {
            #[serde(default)]
            page_id: Option<String>,
            title: String,
            #[serde(default)]
            body: String,
        }
        let p: Write = params(p)?;
        let pack_id = core.active_pack_id().map_err(core_error)?;
        let profile_id = core.profile_id.clone();
        let id = crate::vault::write_page(
            &mut core.store,
            &profile_id,
            Some(&pack_id),
            p.page_id.as_deref(),
            &p.title,
            &p.body,
        )
        .map_err(page_error)?;
        let page = repo::read_page(&core.store, &id).map_err(store_error)?;
        Ok(json!({
            "page_id": id,
            "title": page.as_ref().map(|p| p.title.clone()),
            "file_path": page.map(|p| p.file_path),
        }))
    });

    r.register("page.delete", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            page_id: String,
        }
        let p: Which = params(p)?;
        crate::vault::delete_page(&mut core.store, &p.page_id).map_err(store_error)?;
        Ok(json!({ "page_id": p.page_id, "deleted": true }))
    });

    // Doc 16 section 3.1: "Unresolved links create the page on click."
    r.register("page.create_from_link", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct FromLink {
            title: String,
        }
        let p: FromLink = params(p)?;
        let pack_id = core.active_pack_id().map_err(core_error)?;
        let profile_id = core.profile_id.clone();
        let title = p.title.trim().to_string();
        let body = format!("# {title}\n\n");
        let id = crate::vault::write_page(&mut core.store, &profile_id, Some(&pack_id), None, &title, &body)
            .map_err(page_error)?;
        Ok(json!({ "page_id": id, "title": title }))
    });
}
