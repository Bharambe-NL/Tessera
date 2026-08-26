//! The chunking contract every parser shares. Doc 05 sections 5 and 8.
//!
//! One shape in, one shape out. A parser's whole job is to turn bytes into
//! `Chunk`s; ranking, persistence and citation binding never learn what format
//! a passage came from.
//!
//! Two rules from the spec govern the sizes. Doc 05 section 8.1: "chunk by
//! heading then by 800 character windows with 100 overlap". Doc 05 section 5:
//! "passage text length capped at 1,200 characters (longer spans are split)".
//! The window is the smaller of the two, so the cap only bites on a location
//! that cannot be split further.

use serde::{Deserialize, Serialize};

/// Doc 05 section 8.1.
pub const WINDOW: usize = 800;
pub const OVERLAP: usize = 100;
/// Doc 05 section 5.
pub const MAX_PASSAGE_CHARS: usize = 1_200;

/// Where a chunk came from, in terms the reader can navigate back to.
///
/// This is what a citation points at, so it is per format rather than a generic
/// offset: "article 12, paragraph 3" is a location a person can check, and
/// "character 4,192" is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChunkLocation {
    /// Markdown, HTML, and plain text with headings.
    Heading { title: String, level: usize },
    /// Regulatory text. Doc 05 section 8.3: "Passage location is article and
    /// paragraph."
    ArticleParagraph { article: String, paragraph: usize },
    /// A spreadsheet. Doc 05 section 8.2: "A spreadsheet chunk carries the row
    /// range."
    RowRange { sheet: String, from: usize, to: usize },
    /// A pdf, and anything else that only has a page.
    Page { page: usize },
    /// A file with no structure at all.
    Whole,
}

/// One unit of retrievable text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub text: String,
    pub location: ChunkLocation,
    /// Position within the document, so two chunks of one section stay ordered
    /// and a near duplicate of one section does not collide with another.
    pub sequence: usize,
}

impl Chunk {
    pub fn new(text: impl Into<String>, location: ChunkLocation, sequence: usize) -> Self {
        Self { text: text.into(), location, sequence }
    }
}

/// Cut one section's text into overlapping windows.
///
/// Splits on a character boundary near the window edge rather than mid word,
/// because a passage that begins "…ation requirement is 8.9" reads as damaged
/// and, worse, loses the term a lexical index would have matched on.
pub fn windows(text: &str, location: &ChunkLocation, start_sequence: usize) -> Vec<Chunk> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= WINDOW {
        return vec![Chunk::new(text, location.clone(), start_sequence)];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    let mut sequence = start_sequence;

    while start < chars.len() {
        let hard_end = (start + WINDOW).min(chars.len());
        let end = if hard_end == chars.len() {
            hard_end
        } else {
            // Back off to the last sentence end, then to the last space, then
            // give up and cut where the window says.
            let window = &chars[start..hard_end];
            let boundary = window
                .iter()
                .rposition(|c| *c == '.' || *c == '\n')
                .filter(|i| *i > WINDOW / 2)
                .or_else(|| window.iter().rposition(|c| c.is_whitespace()).filter(|i| *i > WINDOW / 2));
            match boundary {
                Some(i) => start + i + 1,
                None => hard_end,
            }
        };

        let piece: String = chars[start..end].iter().collect();
        let piece = piece.trim();
        if !piece.is_empty() {
            out.push(Chunk::new(piece, location.clone(), sequence));
            sequence += 1;
        }

        if end >= chars.len() {
            break;
        }
        // The overlap is what keeps a fact that straddles a window boundary
        // findable from either side.
        start = end.saturating_sub(OVERLAP).max(start + 1);
    }

    out
}

/// Split anything still over the hard cap. Doc 05 section 5.
pub fn enforce_cap(chunks: Vec<Chunk>) -> Vec<Chunk> {
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.text.chars().count() <= MAX_PASSAGE_CHARS {
            out.push(chunk);
            continue;
        }
        let chars: Vec<char> = chunk.text.chars().collect();
        for (i, piece) in chars.chunks(MAX_PASSAGE_CHARS).enumerate() {
            out.push(Chunk::new(
                piece.iter().collect::<String>(),
                chunk.location.clone(),
                chunk.sequence + i,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading() -> ChunkLocation {
        ChunkLocation::Heading { title: "A section".into(), level: 2 }
    }

    #[test]
    fn short_text_is_one_chunk() {
        let out = windows("The buffer is 2.5 percent.", &heading(), 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "The buffer is 2.5 percent.");
    }

    #[test]
    fn empty_text_yields_nothing() {
        assert!(windows("   \n  ", &heading(), 0).is_empty());
    }

    #[test]
    fn long_text_splits_with_overlap() {
        let sentence = "The capital conservation buffer is two point five percent of assets. ";
        let text = sentence.repeat(40);
        let out = windows(&text, &heading(), 0);
        assert!(out.len() > 1, "long text did not split");
        for chunk in &out {
            assert!(chunk.text.chars().count() <= WINDOW, "a window ran over");
        }
        // Sequence numbers are dense and ordered.
        for (i, chunk) in out.iter().enumerate() {
            assert_eq!(chunk.sequence, i);
        }
    }

    #[test]
    fn a_fact_on_a_window_boundary_survives_in_one_piece() {
        // The reason the overlap exists. A value split across two windows is
        // findable from neither, because each half states half a number.
        let filler = "Words that carry nothing in particular. ".repeat(19);
        let text = format!("{filler}The leverage ratio floor is 3.4 percent.{filler}");
        let out = windows(&text, &heading(), 0);
        assert!(
            out.iter().any(|c| c.text.contains("3.4 percent")),
            "the value was lost at a boundary"
        );
    }

    #[test]
    fn the_hard_cap_is_enforced_even_when_windows_are_not_used() {
        let long = Chunk::new("x".repeat(3_000), ChunkLocation::Whole, 0);
        let out = enforce_cap(vec![long]);
        assert_eq!(out.len(), 3);
        for chunk in &out {
            assert!(chunk.text.chars().count() <= MAX_PASSAGE_CHARS);
        }
    }

    #[test]
    fn splitting_prefers_a_sentence_end_over_mid_word() {
        let text = format!("{} Final sentence here.", "Filler sentence. ".repeat(60));
        let out = windows(&text, &heading(), 0);
        for chunk in &out {
            let t = chunk.text.trim_end();
            assert!(
                t.ends_with('.') || t.ends_with("here.") || chunk.sequence == out.len() - 1,
                "chunk ended mid sentence: {t:?}"
            );
        }
    }
}
