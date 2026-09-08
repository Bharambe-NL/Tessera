//! Entity writes, each carrying the event that announces it.
//!
//! Every function here goes through [`Store::append_with`], so the row and its
//! event land in one transaction (doc 10 section 4). There is no path that
//! writes an entity without an event, which is what makes board history complete
//! rather than best effort.
//!
//! The read side returns the shapes the canvas renders. They mirror doc 01's
//! field names exactly, so a name never means two things across the RPC
//! boundary.
//!
//! One file per subject, and `mod.rs` re-exports all of them, so every
//! `repo::name` a caller already writes still resolves.

pub mod ancestry;
pub mod boards;
pub mod cards;
pub mod concepts;
pub mod exercises;
pub mod flags;
pub mod folders;
pub mod learn;
pub mod media;
pub mod notes;
pub mod pages;
pub mod profile;
pub mod sources;
mod sql;
pub mod verify;

pub use ancestry::*;
pub use boards::*;
pub use cards::*;
pub use concepts::*;
pub use exercises::*;
pub use flags::*;
pub use folders::*;
pub use learn::*;
pub use media::*;
pub use notes::*;
pub use pages::*;
pub use profile::*;
pub use sources::*;
pub use verify::*;
