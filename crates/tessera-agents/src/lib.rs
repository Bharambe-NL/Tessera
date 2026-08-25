//! The agents.
//!
//! Doc 10 section 2: each is "a module with packet in, output out". Everything
//! around that shape lives in the harness, so an agent here is only the
//! reasoning the spec describes for it.
//!
//! No doctrine lives in this crate. A rule about advice language or source
//! hierarchy is a pack field the agent reads (doc 12 operating principle 4).

pub mod prompts;
pub mod router;
pub mod synthesizer;
pub mod verifier;
pub mod visualizer;

pub use router::Router;
pub use synthesizer::Synthesizer;
pub use verifier::Verifier;
pub use visualizer::Visualizer;

/// Citation markers found in a span. The Verifier's marker_integrity check and
/// the Synthesizer's binding pass must agree on what counts as a marker, so
/// they share one parser.
pub(crate) fn synthesizer_markers(text: &str) -> std::collections::BTreeSet<usize> {
    synthesizer::markers_in(text).into_iter().collect()
}
