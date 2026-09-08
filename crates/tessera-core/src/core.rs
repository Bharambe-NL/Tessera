//! The core's state and the work every registered method calls into.
//!
//! Doc 10 section 2: one core, several shells. Everything the desktop shell can
//! do is a method on the router, so the web client that arrives later talks to
//! the identical protocol over a socket rather than to a second, thinner API.
//!
//! The methods themselves are in [`crate::verbs`], one module per noun.

use rusqlite::OptionalExtension;
use std::sync::Arc;

use serde_json::{Value, json};
use tessera_doctrine::PackLibrary;
use tessera_providers::{KeyStore, MemoryKeyStore, ModelPolicy, ModelProvider, ResolvedPolicy, resolve};
use tessera_schema::Registry;
use tessera_store::{Source, Store, repo};

use crate::pipeline::{self, RunContext};

/// The registered surface lives in [`crate::verbs`] now, one module per noun.
/// The path stays here, because the shell and both binaries import it from this
/// module and a move is not a rename.
pub use crate::verbs::build_router;

/// The stages a card run needs resolved before it starts. Doc 03 section 8.3
/// fails before any retrieval rather than discovering a missing key halfway.
const CARD_STAGES: &[&str] = &["route", "plan", "retrieve", "synthesize", "visualize", "verify"];

/// Where imported doctrine packs live, under the profile folder. Doc 12
/// principle 4: a pack is a file, and this is the one the profile owns.
const IMPORTED_PACKS: &str = "packs";

/// Where a new card hangs from. Doc 01 section 4.4.
///
/// A struct rather than three parameters because the three are one decision:
/// they select the card's kind between them, and passing them separately let
/// `ask_on` be called with an anchor and no parent, which names a span on no
/// card.
#[derive(Debug, Default, Clone, Copy)]
pub struct Anchor<'a> {
    pub parent_card_id: Option<&'a str>,
    /// The highlighted span, for doc 09's highlight to branch verb.
    pub anchor_text: Option<&'a str>,
    /// A JSON pointer into the parent visual's payload, for block investigate.
    pub anchor_block_ref: Option<&'a str>,
    /// Doc 16 section 3.4: the learner asked this one notebook question to
    /// reach the web as well. Off everywhere else, because the web is a
    /// per question choice a person makes rather than a mode a board is in.
    pub with_web: bool,
}

impl<'a> Anchor<'a> {
    /// A plain follow-up: the parent's chain is the context, nothing is anchored.
    pub fn on(parent_card_id: &'a str) -> Self {
        Self {
            parent_card_id: Some(parent_card_id),
            ..Self::default()
        }
    }

    pub(crate) fn anchored(&self) -> bool {
        self.anchor_text.is_some() || self.anchor_block_ref.is_some()
    }

    /// Doc 01 section 4.4's card kinds, as the anchor decides them.
    fn kind(&self) -> &'static str {
        match (self.parent_card_id.is_some(), self.anchored()) {
            (true, true) => "branch",
            (true, false) => "follow",
            // An anchor without a parent names a span on no card, which the RPC
            // boundary rejects before this is reached.
            (false, _) => "root",
        }
    }
}

