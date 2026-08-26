//! Plain text. No structure to find, so none is invented.

use crate::chunking::{Chunk, ChunkLocation, windows};

pub fn parse(source: &str) -> Vec<Chunk> {
    windows(source, &ChunkLocation::Whole, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_windows_with_no_location_claimed() {
        let out = parse("The exit plan review interval is 12 months.");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].location, ChunkLocation::Whole);
    }

    #[test]
    fn an_empty_file_yields_nothing() {
        assert!(parse("").is_empty());
    }
}
