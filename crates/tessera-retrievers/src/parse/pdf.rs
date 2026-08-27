//! PDF, text layer only.
//!
//! Doc 05 section 8.2 splits pdfs in two: "pdf with text layer via the
//! product's parser, scanned pdf via the Reader's OCR path". The Reader arrives
//! at M10, so a pdf with no text layer is an honest skip here rather than a
//! silent empty document, and it is recorded as `NeedsOcr` so the Profile's
//! Retrievers page can say why rather than leaving a file that vanished.
//!
//! `pdf-extract` is pure Rust over `lopdf`, which keeps the installer free of a
//! bundled native pdf engine. It also panics on some malformed files rather
//! than returning an error, so the call is caught: the corpus deliberately
//! carries a corrupt pdf and a password protected one, and neither may take the
//! index run down with it.

use std::path::Path;

use crate::chunking::{Chunk, ChunkLocation, windows};
use crate::parse::ParseError;

/// Whether the document declares an encryption dictionary.
///
/// Only the header region is scanned: `/Encrypt` appearing deep inside a
/// content stream is a coincidence, not a declaration.
fn is_encrypted(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(2_048)];
    window.windows(8).any(|w| w == b"/Encrypt")
}

/// Below this many characters, a pdf that parsed "successfully" has no text
/// layer worth the name. A scanned page yields a handful of stray glyphs.
const TEXT_LAYER_FLOOR: usize = 40;

pub fn parse(path: &Path) -> Result<Vec<Chunk>, ParseError> {
    let bytes = std::fs::read(path).map_err(|e| ParseError::Io(e.to_string()))?;

    // Encryption is read from the file rather than from the extractor's error
    // text, which says only that parsing failed. The distinction reaches the
    // user: "this file is protected" is actionable and "this file is damaged"
    // sends them looking for a corruption that is not there.
    if is_encrypted(&bytes) {
        return Err(ParseError::Protected(path.display().to_string()));
    }

    // pdf-extract panics on some inputs. A panic here would kill the whole
    // index run over one bad file, which doc 05 section 10 forbids.
    let extracted = std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&bytes));

    let text = match extracted {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            let detail = e.to_string();
            let lower = detail.to_ascii_lowercase();
            if lower.contains("encrypt") || lower.contains("password") {
                return Err(ParseError::Protected(path.display().to_string()));
            }
            return Err(ParseError::Malformed {
                format: "pdf",
                detail,
            });
        }
        Err(_) => {
            return Err(ParseError::Malformed {
                format: "pdf",
                detail: "the pdf parser could not read this file".into(),
            });
        }
    };

    let trimmed = text.trim();
    if trimmed.chars().filter(|c| !c.is_whitespace()).count() < TEXT_LAYER_FLOOR {
        return Err(ParseError::NeedsOcr(path.display().to_string()));
    }

    // Page breaks are the only structure a text layer reliably carries.
    let mut out = Vec::new();
    let mut sequence = 0usize;
    for (index, page) in trimmed.split('\u{c}').enumerate() {
        let page_text = page.trim();
        if page_text.is_empty() {
            continue;
        }
        let location = ChunkLocation::Page { page: index + 1 };
        let produced = windows(page_text, &location, sequence);
        sequence += produced.len();
        out.extend(produced);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    #[test]
    fn a_protected_pdf_says_so_rather_than_calling_itself_damaged() {
        let path = temp("locked.pdf", b"%PDF-1.7\n/Encrypt 1 0 R\ntrailer<<>>");
        assert!(
            matches!(parse(&path), Err(ParseError::Protected(_))),
            "an encrypted pdf was reported as something else"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_word_encrypt_deep_in_a_stream_is_not_a_declaration() {
        let mut bytes = b"%PDF-1.4\n".to_vec();
        bytes.extend(std::iter::repeat_n(b' ', 4_000));
        bytes.extend_from_slice(b"/Encrypt");
        assert!(!is_encrypted(&bytes));
    }

    #[test]
    fn a_corrupt_pdf_is_an_error_and_never_a_panic() {
        // The corpus carries one of these on purpose.
        let path = temp("corrupt.pdf", b"%PDF-1.4\nthis file stops mid");
        let result = parse(&path);
        assert!(result.is_err(), "a corrupt pdf parsed as if it were fine");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_file_that_is_not_a_pdf_at_all_is_an_error() {
        let path = temp("nonsense.pdf", b"just some bytes");
        assert!(parse(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_reports_io_rather_than_malformed() {
        let path = std::env::temp_dir().join("tessera-no-such-file.pdf");
        assert!(matches!(parse(&path), Err(ParseError::Io(_))));
    }
}
