//! The core: storage, harness, agents and providers behind one JSON-RPC surface.
//!
//! Doc 10 section 2: "One core, several shells. The pipeline, storage, and event
//! log are a library (the core) with a JSON-RPC boundary. The desktop shell is
//! the first client."
//!
//! The core runs in process with the shell in v1, so the web client can later
//! talk to the identical protocol over a socket. Everything the shell needs is a
//! method on the router built by [`core::build_router`]; nothing reaches around
//! it.

pub mod bridge;
pub mod core;
pub mod pipeline;
pub mod raster;
pub mod retrieval;
pub mod rpc;

pub use bridge::{Notification, ToastLevel, translate, translate_all};
pub use core::{Anchor, Core, CoreError, build_router};
pub use pipeline::{CardOutcome, ExerciseOutcome, RunContext, run_card};
pub use rpc::{Request, Response, Router, RpcError, codes, params};
