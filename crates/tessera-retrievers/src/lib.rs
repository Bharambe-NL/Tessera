//! The retrievers. Doc 05.
//!
//! Retrievers fetch passages. They are the only agents that touch external data
//! and the only ones that create Source and Passage rows, and doc 05 section 1
//! is explicit that they contain no model in the common path. That is what
//! makes the whole of this crate measurable against the synthetic corpus for
//! nothing.

pub mod chunking;
pub mod contract;
pub mod embed;
pub mod index;
pub mod parse;

pub use chunking::{Chunk, ChunkLocation};
pub use contract::{Coverage, Packet, Passage, Retrieved, Source};
pub use embed::{Embedder, HashEmbedder, LocalEmbedder};
pub use index::{Hit, search, write_document};
pub use parse::{ParseError, is_supported, parse_file};
