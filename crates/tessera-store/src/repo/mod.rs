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

mod ancestry;
mod boards;
mod cards;
mod concepts;
mod exercises;
mod flags;
mod folders;
mod learn;
mod media;
mod notes;
mod pages;
mod profile;
mod sources;
mod sql;
mod verify;

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