pub struct Core {
    pub store: Store,
    pub registry: Registry,
    pub packs: PackLibrary,
    pub keys: Box<dyn KeyStore>,
    pub provider: Arc<dyn ModelProvider>,
    pub policy: ModelPolicy,
    pub profile_id: String,
    pub source: Source,
    /// Which doctrine pack this profile's boards pin. Doc 01 section 4.1: a
    /// board pins a pack version, and doc 02 section 10.1 runs evaluation on
    /// `finance-eu-synthetic` rather than on whatever ships as the default.
    pub pack_code: String,
    /// Doc 10 section 6's work ledger. One per profile, because the limits it
    /// holds are per profile: three runs, six retriever assignments, one
    /// Verifier per board.
    pub ledger: tessera_harness::Ledger,
    /// What this profile can retrieve from. Empty until a folder is watched,
    /// so a fresh profile answers "no sources" honestly rather than emptily.
    pub retrievers: crate::retrieval::RetrieverSet,
    /// Agent work is async; the RPC surface is not. The core owns the runtime so
    /// a handler can block on a card run without the shell needing to know.
    runtime: tokio::runtime::Runtime,
    /// True only for a core opened with `open_live`. A saved key then rebuilds
    /// the provider from the keychain on the spot, so adding the first key
    /// makes the product answer without a restart. Test and eval cores keep
    /// whatever provider they were handed.
    pub(crate) live_refresh: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("store: {0}")]
    Store(#[from] tessera_store::StoreError),
    #[error("schema: {0}")]
    Schema(#[from] tessera_schema::SchemaError),
    #[error("doctrine: {0}")]
    Doctrine(#[from] tessera_doctrine::DoctrineError),
    #[error("provider: {0}")]
    Provider(#[from] tessera_providers::ProviderError),
    #[error("runtime: {0}")]
    Runtime(String),
}

impl Core {
    /// Open a profile folder for live use: the provider is built from the keys
    /// that exist in the keychain, one adapter per provider the policy names,
    /// and saving a key through the RPC rebuilds it. The app binary calls this;
    /// tests and the eval pass their own provider to `open` instead.
    pub fn open_live(
        root: impl AsRef<std::path::Path>,
        keys: Box<dyn KeyStore>,
        key_ref: &str,
    ) -> Result<Self, CoreError> {
        let policy = ModelPolicy::default_anthropic(key_ref);
        let provider = tessera_providers::live_provider(keys.as_ref(), &policy);
        let mut core = Self::open(root, keys, provider, key_ref)?;
        core.live_refresh = true;
        Ok(core)
    }

    /// Open a profile folder and bring the core up.
    pub fn open(
        root: impl AsRef<std::path::Path>,
        keys: Box<dyn KeyStore>,
        provider: Arc<dyn ModelProvider>,
        key_ref: &str,
    ) -> Result<Self, CoreError> {
        let mut store = Store::open(root)?;
        let registry = Registry::load()?;
        let mut packs = PackLibrary::load_built_in(&registry)?;
        // Doc 12 principle 4: packs are data. An imported one is a file in the
        // profile folder, so it loads the same way on every start rather than
        // living only in the session that imported it.
        packs.load_imported(&registry, &store.root().join(IMPORTED_PACKS));

        // Doc 10 section 6: reclaim before anything else, so a crash from the
        // last session is resolved before this one takes work.
        let reclaimed = tessera_harness::Ledger::reclaim_on_start(&mut store)?;
        if !reclaimed.is_empty() {
            tracing::info!(
                count = reclaimed.len(),
                "reclaimed runs abandoned by a previous session"
            );
        }

        let general = packs.get("general")?;
        let pack_id = repo::ensure_pack(&store, &serde_json::to_value(general).unwrap_or(Value::Null))?;
        let policy = ModelPolicy::default_anthropic(key_ref);
        let profile_id = repo::ensure_profile(
            &store,
            &pack_id,
            "fast",
            &serde_json::to_value(&policy).unwrap_or(Value::Null),
        )?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| CoreError::Runtime(e.to_string()))?;

        // The pack the profile last chose, which has to outlive the process.
        // A code the library no longer has (an imported file that was deleted,
        // or one that stopped validating) falls back to general rather than
        // failing to open the profile, and the Doctrine page says which packs
        // did not load.
        let pack_code = repo::active_pack_code(&store)?
            .filter(|code| packs.get(code).is_ok())
            .unwrap_or_else(|| "general".to_string());

        let mut core = Self {
            store,
            registry,
            packs,
            keys,
            provider,
            policy,
            profile_id,
            source: Source::Live,
            pack_code,
            ledger: tessera_harness::Ledger::new(),
            retrievers: crate::retrieval::RetrieverSet::default(),
            runtime,
            live_refresh: false,
        };
        // Until M14 this was left at `default()` and every production card
        // retrieved from nothing, while the dev server, the eval and the tests
        // each built a set of their own. A profile that has watched a folder
        // and never been told its documents are unreachable is the failure that
        // hides longest, because an honest "no sources found" card is exactly
        // what a working retriever over an empty index looks like.
        core.rebuild_retrievers()?;

        // Doc 16 section 3.1: the file is the export and the row is the index,
        // and either can have moved while the app was closed. No watcher: this
        // call is the unit, here at start, after a page write and from an RPC.
        // A vault nobody has written to reads as an empty folder and costs one
        // failed directory read.
        let report = core.sync_vault()?;
        if report != crate::vault::SyncReport::default() {
            tracing::info!(
                written = report.written,
                adopted = report.adopted,
                created = report.created,
                conflicts = report.conflicts,
                "reconciled the vault with the pages"
            );
        }
        Ok(core)
    }

    /// Reconcile the vault folder with the page rows. Doc 16 section 3.1.
    pub fn sync_vault(&mut self) -> Result<crate::vault::SyncReport, CoreError> {
        let pack_id = self.active_pack_id()?;
        let profile_id = self.profile_id.clone();
        Ok(crate::vault::sync(&mut self.store, &profile_id, Some(&pack_id))?)
    }

    /// Rebuild what this profile can retrieve from.
    ///
    /// Called wherever one of the two inputs changes: the active pack decides
    /// which retrievers are enabled, the watched folders decide whether the
    /// local one has anywhere to read. Cheap enough to redo whole rather than
    /// patch, and a set patched in two places would disagree with the Profile
    /// page about what is configured.
    pub fn rebuild_retrievers(&mut self) -> Result<(), CoreError> {
        let set = {
            let pack = self.packs.get(&self.pack_code)?;
            let folders = repo::watched_folders(&self.store, &self.profile_id)?;
            let seeds = web_seeds(&self.store, &self.profile_id);
            crate::retrieval::assemble(pack, &folders, self.memory_enabled(), &seeds)
        };
        self.retrievers = set;
        Ok(())
    }

    /// Doc 15 section 6's profile switch. Absent reads as on, which is what the
    /// column's default says and what a profile written before memory existed
    /// means.
    fn memory_enabled(&self) -> bool {
        self.store
            .conn()
            .query_row(
                "SELECT memory_enabled FROM profile WHERE id = ?1",
                [&self.profile_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|v| v != 0)
            .unwrap_or(true)
    }

    /// An in memory core for tests, with a keystore holding one fake key.
    pub fn in_memory(provider: Arc<dyn ModelProvider>) -> Result<Self, CoreError> {
        Self::in_memory_with_keys(
            provider,
            Box::new(MemoryKeyStore::with("test-key", "sk-test")),
            "test-key",
        )
    }

    /// A throwaway profile against a real keystore.
    ///
    /// The eval harness needs this: it resolves policies naming several
    /// providers' key_refs, and a core holding only a fake one refuses every
    /// stage with `policy_unresolvable` before a single call goes out.
    pub fn in_memory_with_keys(
        provider: Arc<dyn ModelProvider>,
        keys: Box<dyn KeyStore>,
        key_ref: &str,
    ) -> Result<Self, CoreError> {
        let root = std::env::temp_dir().join(format!("tessera-core-{}", tessera_store::new_id()));
        Self::open(root, keys, provider, key_ref)
    }

    fn resolved(&self) -> Result<ResolvedPolicy, CoreError> {
        Ok(resolve(&self.policy, self.keys.as_ref(), CARD_STAGES)?)
    }

    /// Point the core at a different provider and the policy that names its
    /// models.
    ///
    /// The two move together on purpose. A provider swapped without its policy
    /// would be sent model ids it has never heard of, and the failure would
    /// arrive as a bad request rather than as the configuration error it is.
    pub fn use_provider(&mut self, provider: Arc<dyn ModelProvider>, policy: ModelPolicy) {
        self.provider = provider;
        self.policy = policy;
    }

    /// Use a different doctrine pack for boards created from here on.
    ///
    /// Existing boards keep the pack version they pinned, which is what doc 10
    /// section 9 requires: "a pack update never rewrites a board's pinned
    /// version".
    pub fn use_pack(&mut self, code: &str) -> Result<(), CoreError> {
        // Fail here rather than at the first card, so a typo in a pack code is
        // a startup error and not a run that quietly used the wrong rules.
        self.packs.get(code)?;
        let was = std::mem::replace(&mut self.pack_code, code.to_string());

        // Persisted, because the choice belongs to the profile rather than to
        // the process that happened to be running when it was made.
        let pack_id = self.active_pack_id()?;
        repo::set_active_pack(&self.store, &self.profile_id, &pack_id)?;
        if was != self.pack_code {
            let profile_id = self.profile_id.clone();
            self.store.append(tessera_store::event::NewEvent::new(
                "pack.activated.v1",
                json!({
                    "profile_id": profile_id,
                    "pack_code": self.pack_code,
                    "pack_id": pack_id,
                    "previous_pack_code": was,
                }),
                tessera_store::event::Provenance::user(),
            ))?;
        }

        // The pack is what says which retrievers are enabled, so the set it
        // enables changes with it.
        self.rebuild_retrievers()?;
        Ok(())
    }

    /// Take a doctrine pack file into this profile. Doc 10 section 9.
    ///
    /// Validation is the registry's, against `schemas/pack/doctrine-pack.v1.json`,
    /// so an imported pack is held to what the shipped ones are held to. The
    /// file is copied into the profile folder rather than referenced where it
    /// was found: a pack read from a path outside the profile would change
    /// under the boards that pinned it, or vanish.
    ///
    /// Importing does not activate. Doc 10 section 9 keeps a pack change a
    /// deliberate act, and an import that silently re-judged the next card
    /// would be one.
    pub fn import_pack(&mut self, raw: &str) -> Result<Value, CoreError> {
        let pack = tessera_doctrine::DoctrinePack::parse(&self.registry, raw)?;

        // A built in code is not a name a file may take: boards pin the pack
        // they were judged under by code and version, and a file that renamed
        // `general` would change what every one of them claims.
        if PackLibrary::is_built_in(&pack.code) {
            return Err(CoreError::Doctrine(tessera_doctrine::DoctrineError::Malformed {
                code: pack.code.clone(),
                detail: "a pack that ships with the app already uses this code".to_string(),
            }));
        }

        let dir = self.store.root().join(IMPORTED_PACKS);
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::Runtime(e.to_string()))?;
        std::fs::write(dir.join(format!("{}.json", pack.code)), raw)
            .map_err(|e| CoreError::Runtime(e.to_string()))?;

        let value = serde_json::to_value(&pack).unwrap_or(Value::Null);
        let pack_id = repo::ensure_pack(&self.store, &value)?;
        let summary = json!({
            "code": pack.code,
            "version": pack.version,
            "name": pack.name,
            "pack_id": pack_id,
            "audiences": pack.audiences.len(),
            "flag_rules": pack.flag_rules.len(),
            "source_ranks": pack.source_hierarchy.len(),
            "retrievers": pack.retrievers.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "built_in": false,
            "active": false,
        });
        self.store.append(tessera_store::event::NewEvent::new(
            "pack.imported.v1",
            summary.clone(),
            tessera_store::event::Provenance::user(),
        ))?;
        self.packs.add(pack);
        Ok(summary)
    }

    /// The pack a board is judged by: the one it pinned, not the one the
    /// profile happens to be using now.
    ///
    /// Doc 10 section 9: "a pack update never rewrites a board's pinned
    /// version". Re-verification read `self.pack_code` until M14.4, so a person
    /// who switched packs and reopened an old board had it re-judged by rules
    /// it was never written under, while the board went on naming the pack it
    /// pinned. The version can still move under the code (that is what the
    /// board's update pack verb is for); the code cannot.
    ///
    /// A pinned code the library no longer has, an imported pack whose file was
    /// deleted, falls back to the profile's pack rather than refusing to
    /// re-verify: judging by today's rules and saying so beats not judging.
    fn pack_for_board(&self, board_id: &str) -> Result<tessera_doctrine::DoctrinePack, CoreError> {
        let pinned = repo::board_pack(&self.store, board_id)?;
        let code = pinned
            .as_ref()
            .map(|p| p.code.clone())
            .filter(|code| self.packs.get(code).is_ok())
            .unwrap_or_else(|| self.pack_code.clone());
        Ok(self.packs.get(&code)?.clone())
    }

    /// Whether a newer version of the pack this board pinned is loaded.
    ///
    /// The library holds one version per code, the one that ships or the one
    /// that was imported, so "newer" here means "not the version the board
    /// names". Ordering two version strings would claim more than the pack file
    /// says.
    pub fn board_pack_update(&self, board_id: &str) -> Result<Value, CoreError> {
        let Some(pinned) = repo::board_pack(&self.store, board_id)? else {
            return Ok(json!({ "available": false }));
        };
        let Ok(current) = self.packs.get(&pinned.code) else {
            // The pack the board names is not loaded at all. Nothing to update
            // to, and the Doctrine page is where a missing file is explained.
            return Ok(json!({
                "available": false,
                "pack_code": pinned.code,
                "pinned_version": pinned.version,
                "pack_loaded": false,
            }));
        };
        Ok(json!({
            "available": current.version != pinned.version,
            "pack_code": pinned.code,
            "pinned_version": pinned.version,
            "current_version": current.version,
            "pack_loaded": true,
        }))
    }

    /// Doc 10 section 9's "update pack", which reruns `verify_only`.
    ///
    /// The board moves to the loaded version of the pack it already pinned, and
    /// every card that has an answer is re-judged under the new rules. Each one
    /// records why it was re-run, so a card that flips to flagged months later
    /// can be traced to the pack version that flipped it rather than looking
    /// like the Verifier changed its mind.
    pub fn update_board_pack(&mut self, board_id: &str) -> Result<Value, CoreError> {
        let Some(pinned) = repo::board_pack(&self.store, board_id)? else {
            return Err(CoreError::Runtime("no such board".to_string()));
        };
        let pack = self.packs.get(&pinned.code)?.clone();
        if pack.version == pinned.version {
            return Ok(json!({
                "board_id": board_id,
                "pack_code": pinned.code,
                "from_version": pinned.version,
                "to_version": pack.version,
                "updated": false,
                "cards": [],
            }));
        }

        let to = repo::PinnedPack {
            pack_id: repo::ensure_pack(&self.store, &serde_json::to_value(&pack).unwrap_or(Value::Null))?,
            code: pack.code.clone(),
            version: pack.version.clone(),
        };
        let cards = repo::cards_to_reverify(&self.store, board_id)?;
        repo::repin_board(&mut self.store, board_id, &to, &pinned.version, cards.len())?;

        let mut outcomes = Vec::new();
        for card_id in cards {
            // `card.rerun.v1` has been in the vocabulary since M2 with no
            // writer. This is what it is for: the card was not asked again, it
            // was judged again, and the payload says by what.
            self.store.append(
                tessera_store::event::NewEvent::new(
                    "card.rerun.v1",
                    json!({
                        "card_id": card_id,
                        "kind": "verify_only",
                        "reason": "pack_updated",
                        "pack_code": to.code,
                        "from_version": pinned.version,
                        "to_version": to.version,
                    }),
                    tessera_store::event::Provenance::user(),
                )
                .on_board(board_id)
                .on_card(&card_id),
            )?;

            match self.verify_card(board_id, &card_id) {
                Ok(outcome) => outcomes.push(json!({
                    "card_id": outcome.card_id,
                    "status": outcome.status,
                    "flags": outcome.flags,
                })),
                // Doc 07 fails closed, and one card that could not be re-judged
                // does not stop the rest of the board being re-judged. What
                // happened is in the log either way.
                Err(e) => outcomes.push(json!({
                    "card_id": card_id,
                    "status": "not_reverified",
                    "detail": e.to_string(),
                })),
            }
        }

        Ok(json!({
            "board_id": board_id,
            "pack_code": to.code,
            "from_version": pinned.version,
            "to_version": to.version,
            "updated": true,
            "cards": outcomes,
        }))
    }

    /// The stored id of the active doctrine pack, writing it if it is not there.
    ///
    /// A caller building rows that reference a pack needs the id the store
    /// knows, and `pack_code` is the pack's name rather than its row.
    pub fn active_pack_id(&self) -> Result<String, CoreError> {
        let pack = self.packs.get(&self.pack_code)?;
        Ok(repo::ensure_pack(
            &self.store,
            &serde_json::to_value(pack).unwrap_or(Value::Null),
        )?)
    }

    /// Create a board on the active pack.
    pub fn create_board(&mut self, title: &str, depth: &str) -> Result<String, CoreError> {
        let pack_id = self.active_pack_id()?;
        let profile_id = self.profile_id.clone();
        Ok(repo::create_board(
            &mut self.store,
            repo::NewBoard {
                profile_id: &profile_id,
                title,
                doctrine_pack_id: &pack_id,
                default_depth: depth,
                named_by_user: false,
                parent_board_id: None,
                seed_label: None,
                context: None,
            },
        )?)
    }

    /// Doc 17 section 6: the Map is "a board of `mode: map`", one per profile,
    /// created the first time anything needs it.
    ///
    /// A board rather than a view of its own, so viewport, events and export
    /// come free; and one per profile, because a second map would be a second
    /// answer to where the learner stands.
    pub fn map_board(&mut self) -> Result<String, CoreError> {
        let existing: Option<String> = self
            .store
            .conn()
            .query_row(
                "SELECT id FROM board WHERE profile_id = ?1 AND mode = 'map' LIMIT 1",
                rusqlite::params![self.profile_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(tessera_store::StoreError::from)?;
        if let Some(existing) = existing {
            return Ok(existing);
        }
        let board_id = self.create_board(MAP_TITLE, "deep")?;
        self.store
            .conn()
            .execute(
                "UPDATE board SET mode = 'map', named_by_user = 1 WHERE id = ?1",
                rusqlite::params![board_id],
            )
            .map_err(tessera_store::StoreError::from)?;
        Ok(board_id)
    }

    /// Load a learning path. Doc 17 section 2.1: "creates or links the
    /// concepts, creates the edges as confirmed, and offers a mission".
    ///
    /// Confirmed rather than proposed, and that is the one place an edge starts
    /// there: a path is doctrine somebody wrote down, not a guess an agent
    /// made, and asking the learner to confirm what the pack already states
    /// would be asking them to check the author's work.
    pub fn load_path(&mut self, path: &Value) -> Result<Value, CoreError> {
        let pack_id = self.active_pack_id()?;
        let profile_id = self.profile_id.clone();
        let path_id = tessera_store::new_id();

        let mut ids: std::collections::BTreeMap<String, String> = Default::default();
        for concept in path["concepts"].as_array().into_iter().flatten() {
            let Some(term) = concept["concept_term"]
                .as_str()
                .or_else(|| concept["term"].as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())
            else {
                continue;
            };
            let id = repo::ensure_concept(&mut self.store, &profile_id, &pack_id, term)?;
            ids.insert(term.to_lowercase(), id);
        }

        let mut edges = 0usize;
        for concept in path["concepts"].as_array().into_iter().flatten() {
            let Some(to) = concept["concept_term"]
                .as_str()
                .or_else(|| concept["term"].as_str())
                .and_then(|t| ids.get(&t.trim().to_lowercase()))
                .cloned()
            else {
                continue;
            };
            for prerequisite in concept["prerequisite_terms"].as_array().into_iter().flatten() {
                let Some(from) = prerequisite
                    .as_str()
                    .and_then(|t| ids.get(&t.trim().to_lowercase()))
                    .cloned()
                else {
                    continue;
                };
                repo::propose_edge(
                    &mut self.store,
                    repo::NewEdge {
                        from_concept_id: &from,
                        to_concept_id: &to,
                        relation: "prerequisite_of",
                        proposed_by: "path",
                        status: "confirmed",
                        weight: 1.0,
                    },
                )?;
                edges += 1;
            }
        }

        let concept_ids: Vec<String> = ids.values().cloned().collect();
        self.store.append(tessera_store::event::NewEvent::new(
            "path.loaded.v1",
            json!({
                "path_id": path_id,
                "code": path["code"].clone(),
                "concept_ids": concept_ids,
            }),
            tessera_store::event::Provenance::user(),
        ))?;

        // Doc 17 section 2.1: the path "offers a mission". Offered rather than
        // started: the statement is the pack's template until the learner says
        // why they want it, and a mission nobody meant is a lesson planned
        // against a reason nobody has.
        let mission = path["mission_template"].as_str().map(str::to_string);

        // Doc 17 sections 2.1 and 5: the path names the sources it was written
        // around, and a lesson planned from it reads them first. Offered with
        // the mission rather than stored on the path, because a mission is what
        // a lesson is planned against and a path can be loaded without one.
        let sources_hint: Vec<String> = path["sources_hint"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["locator"].as_str().or_else(|| s.as_str()))
            .map(str::to_string)
            .collect();

        Ok(json!({
            "path_id": path_id,
            "concepts": concept_ids.len(),
            "edges": edges,
            "mission_offered": mission,
            "sources_hint": sources_hint,
        }))
    }

    /// Run the Learning Planner. Doc 17 section 7.
    pub fn plan_learning(&mut self, reason: &str, topic: Option<&str>) -> Result<Value, CoreError> {
        let board_id = self.map_board()?;
        let pack = self.packs.get(&self.pack_code)?.clone();
        let (concepts, edges) = repo::read_map(&self.store, &self.profile_id)?;
        let mission = repo::active_mission(&self.store, &self.profile_id)?;

        let templates = &pack.learning_templates;
        let packet = json!({
            "schema_version": "1.0",
            "run_id": Value::Null,
            "reason": reason,
            "topic": topic,
            "mission": mission,
            "concepts": concepts,
            "edges": edges,
            "doctrine": {
                "mastered_at": templates.mastered_at,
                "decay_days": templates.decay_days("default"),
                "max_new_concepts": MAX_NEW_CONCEPTS,
            }
        });

        let policy = self.resolved()?;
        let policy_snapshot = serde_json::to_value(&self.policy).unwrap_or(Value::Null);
        let run_id = repo::start_run(
            &self.store,
            repo::NewRun {
                board_id: &board_id,
                card_id: None,
                kind: "card",
                depth: None,
                policy_snapshot: &policy_snapshot,
                pack_version: &pack.version,
            },
        )?;

        let mut packet = packet;
        packet["run_id"] = json!(run_id);

        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers: &self.retrievers,
        };

        let out = self
            .runtime
            .handle()
            .clone()
            .block_on(pipeline::run_learning_planner(
                &mut self.store,
                &ctx,
                &board_id,
                &run_id,
                packet,
            ))
            .map_err(|f| CoreError::Runtime(f.to_string()))?;

        // Doc 17 section 9: the frontier is recorded where the Map reads it.
        self.store.append(
            tessera_store::event::NewEvent::new(
                "frontier.computed.v1",
                json!({
                    "reason": reason,
                    "concept_ids": out["frontier"].clone(),
                    "level": out["lesson"]["level"].clone(),
                }),
                tessera_store::event::Provenance::harness("learning_planner", Some(run_id.clone())),
            )
            .on_board(&board_id),
        )?;
        repo::end_run(&self.store, &run_id, "done")?;

        Ok(out)
    }

    /// Ask a question and run the card to completion.
    ///
    /// Blocking on purpose: the RPC surface is synchronous so both the in
    /// process shell and a future socket client see the same contract.
    pub fn ask(
        &mut self,
        board_id: &str,
        question: &str,
        depth_override: Option<&str>,
    ) -> Result<pipeline::CardOutcome, CoreError> {
        self.ask_on(board_id, question, depth_override, None, Anchor::default())
    }

    /// Run one Tutor turn. Doc 14 section 3.3.
    pub fn tutor_turn(
        &mut self,
        board_id: &str,
        stage: &str,
        learner_message: Option<&str>,
        target_card_id: Option<&str>,
    ) -> Result<Value, CoreError> {
        let policy = self.resolved()?;
        let pack = self.packs.get(&self.pack_code)?.clone();
        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers: &self.retrievers,
        };

        self.runtime
            .handle()
            .clone()
            .block_on(pipeline::run_tutor_turn(
                &mut self.store,
                &ctx,
                board_id,
                stage,
                learner_message,
                target_card_id,
            ))
            .map_err(|f| CoreError::Runtime(f.to_string()))
    }

    /// Doc 17 section 5's learning record, written to the vault at lesson end.
    ///
    /// Nothing when the board is not running a lesson, which is what an ordinary
    /// board closing a panel it never opened should produce. A failure to write
    /// is not a failure to end the lesson: the session is the record of what
    /// happened and the page is a copy of it, so the learner keeps the first
    /// even when the second cannot be saved.
    pub fn write_learning_record(&mut self, board_id: &str) -> Result<Option<String>, CoreError> {
        let Some(session) = repo::read_learn_session(&self.store, board_id)? else {
            return Ok(None);
        };
        let mission = repo::active_mission(&self.store, &self.profile_id)?;
        let today = tessera_store::now_iso8601()
            .split('T')
            .next()
            .unwrap_or_default()
            .to_string();
        let record = crate::record::build(&self.store, board_id, &session, &mission, &today)?;

        let pack_id = self.active_pack_id()?;
        let profile_id = self.profile_id.clone();
        let page_id = crate::vault::write_page_in(
            &mut self.store,
            &profile_id,
            Some(&pack_id),
            None,
            &record.folder,
            &record.title,
            &record.body,
            record.citations_carried.clone(),
        )
        .map_err(|e| CoreError::Runtime(e.to_string()))?;

        self.store.append(
            tessera_store::event::NewEvent::new(
                "learning_record.saved.v1",
                json!({
                    "page_id": page_id,
                    "mission_id": mission["mission_id"].clone(),
                    "covered": record.covered,
                    "checked": record.checked,
                    "remains": record.remains,
                    "citations_carried": record
                        .citations_carried
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or(0),
                    // Doc 17 section 10's traceability gate reads this. One
                    // entry per line of the page, naming the row it came from,
                    // so the claim "every line is something the session
                    // recorded" is checkable by something that never saw the
                    // generator and never parses the markdown back.
                    "lines": record.lines,
                }),
                tessera_store::event::Provenance::user(),
            )
            .on_board(board_id),
        )?;
        Ok(Some(page_id))
    }

    /// Doc 17 section 3: whether this topic is a concept the learner claimed
    /// and nobody has checked.
    ///
    /// A lesson on one of those opens with a check, because placement recorded
    /// a claim and only a check turns a claim into evidence. Resolved by term,
    /// which is what a lesson's topic is and what the Map's Start lesson hands
    /// over; a topic that names no concept is somebody learning something new,
    /// and there is no claim to test.
    ///
    /// Never an error: a lesson that could not read the map still starts, with
    /// doc 14's intake, which is what every lesson did before this rule.
    pub(crate) fn claimed_but_unchecked(&self, topic: &str) -> bool {
        let Ok((rows, _)) = repo::read_map(&self.store, &self.profile_id) else {
            return false;
        };
        let mastered_at = self
            .packs
            .get(&self.pack_code)
            .map(|p| p.learning_templates.mastered_at)
            .unwrap_or(0.8);
        let wanted = topic.trim().to_lowercase();
        tessera_agents::learning::concepts_from(&rows)
            .iter()
            .filter(|c| c.term.trim().to_lowercase() == wanted)
            .any(|c| tessera_agents::learning::unverified_claim(c, mastered_at))
    }

    /// Grade one check and let doc 17 section 4's ladder decide what follows.
    ///
    /// The rule lives in `tessera-agents`; what this adds is the two things it
    /// needs and cannot read for itself: the pack's mastery threshold and the
    /// map's prerequisite edges.
    pub fn record_check(
        &mut self,
        board_id: &str,
        item: &Value,
        picked: &str,
        concept_ids: &[String],
    ) -> Result<pipeline::Adaptation, CoreError> {
        let pack = self.packs.get(&self.pack_code)?.clone();
        let (_, edge_rows) = repo::read_map(&self.store, &self.profile_id)?;
        let edges = tessera_agents::learning::edges_from(&edge_rows);
        let ladder = pipeline::Ladder {
            mastered_at: pack.learning_templates.mastered_at,
            edges: &edges,
        };
        pipeline::record_check(&mut self.store, board_id, item, picked, concept_ids, &ladder)
            .map_err(|f| CoreError::Runtime(f.to_string()))
    }

    /// Read an image into a card. Doc 07 part A.
    pub fn read_image(&mut self, board_id: &str, image_id: &str) -> Result<pipeline::CardOutcome, CoreError> {
        let policy = self.resolved()?;
        let pack = self.packs.get(&self.pack_code)?.clone();
        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers: &self.retrievers,
        };

        self.runtime
            .handle()
            .clone()
            .block_on(pipeline::run_read(&mut self.store, &ctx, board_id, image_id))
            .map_err(|f| CoreError::Runtime(f.to_string()))
    }

    /// Put an image on a board. Doc 01 section 4.6.
    pub fn add_image(
        &mut self,
        board_id: &str,
        bytes: &[u8],
        mime: &str,
        width: u32,
        height: u32,
    ) -> Result<String, CoreError> {
        Ok(repo::write_image(
            &mut self.store,
            repo::NewImage {
                board_id,
                origin: "pasted",
                bytes,
                mime,
                width,
                height,
                source_ink_ids: None,
            },
        )?)
    }

    /// Turn a board's ink into an image. Doc 12 phase 9's sketch raster path.
    ///
    /// The strokes stay: the raster is a second representation of the same
    /// drawing, made so a vision model has something to look at, and deleting
    /// the ink would take away the thing the person can still edit.
    pub fn rasterise_ink(&mut self, board_id: &str) -> Result<String, CoreError> {
        let strokes: Vec<crate::raster::Stroke> = repo::read_ink(&self.store, board_id)?
            .into_iter()
            .filter_map(|s| serde_json::from_value(s).ok())
            .collect();

        let raster = crate::raster::rasterise(&strokes).map_err(|e| CoreError::Runtime(e.to_string()))?;

        Ok(repo::write_image(
            &mut self.store,
            repo::NewImage {
                board_id,
                origin: "sketch_raster",
                bytes: &raster.bytes,
                mime: "image/png",
                width: raster.width,
                height: raster.height,
                source_ink_ids: None,
            },
        )?)
    }

    /// Generate an exercise from the cards this board already holds. Doc 08.
    ///
    /// `level` is doc 17 section 4's rung. A board asking for an exercise of
    /// its own names none and gets the pack's ordinary template; a lesson names
    /// the one the learner is working at.
    pub fn make_exercise(
        &mut self,
        board_id: &str,
        audience_id: Option<&str>,
        level: Option<u8>,
    ) -> Result<pipeline::ExerciseOutcome, CoreError> {
        let policy = self.resolved()?;
        let pack = self.packs.get(&self.pack_code)?.clone();
        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers: &self.retrievers,
        };

        self.runtime
            .handle()
            .clone()
            .block_on(pipeline::run_exercise(
                &mut self.store,
                &ctx,
                board_id,
                audience_id,
                level,
            ))
            .map_err(|f| CoreError::Runtime(f.to_string()))
    }

    /// Re-verify a card already on a board, against the corpus as it stands now.
    ///
    /// Doc 07 section B3 batches these when a source goes stale. Nothing is
    /// retrieved and no answer is rewritten, so this is what a board reopened
    /// months later runs before the user reads it.
    pub fn verify_card(&mut self, board_id: &str, card_id: &str) -> Result<pipeline::CardOutcome, CoreError> {
        let policy = self.resolved()?;
        let pack = self.pack_for_board(board_id)?;
        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers: &self.retrievers,
        };

        let result = self.runtime.handle().clone().block_on(pipeline::run_verify_only(
            &mut self.store,
            &ctx,
            board_id,
            card_id,
        ));

        match result {
            Ok(outcome) => Ok(outcome),
            Err(f) => Err(CoreError::Runtime(f.to_string())),
        }
    }

    /// Ask a follow-up on an existing card.
    ///
    /// Doc 01 section 4.4's `parent_card_id` is what makes "which article says
    /// so?" answerable: on its own it names no subject, and the pipeline reads
    /// the parent's question and answer back out of this chain. Asked without a
    /// parent, such a question retrieves nothing, correctly and uselessly.
    ///
    /// The anchor is what separates doc 09's two branch verbs from a plain
    /// follow-up. A highlight carries the selected span, a block investigation
    /// carries the JSON pointer into the visual's payload, and either one makes
    /// the card a `branch` rather than a `follow`.
    pub fn ask_on(
        &mut self,
        board_id: &str,
        question: &str,
        depth_override: Option<&str>,
        model_override: Option<&str>,
        anchor: Anchor<'_>,
    ) -> Result<pipeline::CardOutcome, CoreError> {
        // The user's model choice from the chat window pins the answer stage to
        // the alias they named, with no fallback: the model they chose answers
        // or the run fails with `policy_unresolvable`, never a quiet
        // substitute. Without a choice the policy resolves as configured, which
        // since 2026-08-30 falls back across providers to whichever keys exist.
        let policy = match model_override {
            Some(alias) => resolve(
                &self.policy.pin_stage("synthesize", alias),
                self.keys.as_ref(),
                CARD_STAGES,
            )?,
            None => self.resolved()?,
        };
        let (board_depth, board_mode): (String, String) = self
            .store
            .conn()
            .query_row(
                "SELECT default_depth, mode FROM board WHERE id = ?1",
                rusqlite::params![board_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| ("fast".to_string(), "explore".to_string()));

        let card_id = repo::create_card(
            &mut self.store,
            repo::NewCard {
                board_id,
                parent_card_id: anchor.parent_card_id,
                // Doc 01 section 4.4's card kinds. A card with a parent and an
                // anchor is a branch, one with a parent alone is a follow, and
                // one with neither is a root.
                kind: anchor.kind(),
                question,
                depth: depth_override.unwrap_or(&board_depth),
                anchor_text: anchor.anchor_text,
                anchor_block_ref: anchor.anchor_block_ref,
                audience_id: None,
            },
        )?;

        // A board takes its title from the first question unless the user named
        // it. Doc 01 section 4.1 `named_by_user`.
        self.store
            .conn()
            .execute(
                "UPDATE board SET title = ?1 WHERE id = ?2 AND named_by_user = 0
                 AND (SELECT COUNT(*) FROM card WHERE board_id = ?2) = 1",
                rusqlite::params![truncate_title(question), board_id],
            )
            .ok();

        let pack = self.packs.get(&self.pack_code)?.clone();

        // Doc 16 section 3.4: a notebook question runs the normal pipeline at
        // deep "with retrievers restricted to `local:vault`, `boards`, and
        // optionally `local:*`, web off by default". The narrowing happens here
        // rather than inside the fan-out, because what a question may open is a
        // property of the run and the plan-less fallback reads the set too.
        // Both narrowings happen here, in one place, because everything
        // downstream reads the set: the fan-out opens it, the plan-less
        // fallback picks from it, and the Planner packet says which retrievers
        // are enabled. The lesson narrowed inside the fan-out until 13g, so the
        // Planner was told it could assign `local` on a lesson board and the
        // run then skipped it, which is BN-140's failure one layer up: a card
        // came back thin and nothing said why.
        let allow: Vec<&str> = match board_mode.as_str() {
            // Doc 16 section 3.4: a notebook question runs the normal pipeline
            // at deep "with retrievers restricted to `local:vault`, `boards`,
            // and optionally `local:*`, web off by default".
            //
            // Its one click way out adds `web` to the same narrowing rather
            // than skipping it: a rerun that dropped the restriction would
            // reach every retriever the profile has, which is not what the
            // button says.
            NOTEBOOK => {
                let mut allow: Vec<&str> = NOTEBOOK_RETRIEVERS.to_vec();
                if anchor.with_web {
                    allow.push("web");
                }
                allow
            }
            // Doc 17 section 5: a lesson reads the research retriever plus the
            // vault and boards.
            pipeline::LEARN => pipeline::LESSON_RETRIEVERS.to_vec(),
            _ => Vec::new(),
        };
        let restricted;
        let retrievers = if allow.is_empty() {
            &self.retrievers
        } else {
            restricted = self.retrievers.restricted(&allow);
            &restricted
        };

        let ctx = RunContext {
            registry: &self.registry,
            provider: self.provider.as_ref(),
            pack: &pack,
            policy,
            profile_id: self.profile_id.clone(),
            source: self.source,
            ledger: &self.ledger,
            retrievers,
        };

        // The runtime is owned by the core, so a handler blocks here rather than
        // the shell learning that agents are async.
        let result = self.runtime.handle().clone().block_on(pipeline::run_card(
            &mut self.store,
            &ctx,
            board_id,
            &card_id,
            question,
            depth_override,
            model_override,
        ));

        match result {
            Ok(outcome) => {
                if board_mode == NOTEBOOK {
                    self.record_grounding(board_id, &outcome)?;
                }
                Ok(outcome)
            }
            Err(f) => Err(CoreError::Runtime(f.to_string())),
        }
    }

    /// Doc 16 section 3.4's three states, recorded where the answer settled.
    ///
    /// Ungrounded is `no_passages` and nothing else: doc 06 section A10 makes
    /// that an honest card that says it found nothing, and the notebook labels
    /// it rather than replacing it with an answer from model knowledge. A card
    /// that looks answered and is not is the failure both documents are written
    /// against, and doc 16 section 2.1 adopts the contract on the grounds that
    /// Tessera already has it in this path.
    fn record_grounding(&mut self, board_id: &str, outcome: &pipeline::CardOutcome) -> Result<(), CoreError> {
        let state = if outcome.passages_seen == 0 {
            "ungrounded"
        } else if outcome.unsupported > 0 {
            "partly_grounded"
        } else {
            "grounded"
        };
        self.store.append(
            tessera_store::event::NewEvent::new(
                "notebook.grounding.v1",
                json!({
                    "card_id": outcome.card_id,
                    "state": state,
                    "passages": outcome.passages_seen,
                    "unsupported": outcome.unsupported,
                }),
                tessera_store::event::Provenance::harness("notebook", Some(outcome.run_id.clone())),
            )
            .on_board(board_id)
            .on_card(&outcome.card_id),
        )?;
        Ok(())
    }
}

