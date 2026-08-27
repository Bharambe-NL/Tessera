//! DOCX: a zip of XML. Doc 05 section 8.2 lists docx among the parsed formats.
//!
//! The text lives in `word/document.xml` as `w:t` runs inside `w:p`
//! paragraphs. Two details make a naive reader wrong. A single visible sentence
//! is often split across several runs, because a spell check mark or a change
//! of formatting starts a new one, so runs are joined within their paragraph
//! rather than treated as separate text. And `w:tab` and `w:br` carry no text
//! node at all, so without handling them words either side of a tab run
//! together into one token that no index will match.

use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::chunking::{Chunk, ChunkLocation, windows};
use crate::parse::ParseError;

pub fn parse(path: &Path) -> Result<Vec<Chunk>, ParseError> {
    let file = std::fs::File::open(path).map_err(|e| ParseError::Io(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| match e {
        zip::result::ZipError::InvalidArchive(detail) => ParseError::Malformed {
            format: "docx",
            detail: detail.to_string(),
        },
        other => ParseError::Malformed {
            format: "docx",
            detail: other.to_string(),
        },
    })?;

    let mut document = archive
        .by_name("word/document.xml")
        .map_err(|_| ParseError::Malformed {
            format: "docx",
            detail: "no word/document.xml in the archive".into(),
        })?;

    let mut xml = String::new();
    {
        use std::io::Read;
        document
            .read_to_string(&mut xml)
            .map_err(|e| ParseError::Malformed {
                format: "docx",
                detail: e.to_string(),
            })?;
    }

    Ok(chunks_from_xml(&xml))
}

fn chunks_from_xml(xml: &str) -> Vec<Chunk> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "t" => in_text = true,
                "p" => current.clear(),
                _ => {}
            },
            Ok(Event::End(e)) => match local_name(e.name().as_ref()) {
                "t" => in_text = false,
                "p" => {
                    let text = current.trim();
                    if !text.is_empty() {
                        paragraphs.push(text.to_string());
                    }
                    current.clear();
                }
                _ => {}
            },
            // A tab or a line break is whitespace, and dropping it welds two
            // words into one token.
            Ok(Event::Empty(e)) => {
                if matches!(local_name(e.name().as_ref()), "tab" | "br" | "cr") {
                    current.push(' ');
                }
            }
            Ok(Event::Text(t)) if in_text => {
                // The event carries the escaped text; `&amp;` in a document is
                // an ampersand to the reader and must be one in the index too.
                match quick_xml::escape::unescape(t.as_ref()) {
                    Ok(text) => current.push_str(&text),
                    Err(_) => current.push_str(t.as_ref()),
                }
            }
            Ok(Event::Eof) => break,
            // A damaged file yields what was read before the damage rather than
            // nothing, which is the same posture the other parsers take.
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    let body = paragraphs.join("\n");
    windows(&body, &ChunkLocation::Whole, 0)
}

/// Strip the namespace prefix: `w:t` and `t` are the same element.
fn local_name(name: &str) -> &str {
    match name.rfind(':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_inside_one_paragraph_are_joined() {
        // Word splits a sentence across runs freely. Treating each run as its
        // own chunk would cut "8.4 %" away from what it measures.
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>The minimum own funds </w:t></w:r><w:r><w:t>requirement is 8.4 %.</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let out = chunks_from_xml(xml);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "The minimum own funds requirement is 8.4 %.");
    }

    #[test]
    fn paragraphs_stay_separate() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>First paragraph.</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second paragraph.</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let out = chunks_from_xml(xml);
        assert!(out[0].text.contains("First paragraph."));
        assert!(out[0].text.contains("Second paragraph."));
        assert!(out[0].text.contains('\n'), "paragraphs were welded together");
    }

    #[test]
    fn a_tab_becomes_a_space_rather_than_nothing() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Buffer</w:t><w:tab/><w:t>2.5 %</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let out = chunks_from_xml(xml);
        assert_eq!(out[0].text, "Buffer 2.5 %", "a tab welded two tokens together");
    }

    #[test]
    fn an_unprefixed_document_parses_the_same() {
        let xml = "<document><body><p><r><t>Plain namespaces.</t></r></p></body></document>";
        assert_eq!(chunks_from_xml(xml)[0].text, "Plain namespaces.");
    }

    #[test]
    fn truncated_xml_yields_what_was_read_rather_than_panicking() {
        let xml = "<w:document><w:body><w:p><w:r><w:t>A sentence that surviv";
        let _ = chunks_from_xml(xml);
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tessera-docx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("broken.docx");
        std::fs::write(&path, b"this is not a zip archive at all").expect("write");
        assert!(matches!(parse(&path), Err(ParseError::Malformed { .. })));
        std::fs::remove_dir_all(&dir).ok();
    }
}
