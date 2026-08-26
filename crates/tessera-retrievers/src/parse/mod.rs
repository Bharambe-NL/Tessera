//! One module per format, all returning `Vec<Chunk>`. Doc 05 section 8.2.
//!
//! Every parser here obeys the same rule: a file it cannot read is an error for
//! that file and never a panic and never the end of the index run. Doc 05
//! section 10 makes `parse_error` a per file outcome, "skip file, record in
//! index errors", and the corpus deliberately contains a corrupt pdf, a
//! password protected pdf, a scanned pdf with no text layer, and an empty
//! document to prove it.
//!
//! Format is decided by looking inside the file, not by trusting its name. The
//! synthetic corpus turned out to contain a pdf called `.docx`, which is a
//! generator bug (BN-037) and also exactly what a real watched folder is full
//! of: files renamed by hand, exported with the wrong extension, or saved by a
//! tool that had its own opinion. A retriever that reads the extension and
//! stops has to declare those unreadable.

pub mod docx;
pub mod html;
pub mod markdown;
pub mod pdf;
pub mod text;
pub mod xlsx;

use std::io::Read;
use std::path::Path;

use thiserror::Error;

use crate::chunking::{Chunk, enforce_cap};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("io: {0}")]
    Io(String),
    /// The file is a format we know, and this copy of it is broken.
    #[error("{format} could not be read: {detail}")]
    Malformed { format: &'static str, detail: String },
    /// Encrypted, and we do not ask the user for passwords.
    #[error("{0} is protected and was not opened")]
    Protected(String),
    /// A pdf with no text layer. Doc 05 section 8.2 sends these to the Reader's
    /// OCR path, which arrives at M10, so until then it is an honest skip.
    #[error("{0} has no text layer; scanned documents need the Reader, which arrives at M10")]
    NeedsOcr(String),
    #[error("nothing here is a format we read: {0}")]
    UnsupportedFormat(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Html,
    Text,
    Docx,
    Xlsx,
    Pdf,
}

/// What the extension claims.
pub fn format_by_extension(path: &Path) -> Option<Format> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => Some(Format::Markdown),
        "html" | "htm" => Some(Format::Html),
        "txt" => Some(Format::Text),
        "docx" => Some(Format::Docx),
        "xlsx" => Some(Format::Xlsx),
        "pdf" => Some(Format::Pdf),
        _ => None,
    }
}

/// What the bytes say. `None` for the text formats, which have no signature.
pub fn sniff(path: &Path) -> Option<Format> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0u8; 4];
    let read = file.read(&mut magic).ok()?;
    if read < 4 {
        return None;
    }

    if &magic == b"%PDF" {
        return Some(Format::Pdf);
    }
    if &magic == b"PK\x03\x04" {
        // Both office formats are zips. Which one is decided by what is inside,
        // because the container says nothing.
        let file = std::fs::File::open(path).ok()?;
        let archive = zip::ZipArchive::new(file).ok()?;
        let names: Vec<&str> = archive.file_names().collect();
        if names.iter().any(|n| n.starts_with("word/")) {
            return Some(Format::Docx);
        }
        if names.iter().any(|n| n.starts_with("xl/")) {
            return Some(Format::Xlsx);
        }
        return None;
    }
    None
}

/// Parse a file into chunks.
///
/// The signature wins over the extension when the two disagree, because the
/// bytes are what has to be read either way.
pub fn parse_file(path: &Path) -> Result<Vec<Chunk>, ParseError> {
    let format = sniff(path)
        .or_else(|| format_by_extension(path))
        .ok_or_else(|| {
            ParseError::UnsupportedFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("no extension")
                    .to_string(),
            )
        })?;

    let chunks = match format {
        Format::Markdown => markdown::parse(&read(path)?),
        Format::Html => html::parse(&read(path)?),
        Format::Text => text::parse(&read(path)?),
        Format::Docx => docx::parse(path)?,
        Format::Xlsx => xlsx::parse(path)?,
        Format::Pdf => pdf::parse(path)?,
    };

    Ok(enforce_cap(chunks))
}

/// Whether this build can read the file at all, so a folder scan can skip an
/// image without recording it as a failure.
pub fn is_supported(path: &Path) -> bool {
    format_by_extension(path).is_some() || sniff(path).is_some()
}

fn read(path: &Path) -> Result<String, ParseError> {
    let bytes = std::fs::read(path).map_err(|e| ParseError::Io(e.to_string()))?;
    // Lossy on purpose. The corpus carries an ocr noise case, and a stray byte
    // in the middle of a memo is not a reason to lose the whole document.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-parse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    #[test]
    fn a_pdf_wearing_a_docx_extension_is_read_as_a_pdf() {
        // Exactly what the corpus contains (BN-037), and what a watched folder
        // is full of. Trusting the name here means declaring the file broken.
        let path = temp("mislabelled.docx", b"%PDF-1.3\nnot really a word document");
        assert_eq!(sniff(&path), Some(Format::Pdf));
        assert_eq!(format_by_extension(&path), Some(Format::Docx));
        // It still fails, because those bytes are not a usable pdf, but it
        // fails as a pdf rather than as a zip that is missing word/document.xml.
        assert!(matches!(parse_file(&path), Err(ParseError::Malformed { format: "pdf", .. })));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_text_format_falls_back_to_its_extension() {
        let path = temp("note.md", b"# A heading\n\nThe buffer is 2.5 percent.\n");
        assert_eq!(sniff(&path), None, "markdown has no signature to find");
        let chunks = parse_file(&path).expect("parses by extension");
        assert!(chunks.iter().any(|c| c.text.contains("2.5 percent")));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_file_that_is_neither_is_refused_by_name() {
        let path = temp("photo.jpeg", b"\xff\xd8\xff\xe0 jpeg bytes");
        assert!(matches!(parse_file(&path), Err(ParseError::UnsupportedFormat(_))));
        assert!(!is_supported(&path));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_empty_file_is_not_mistaken_for_a_format() {
        let path = temp("empty.bin", b"");
        assert_eq!(sniff(&path), None);
        std::fs::remove_file(&path).ok();
    }
}