/// Doc 16 section 3.4: a notebook session is a board of this mode.
pub const NOTEBOOK: &str = "notebook";

/// What a notebook question may open. Doc 16 section 3.4: the vault, the
/// profile's own prior cards, and not the web.
///
/// Doc 16 lists `local:*` as optional. It is left out: the notebook is the view
/// over what the person wrote, and a question that quietly reached their whole
/// document folder would answer from somewhere they did not ask about.
pub const NOTEBOOK_RETRIEVERS: &[&str] = &["vault", "boards"];

/// Doc 17 section 6's singleton map board, before anyone renames it.
const MAP_TITLE: &str = "What I am learning";

/// Doc 17 section 7: "at most three new ones".
const MAX_NEW_CONCEPTS: u64 = 3;

pub(crate) fn truncate_title(question: &str) -> String {
    let trimmed = question.trim();
    if trimmed.chars().count() <= 60 {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(57).collect();
    format!("{}…", cut.trim_end())
}

/// Doc 05 section 8.1's seeds, from the profile's retriever config.
///
/// The column already holds "folder inclusions, corpus subscriptions, key
/// references" and never a secret, which is exactly what a list of bases the
/// web retriever may read is. A profile that has named none has no web
/// retriever, which is the honest report rather than one pointed at the whole
/// internet by default.
fn web_seeds(store: &tessera_store::Store, profile_id: &str) -> Vec<String> {
    store
        .conn()
        .query_row(
            "SELECT retriever_config FROM profile WHERE id = ?1",
            rusqlite::params![profile_id],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|config| {
            config["web_seeds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
}
