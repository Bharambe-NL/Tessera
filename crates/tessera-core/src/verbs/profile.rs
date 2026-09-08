//! The profile: what is set up, what is watched, and where the keys go.
//!
//! Doc 11 section 6's Profile page, doc 10 section 15's backup, and doc 12
//! principle 4's packs, which are data a profile reads rather than code it runs.

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_doctrine::PackLibrary;
use tessera_store::repo;

use crate::core::Core;
use crate::pipeline;
use crate::rpc::{Router, RpcError, params};
use crate::verbs::support::*;

pub(super) fn register(r: &mut Router<Core>) {
    // Restore is deliberately not a method here. It replaces the database the
    // running core is holding open, so a core cannot perform one on itself:
    // the shell closes the core, calls `tessera_bundle::restore` against a
    // folder, and opens a core on it. Registering a `profile.restore` that
    // half worked would be worse than not having one, because the failure
    // would land on someone whose database is already damaged.
    //
    // Doc 10 section 15. The profile folder is the unit, and the shell writes
    // the file wherever the person chose, so the core hands back bytes rather
    // than taking a path: a core that wrote to a path a caller named would be
    // a file writer with the whole disk in reach.
    r.register("profile.back_up", |core: &mut Core, _p| {
        let mut archive = std::io::Cursor::new(Vec::new());
        let manifest = tessera_bundle::back_up(&core.store, &mut archive)
            .map_err(|e| RpcError::core("backup", e.to_string()))?;
        Ok(json!({
            "manifest": manifest,
            "bytes": pipeline::base64(&archive.into_inner()),
        }))
    });

    // Doc 10 section 11. The summary is returned beside the bytes so the shell
    // can show what is in the file before anyone sends it: this is the one
    // export whose recipient is a stranger, and a person deserves to see what
    // they are handing over.
    r.register("profile.diagnostics", |core: &mut Core, _p| {
        let mut archive = std::io::Cursor::new(Vec::new());
        let summary = tessera_bundle::diagnostics(&core.store, &mut archive)
            .map_err(|e| RpcError::core("diagnostics", e.to_string()))?;
        Ok(json!({
            "summary": summary,
            "bytes": pipeline::base64(&archive.into_inner()),
        }))
    });

    // Doc 16 section 3.1's two way mirror, on demand. The shell calls it when
    // the window regains focus, which is when a person is most likely to have
    // just edited a page in another app.
    r.register("vault.sync", |core: &mut Core, _| {
        let report = core.sync_vault().map_err(core_error)?;
        Ok(json!({
            "written": report.written,
            "adopted": report.adopted,
            "created": report.created,
            "conflicts": report.conflicts,
            "agreed": report.agreed,
            // Doc 05 section 11's posture, for files rather than documents: a
            // thing that could not be taken in is named where it can be fixed.
            "skipped": report
                .skipped
                .iter()
                .map(|(path, reason)| json!({ "path": path, "reason": reason }))
                .collect::<Vec<_>>(),
        }))
    });

    r.register("profile.get", |core: &mut Core, _| {
        // Doc 11 section 6's Profile pages, in one read: Context, Models,
        // Retrievers, Doctrine, Diagnostics. Every one of them is a projection
        // of state the core already holds, so this is a read and not five.
        //
        // Doc 10 section 8 and the standing constraint: a key lives in the OS
        // keychain and is never printed, logged or passed as an argument. So
        // `aliases` says which key_ref each alias wants and whether the keychain
        // has it, and nothing here can say what it is.
        let aliases: Vec<Value> = core
            .policy
            .aliases
            .iter()
            .map(|(name, alias)| {
                json!({
                    "alias": name,
                    "provider": alias.provider,
                    "model": alias.model,
                    "key_ref": alias.key_ref,
                    "key_present": core.keys.has(&alias.key_ref),
                })
            })
            .collect();

        let retrievers: Vec<Value> = core
            .packs
            .get(&core.pack_code)
            .map(|pack| {
                pack.retrievers
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "enabled_by_default": r.enabled_by_default,
                            "configured": core.retrievers.configured(&r.id),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let counts = repo::profile_counts(&core.store, &core.profile_id).map_err(store_error)?;

        // Doc 10 section 9: a built in pack is the same on every machine and an
        // imported one is not, so the Doctrine page says which is which rather
        // than listing codes that all look alike.
        let packs: Vec<Value> = core
            .packs
            .codes()
            .map(|code| {
                json!({
                    "code": code,
                    "built_in": PackLibrary::is_built_in(code),
                    "active": code == core.pack_code,
                })
            })
            .collect();
        let pack_problems: Vec<Value> = core
            .packs
            .problems()
            .iter()
            .map(|p| json!({ "file": p.file, "detail": p.detail }))
            .collect();

        Ok(json!({
            "profile_id": core.profile_id,
            "packs": core.packs.codes().collect::<Vec<_>>(),
            "pack_details": packs,
            "pack_problems": pack_problems,
            "active_pack": core.pack_code,
            "provider": core.provider.id(),
            "policy": serde_json::to_value(&core.policy).unwrap_or(Value::Null),
            "aliases": aliases,
            "retrievers": retrievers,
            "diagnostics": counts,
        }))
    });

    // Doc 11 section 6's First run: choose a pack, add a model key, optionally
    // a folder.
    //
    // One read that answers "is anything set up yet", rather than the shell
    // inferring it from `profile.get`. Inferring would put the definition of a
    // first run in the shell, where a second shell would define it differently
    // and one of them would show the setup screen to someone who had already
    // finished it.
    r.register("profile.first_run", |core: &mut Core, _| {
        let boards: i64 = core
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM board WHERE profile_id = ?1",
                rusqlite::params![core.profile_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let folders: i64 = core
            .store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM watched_folder WHERE profile_id = ?1",
                rusqlite::params![core.profile_id],
                |r| r.get(0),
            )
            .unwrap_or(0);

        // A key for the alias that answers a deep card, which is what doc 12
        // phase 11's acceptance measures. Any key would let the app start; this
        // one is what lets it do the thing a person installed it for.
        let needed: Vec<&str> = core.policy.aliases.values().map(|a| a.key_ref.as_str()).collect();
        let have = needed.iter().any(|r| core.keys.has(r));

        Ok(json!({
            // Setup is finished when a key is in place. A pack is always set
            // (the profile has a default), and a folder is optional by doc 11
            // section 6, so neither can be what the question turns on.
            "needs_setup": !have,
            "has_key": have,
            "boards": boards,
            "folders": folders,
            "packs": core.packs.codes().collect::<Vec<_>>(),
            "active_pack": core.pack_code,
            "key_refs": needed,
        }))
    });

    // Doc 12 principle 4: packs are data. Choosing one is choosing which file
    // the profile reads, not editing a rule.
    // Doc 10 section 9 and doc 12 principle 4: a pack is data, so taking one in
    // is reading a file and validating it, never running it.
    //
    // A path rather than the pack's text, to match the folder step: the shell
    // knows where the person pointed and the core is what may read it. The file
    // is copied into the profile folder, so a pack that a board pinned cannot
    // later move or change under it.
    r.register("pack.import", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Import {
            path: String,
        }
        let p: Import = params(p)?;
        let path = std::path::Path::new(p.path.trim());
        if p.path.trim().is_empty() {
            return Err(RpcError::core("no_pack", "Choose a doctrine pack file."));
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|e| RpcError::core("no_pack", format!("That pack file could not be read: {e}")))?;
        let summary = core.import_pack(&raw).map_err(core_error)?;
        Ok(summary)
    });

    r.register("profile.set_pack", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct SetPack {
            code: String,
        }
        let p: SetPack = params(p)?;
        core.use_pack(&p.code).map_err(core_error)?;
        Ok(json!({ "active_pack": core.pack_code }))
    });

    // Doc 11 section 6's optional folder, and doc 10 section 16's requirement
    // that the Retrievers page say per folder whether chunk text leaves the
    // machine. `sensitive` and `embeddings` are set here rather than after,
    // because a folder indexed once with provider embeddings has already sent
    // its text and a later toggle cannot take it back.
    // Doc 05 section 8.1: the web retriever reads from where the profile points
    // it, and nowhere else. A base rather than a search engine, because there
    // is no search key in this build and a retriever pointed at nothing is
    // honest about it.
    r.register("profile.watch_web", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Seed {
            url: String,
        }
        let p: Seed = params(p)?;
        let url = p.url.trim().to_string();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(RpcError::core(
                "bad_url",
                "A web source starts with http:// or https://.",
            ));
        }

        let raw: String = core
            .store
            .conn()
            .query_row(
                "SELECT retriever_config FROM profile WHERE id = ?1",
                rusqlite::params![core.profile_id],
                |r| r.get(0),
            )
            .map_err(|e| RpcError::core("profile", e.to_string()))?;
        let mut config: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
        let mut seeds: Vec<String> = config["web_seeds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        if !seeds.contains(&url) {
            seeds.push(url.clone());
        }
        config["web_seeds"] = json!(seeds);
        core.store
            .conn()
            .execute(
                "UPDATE profile SET retriever_config = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![config.to_string(), tessera_store::now_iso8601(), core.profile_id],
            )
            .map_err(|e| RpcError::core("profile", e.to_string()))?;

        core.rebuild_retrievers().map_err(core_error)?;
        Ok(json!({ "web_seeds": config["web_seeds"], "configured": core.retrievers.configured("web") }))
    });

    r.register("profile.watch_folder", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Watch {
            root: String,
            label: String,
            #[serde(default)]
            sensitive: bool,
            #[serde(default)]
            provider_embeddings: bool,
        }
        let p: Watch = params(p)?;
        if p.root.trim().is_empty() {
            return Err(RpcError::core("no_folder", "Choose a folder to watch."));
        }
        if !std::path::Path::new(p.root.trim()).is_dir() {
            return Err(RpcError::core(
                "no_folder",
                "That folder does not exist on this machine.",
            ));
        }
        // Doc 05 section 7 and doc 10 section 16: a sensitive folder keeps its
        // text local, so asking for provider embeddings on one is a
        // contradiction rather than a preference to honour quietly.
        if p.sensitive && p.provider_embeddings {
            return Err(RpcError::core(
                "sensitive_folder",
                "A sensitive folder keeps its text on this machine, so it cannot use provider embeddings.",
            ));
        }

        let id = tessera_store::new_id();
        core.store
            .conn()
            .execute(
                "INSERT INTO watched_folder (id, profile_id, root, label, sensitive, embeddings,
                     created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    id,
                    core.profile_id,
                    p.root.trim(),
                    p.label.trim(),
                    i64::from(p.sensitive),
                    if p.provider_embeddings {
                        "provider"
                    } else {
                        "local"
                    },
                    tessera_store::now_iso8601()
                ],
            )
            .map_err(|e| RpcError::core("store", e.to_string()))?;

        // Doc 05 section 8.2: adding a folder is adding its documents, so the
        // walk happens here rather than at the first card. A folder that is
        // watched and not indexed answers nothing, and nothing is what an
        // empty corpus and an unread one both look like from the card.
        //
        // No embedder: the local model is a download the app does not ship, so
        // this indexes the lexical half. Doc 05 section 11 puts parse errors on
        // the Retrievers page, which is why they are returned rather than
        // logged.
        let exclude = core
            .packs
            .get(&core.pack_code)
            .map(|pack| pack.must_exclude())
            .unwrap_or_default();
        let report = tessera_retrievers::index_folder(
            core.store.conn(),
            &core.profile_id,
            &id,
            p.label.trim(),
            std::path::Path::new(p.root.trim()),
            &exclude,
            None,
        )
        .map_err(|e| RpcError::core("store", e.to_string()))?;

        // `index.folder_added.v1` has been in the vocabulary since M2 and had no
        // writer until the folder was actually read. Doc 05 section 11's page
        // reads the log for what each folder holds.
        let profile_id = core.profile_id.clone();
        core.store
            .append(tessera_store::event::NewEvent::new(
                "index.folder_added.v1",
                json!({
                    "profile_id": profile_id,
                    "folder_id": id,
                    "label": p.label.trim(),
                    "sensitive": p.sensitive,
                    "embeddings": if p.provider_embeddings { "provider" } else { "local" },
                    "indexed": report.indexed,
                    "chunks": report.chunks,
                    "excluded": report.excluded,
                    "unreadable": report.errors.len(),
                }),
                tessera_store::event::Provenance::user(),
            ))
            .map_err(store_error)?;

        // The set is what the fan-out reads, and the folder just added is in it
        // now rather than after a restart.
        core.rebuild_retrievers().map_err(core_error)?;

        Ok(json!({
            "folder_id": id,
            "label": p.label.trim(),
            "sensitive": p.sensitive,
            // Doc 10 section 16's sentence, as data the Retrievers page renders
            // rather than a string it composes: two screens composing it would
            // one day disagree about which folders send text.
            "text_leaves_machine": p.provider_embeddings,
            "indexed": report.indexed,
            "chunks": report.chunks,
            "excluded": report.excluded,
            "errors": report
                .errors
                .iter()
                .map(|(path, kind, detail)| json!({
                    "path": path,
                    "kind": kind,
                    "detail": detail,
                }))
                .collect::<Vec<_>>(),
        }))
    });

    // Doc 11 section 6's Models page, which is what retires the `tessera-keys`
    // CLI. The secret goes straight to the keychain: it is never written to the
    // store, never logged, and never echoed back, so the only answer this gives
    // is whether the keychain took it.
    r.register("profile.set_key", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct SetKey {
            key_ref: String,
            secret: String,
        }
        let p: SetKey = params(p)?;
        if p.secret.trim().is_empty() {
            return Err(RpcError::core("empty_key", "Paste the key, then save it."));
        }
        core.keys
            .set(&p.key_ref, p.secret.trim())
            .map_err(|e| RpcError::core("keychain", e.to_string()))?;
        // A live core rebuilds its provider on the spot, so the first key makes
        // the product answer without a restart and a second provider's key
        // becomes routable the moment it is saved.
        if core.live_refresh {
            core.provider = tessera_providers::live_provider(core.keys.as_ref(), &core.policy);
        }
        Ok(json!({ "key_ref": p.key_ref, "key_present": true }))
    });
}
