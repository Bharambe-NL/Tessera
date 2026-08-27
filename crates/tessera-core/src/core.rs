//! The core's state and its registered method surface.
//!
//! Doc 10 section 2: one core, several shells. Everything the desktop shell can
//! do is a method registered here, so the web client that arrives later talks to
//! the identical protocol over a socket rather than to a second, thinner API.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use tessera_doctrine::PackLibrary;
use tessera_providers::{KeyStore, MemoryKeyStore, ModelPolicy, ModelProvider, ResolvedPolicy, resolve};
use tessera_schema::Registry;
use tessera_store::{Source, Store, repo};

use crate::pipeline::{self, RunContext};
use crate::rpc::{Router, RpcError, params};

/// The stages a card run needs resolved before it starts. Doc 03 section 8.3
/// fails before any retrieval rather than discovering a missing key halfway.
const CARD_STAGES: &[&str] = &["route", "plan", "retrieve", "synthesize", "visualize", "verify"];

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
}

impl<'a> Anchor<'a> {
    /// A plain follow-up: the parent's chain is the context, nothing is anchored.
    pub fn on(parent_card_id: &'a str) -> Self {
        Self {
            parent_card_id: Some(parent_card_id),
            ..Self::default()
        }
    }

    fn anchored(&self) -> bool {
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
    /// Open a profile folder and bring the core up.
    pub fn open(
        root: impl AsRef<std::path::Path>,
        keys: Box<dyn KeyStore>,
        provider: Arc<dyn ModelProvider>,
        key_ref: &str,
    ) -> Result<Self, CoreError> {
        let mut store = Store::open(root)?;
        let registry = Registry::load()?;
        let packs = PackLibrary::load_built_in(&registry)?;

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

        Ok(Self {
            store,
            registry,
            packs,
            keys,
            provider,
            policy,
            profile_id,
            source: Source::Live,
            pack_code: "general".to_string(),
            ledger: tessera_harness::Ledger::new(),
            retrievers: crate::retrieval::RetrieverSet::default(),
            runtime,
        })
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
        self.pack_code = code.to_string();
        Ok(())
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
        self.ask_on(board_id, question, depth_override, Anchor::default())
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
    pub fn make_exercise(
        &mut self,
        board_id: &str,
        audience_id: Option<&str>,
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
        anchor: Anchor<'_>,
    ) -> Result<pipeline::CardOutcome, CoreError> {
        let policy = self.resolved()?;
        let board_depth: String = self
            .store
            .conn()
            .query_row(
                "SELECT default_depth FROM board WHERE id = ?1",
                rusqlite::params![board_id],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "fast".to_string());

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

        // The runtime is owned by the core, so a handler blocks here rather than
        // the shell learning that agents are async.
        let result = self.runtime.handle().clone().block_on(pipeline::run_card(
            &mut self.store,
            &ctx,
            board_id,
            &card_id,
            question,
            depth_override,
        ));

        match result {
            Ok(outcome) => Ok(outcome),
            Err(f) => Err(CoreError::Runtime(f.to_string())),
        }
    }
}

fn truncate_title(question: &str) -> String {
    let trimmed = question.trim();
    if trimmed.chars().count() <= 60 {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(57).collect();
    format!("{}…", cut.trim_end())
}

// ------------------------------------------------------------- rpc surface --

#[derive(Deserialize)]
struct BoardCreate {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    depth: Option<String>,
}

#[derive(Deserialize)]
struct BoardRef {
    board_id: String,
}

#[derive(Deserialize)]
struct Ask {
    board_id: String,
    question: String,
    #[serde(default)]
    depth: Option<String>,
    /// Doc 09 section 5's Branch verb, in its three forms: absent for a root
    /// card, present alone for a follow-up, present with an anchor for a branch.
    #[serde(default)]
    parent_card_id: Option<String>,
    #[serde(default)]
    anchor_text: Option<String>,
    #[serde(default)]
    anchor_block_ref: Option<String>,
}

#[derive(Deserialize)]
struct CardRef {
    board_id: String,
    card_id: String,
}

fn default_board_status() -> String {
    "active".to_string()
}

/// Enough rows for the queue to be worth scrolling, few enough that a profile
/// with thousands of flags does not hand the webview all of them at once.
fn default_flag_limit() -> i64 {
    200
}

/// `with_history` defaults to on. Doc 01 section 7: "events.jsonl is on by
/// default", and dropping history is the deliberate act, not keeping it.
fn yes() -> bool {
    true
}

fn default_library_limit() -> i64 {
    500
}

/// A bound on one pasted image, so a gesture cannot fill the profile folder.
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Decode one base64 image. `None` on anything that is not base64, which the
/// boundary reports as a bad image rather than storing a blob nobody can read.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
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

/// Register every method the shell may call. Doc 10 section 2's boundary.
pub fn build_router() -> Router<Core> {
    let mut r = Router::new();

    r.register("board.create", |core: &mut Core, p| {
        let p: BoardCreate = params(p)?;
        let id = core
            .create_board(
                p.title.as_deref().unwrap_or("Untitled board"),
                p.depth.as_deref().unwrap_or("fast"),
            )
            .map_err(core_error)?;
        Ok(json!({ "board_id": id }))
    });

    r.register("board.list", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Which {
            /// Doc 09 open question 1: Trash is a filter on Home, so it is this
            /// word rather than a second method.
            #[serde(default = "default_board_status")]
            status: String,
        }
        let p: Which = params(p).unwrap_or(Which {
            status: default_board_status(),
        });
        if !matches!(p.status.as_str(), "active" | "trashed") {
            return Err(RpcError::core("unknown_status", "A board is active or trashed."));
        }
        let boards = repo::list_boards(&core.store, &core.profile_id, &p.status).map_err(store_error)?;
        Ok(json!({ "boards": boards }))
    });

    r.register("board.get", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        match repo::read_board(&core.store, &p.board_id).map_err(store_error)? {
            Some(board) => Ok(serde_json::to_value(board).unwrap_or(Value::Null)),
            None => Err(RpcError::core(
                "board_missing",
                "That board is not on this profile.",
            )),
        }
    });

    // Doc 09 section 5's Edit verb on a board. Renaming is what turns off the
    // inference that titles an unnamed board from its first question.
    r.register("board.rename", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Rename {
            board_id: String,
            title: String,
        }
        let p: Rename = params(p)?;
        let title = p.title.trim();
        if title.is_empty() {
            return Err(RpcError::core(
                "empty_title",
                "Type a title, or leave the one the first question gave it.",
            ));
        }
        repo::rename_board(&mut core.store, &p.board_id, title).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "title": title }))
    });

    // Doc 09 section 5's Remove verb on a board, and its two undos. Doc 09 open
    // question 1, adopted by doc 11: Trash is a filter on Home rather than a
    // rail item, so these three are what that filter acts on.
    r.register("board.trash", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        repo::trash_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "trashed" }))
    });

    r.register("board.restore", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        repo::restore_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "active" }))
    });

    // The one verb with nothing behind it. A purged board is gone; its events
    // stay, because the log is append only and the database enforces that.
    r.register("board.purge", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let trashed: bool = core
            .store
            .conn()
            .query_row(
                "SELECT status = 'trashed' FROM board WHERE id = ?1",
                rusqlite::params![p.board_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !trashed {
            return Err(RpcError::core(
                "purge_needs_trash",
                "Move the board to Trash first, so a purge is never one click from a board in use.",
            ));
        }
        repo::purge_board(&mut core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "board_id": p.board_id, "status": "purged" }))
    });

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

    // Doc 14 section 3.3's triggers, as the surface the panel drives.
    //
    // One method per trigger rather than one that infers the stage, because doc
    // 14 section 3.4's machine moves on what the learner did and a turn that
    // guessed which move it was would be guessing at the learner.
    r.register("learn.start", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Start {
            board_id: String,
            topic: String,
        }
        let p: Start = params(p)?;
        if p.topic.trim().is_empty() {
            return Err(RpcError::core(
                "empty_topic",
                "Say what you want to learn about first.",
            ));
        }
        let session_id =
            repo::start_learn_session(&mut core.store, &p.board_id, p.topic.trim()).map_err(store_error)?;
        let turn = core
            .tutor_turn(&p.board_id, "intake", None, None)
            .map_err(core_error)?;
        Ok(json!({ "session_id": session_id, "turn": turn }))
    });

    r.register("learn.get", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let session = repo::read_learn_session(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "session": session }))
    });

    // Doc 14 section 3.4: the learner may skip intake with "just build it", so
    // answering is optional and building is its own call.
    r.register("learn.answer_intake", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Answer {
            board_id: String,
            q: String,
            a: String,
        }
        let p: Answer = params(p)?;
        let session = repo::read_learn_session(&core.store, &p.board_id)
            .map_err(store_error)?
            .ok_or_else(|| RpcError::core("no_session", "This board has no learn session."))?;

        let mut intake = session["intake"].as_array().cloned().unwrap_or_default();
        intake.push(json!({ "q": p.q, "a": p.a }));
        let session_id = session["session_id"].as_str().unwrap_or_default().to_string();

        repo::update_learn_session(
            &mut core.store,
            repo::LearnUpdate {
                actor: repo::Actor::Learner,
                session_id: &session_id,
                board_id: &p.board_id,
                status: None,
                set: vec![("intake", Value::Array(intake))],
                event: "learn.intake_answered.v1",
                payload: json!({ "session_id": session_id, "q": p.q, "a": p.a }),
            },
        )
        .map_err(store_error)?;
        Ok(json!({ "recorded": true }))
    });

    r.register("learn.build", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "building", None, None)
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    r.register("learn.check", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Check {
            board_id: String,
            #[serde(default)]
            card_id: Option<String>,
        }
        let p: Check = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "checking", None, p.card_id.as_deref())
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    // Doc 14 section 3.6. No agent: grading one multiple choice answer needs
    // none, the same reason doc 08 section 7 has the UI record an attempt.
    r.register("learn.answer_check", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Answered {
            board_id: String,
            item: Value,
            picked: String,
            #[serde(default)]
            concept_ids: Vec<String>,
        }
        let p: Answered = params(p)?;
        let correct =
            pipeline::record_check(&mut core.store, &p.board_id, &p.item, &p.picked, &p.concept_ids)
                .map_err(|f| RpcError::core("learn", f.to_string()))?;
        Ok(json!({ "correct": correct }))
    });

    r.register("learn.say", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Say {
            board_id: String,
            message: String,
        }
        let p: Say = params(p)?;
        let turn = core
            .tutor_turn(&p.board_id, "reading", Some(&p.message), None)
            .map_err(core_error)?;
        Ok(json!({ "turn": turn }))
    });

    r.register("learn.end", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let summary = pipeline::end_learn_session(&mut core.store, &p.board_id)
            .map_err(|f| RpcError::core("learn", f.to_string()))?;
        Ok(summary)
    });

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
            #[serde(default)]
            exported_by: Option<String>,
        }
        let p: Export = params(p)?;
        let options = tessera_bundle::ExportOptions {
            with_history: p.with_history,
            local_documents: p.local_documents.into_iter().collect(),
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

    r.register("board.add_image", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct AddImage {
            board_id: String,
            data: String,
            mime: String,
            width: u32,
            height: u32,
        }
        let p: AddImage = params(p)?;
        let bytes = decode_base64(&p.data)
            .ok_or_else(|| RpcError::core("bad_image", "That image could not be read as an image."))?;
        // A bound, so a paste cannot fill the profile folder in one gesture.
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(RpcError::core(
                "image_too_large",
                "That image is over 20 MB. Scale it down and paste it again.",
            ));
        }
        let id = core
            .add_image(&p.board_id, &bytes, &p.mime, p.width, p.height)
            .map_err(core_error)?;
        Ok(json!({ "image_id": id }))
    });

    // Doc 07 section A3: "on demand from Read sketch, Read this image, or Read
    // on an Image row".
    r.register("card.read", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Read {
            board_id: String,
            image_id: String,
        }
        let p: Read = params(p)?;
        let outcome = core.read_image(&p.board_id, &p.image_id).map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags,
        }))
    });

    // The sketch raster path. Doc 12 phase 9 names it; the ink survives it.
    r.register("board.rasterise_ink", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let image_id = core.rasterise_ink(&p.board_id).map_err(core_error)?;
        Ok(json!({ "image_id": image_id }))
    });

    // Doc 08 section 3: "on demand from a board". The toolbar's Check
    // understanding, which is the only trigger until Learn mode adds its own.
    r.register("exercise.create", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Make {
            board_id: String,
            #[serde(default)]
            audience_id: Option<String>,
        }
        let p: Make = params(p)?;
        let outcome = core
            .make_exercise(&p.board_id, p.audience_id.as_deref())
            .map_err(core_error)?;
        Ok(json!({
            "exercise_id": outcome.exercise_id,
            "run_id": outcome.run_id,
            "items": outcome.items,
            "dropped": outcome.dropped,
        }))
    });

    r.register("exercise.list", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let exercises = repo::list_exercises(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "exercises": exercises }))
    });

    // Doc 08 section 7: the attempt comes from the UI, because grading a
    // multiple choice answer needs no agent. The score is computed in the store
    // from the exercise's own items rather than trusted from the caller, so it
    // is a fact about the exercise and not a number the shell sent.
    r.register("exercise.attempt", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Attempt {
            exercise_id: String,
            /// Item id to chosen option id.
            answers: Value,
        }
        let p: Attempt = params(p)?;
        let (attempt_id, correct, total) =
            repo::record_attempt(&mut core.store, &p.exercise_id, &p.answers).map_err(store_error)?;
        Ok(json!({
            "attempt_id": attempt_id,
            "correct": correct,
            "total": total,
        }))
    });

    // Doc 08 section 11: a wrong item is reported from the card, and the report
    // feeds pack maintenance rather than changing the exercise.
    r.register("exercise.report_item", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Report {
            exercise_id: String,
            item_id: String,
            #[serde(default)]
            reason: Option<String>,
        }
        let p: Report = params(p)?;
        repo::report_exercise_item(&mut core.store, &p.exercise_id, &p.item_id, p.reason.as_deref())
            .map_err(store_error)?;
        Ok(json!({ "reported": p.item_id }))
    });

    // Doc 09 section 9's Concepts row actions. Doc 01 section 4.10: agents
    // propose and the user confirms, so this is the confirming half.
    r.register("concept.decide", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Decide {
            concept_id: String,
            accept: bool,
        }
        let p: Decide = params(p)?;
        match repo::decide_concept(&mut core.store, &p.concept_id, p.accept).map_err(store_error)? {
            Some(term) => Ok(json!({ "concept_id": p.concept_id, "term": term })),
            None => Err(RpcError::core(
                "no_proposed_concept",
                "That concept was decided already. Reload Library to see where it went.",
            )),
        }
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

    // Doc 09 section 12: board history, rendered from events.
    r.register("board.history", |core: &mut Core, p| {
        let p: BoardRef = params(p)?;
        let events = repo::board_history(&core.store, &p.board_id).map_err(store_error)?;
        Ok(json!({ "events": events }))
    });

    r.register("card.ask", |core: &mut Core, p| {
        let p: Ask = params(p)?;
        if p.question.trim().is_empty() {
            return Err(RpcError::core("empty_question", "Type a question first."));
        }
        let anchor = Anchor {
            parent_card_id: p.parent_card_id.as_deref(),
            anchor_text: p.anchor_text.as_deref(),
            anchor_block_ref: p.anchor_block_ref.as_deref(),
        };
        // An anchor names a span on a card, so without the card it names
        // nothing. Refuse here rather than storing a root card carrying a
        // pointer into a visual it has no parent to read.
        if anchor.parent_card_id.is_none() && anchor.anchored() {
            return Err(RpcError::core(
                "anchor_without_parent",
                "Branching from a highlight needs the card it was highlighted on.",
            ));
        }
        let outcome = core
            .ask_on(&p.board_id, p.question.trim(), p.depth.as_deref(), anchor)
            .map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags
        }))
    });

    // Doc 09 section 5's Rerun verb on a card. Nothing is retrieved and no
    // answer is rewritten: the card is checked again against the corpus as it
    // stands, which is what a stale flag asks the reader to do.
    r.register("card.verify", |core: &mut Core, p| {
        let p: CardRef = params(p)?;
        let outcome = core.verify_card(&p.board_id, &p.card_id).map_err(core_error)?;
        Ok(json!({
            "card_id": outcome.card_id,
            "run_id": outcome.run_id,
            "status": outcome.status,
            "confidence": outcome.confidence,
            "flags": outcome.flags
        }))
    });

    // The events a board has produced since an index, translated for the UI.
    // Pattern 25: the protocol is a view over the log.
    r.register("board.notifications", |core: &mut Core, p| {
        #[derive(Deserialize)]
        struct Since {
            board_id: String,
            #[serde(default)]
            after: i64,
        }
        let p: Since = params(p)?;
        let events = core.store.events(Some(&p.board_id)).map_err(store_error)?;
        let notifications: Vec<Value> = events
            .iter()
            .filter(|e| e.monotonic_index > p.after)
            .filter_map(crate::bridge::translate)
            .filter_map(|n| serde_json::to_value(n).ok())
            .collect();
        let latest = events.last().map(|e| e.monotonic_index).unwrap_or(p.after);
        Ok(json!({ "notifications": notifications, "index": latest }))
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

        Ok(json!({
            "profile_id": core.profile_id,
            "packs": core.packs.codes().collect::<Vec<_>>(),
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

        Ok(json!({
            "folder_id": id,
            "label": p.label.trim(),
            "sensitive": p.sensitive,
            // Doc 10 section 16's sentence, as data the Retrievers page renders
            // rather than a string it composes: two screens composing it would
            // one day disagree about which folders send text.
            "text_leaves_machine": p.provider_embeddings,
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
        Ok(json!({ "key_ref": p.key_ref, "key_present": true }))
    });

    r
}

fn core_error(e: CoreError) -> RpcError {
    // House style: say what happened and how to fix it. Doc 11 section 9.
    match &e {
        CoreError::Provider(p) => RpcError::core(p.kind(), provider_message(p)),
        other => RpcError::core("core", other.to_string()),
    }
}

fn provider_message(e: &tessera_providers::ProviderError) -> String {
    use tessera_providers::ProviderError as P;
    match e {
        P::NoKey { .. } => "No model key. Add one in Profile to answer cards.".into(),
        P::Auth { provider } => format!("The {provider} key was rejected. Check it in Profile."),
        P::RateLimited { provider, .. } => format!("{provider} is rate limiting. Try again shortly."),
        other => other.to_string(),
    }
}

fn store_error(e: tessera_store::StoreError) -> RpcError {
    RpcError::core("store", e.to_string())
}
