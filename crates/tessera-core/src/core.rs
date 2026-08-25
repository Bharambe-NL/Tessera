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
            runtime,
        })
    }

    /// An in memory core for tests and for the eval harness.
    pub fn in_memory(provider: Arc<dyn ModelProvider>) -> Result<Self, CoreError> {
        let root = std::env::temp_dir().join(format!("tessera-core-{}", tessera_store::new_id()));
        Self::open(
            root,
            Box::new(MemoryKeyStore::with("test-key", "sk-test")),
            provider,
            "test-key",
        )
    }

    fn resolved(&self) -> Result<ResolvedPolicy, CoreError> {
        Ok(resolve(&self.policy, self.keys.as_ref(), CARD_STAGES)?)
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

    /// Create a board on the active pack.
    pub fn create_board(&mut self, title: &str, depth: &str) -> Result<String, CoreError> {
        let general = self.packs.get(&self.pack_code)?;
        let pack_id = repo::ensure_pack(&self.store, &serde_json::to_value(general).unwrap_or(Value::Null))?;
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
                parent_card_id: None,
                kind: "root",
                question,
                depth: depth_override.unwrap_or(&board_depth),
                anchor_text: None,
                anchor_block_ref: None,
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

    r.register("board.list", |core: &mut Core, _| {
        let boards = repo::list_boards(&core.store, &core.profile_id, "active").map_err(store_error)?;
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
        let outcome = core
            .ask(&p.board_id, p.question.trim(), p.depth.as_deref())
            .map_err(core_error)?;
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
        Ok(json!({
            "profile_id": core.profile_id,
            "packs": core.packs.codes().collect::<Vec<_>>(),
            "active_pack": core.pack_code,
            "provider": core.provider.id(),
            "policy": serde_json::to_value(&core.policy).unwrap_or(Value::Null),
        }))
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
