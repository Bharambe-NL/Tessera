//! HTML, parsed rather than stripped.
//!
//! Doc 05 section 8.1: "main content extraction, boilerplate removal", then
//! chunk by heading. A regex that deletes anything between angle brackets would
//! be shorter and would also keep the contents of `<script>` and `<style>`,
//! which is how a retriever ends up indexing minified javascript and ranking it
//! against a question about capital buffers. html5ever knows the difference.

use scraper::{Html, Selector};

use crate::chunking::{Chunk, ChunkLocation, windows};

/// Elements whose text is never content.
const BOILERPLATE: &[&str] = &["script", "style", "nav", "footer", "header", "noscript", "svg"];

/// The document title and the metadata the corpus carries, for the Source row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HtmlMeta {
    pub title: Option<String>,
    pub issuer: Option<String>,
    pub published_at: Option<String>,
}

pub fn meta(source: &str) -> HtmlMeta {
    let document = Html::parse_document(source);
    let mut out = HtmlMeta::default();

    if let Ok(sel) = Selector::parse("title")
        && let Some(el) = document.select(&sel).next()
    {
        let text = el.text().collect::<String>().trim().to_string();
        out.title = (!text.is_empty()).then_some(text);
    }
    if let Ok(sel) = Selector::parse("meta[name]") {
        for el in document.select(&sel) {
            let name = el.value().attr("name").unwrap_or_default();
            let content = el.value().attr("content").unwrap_or_default().trim();
            if content.is_empty() {
                continue;
            }
            match name {
                "issuer" => out.issuer = Some(content.to_string()),
                "published" | "published_at" | "date" => {
                    out.published_at = Some(content.to_string())
                }
                _ => {}
            }
        }
    }
    out
}

pub fn parse(source: &str) -> Vec<Chunk> {
    let document = Html::parse_document(source);

    let Ok(body_sel) = Selector::parse("body") else {
        return Vec::new();
    };
    let Some(body) = document.select(&body_sel).next() else {
        return Vec::new();
    };

    // Walk the body in document order, opening a new section at every heading.
    let mut out = Vec::new();
    let mut sequence = 0usize;
    let mut title = String::new();
    let mut level = 1usize;
    let mut body_text = String::new();

    let Ok(walk) = Selector::parse("h1, h2, h3, h4, h5, h6, p, li, td, th, blockquote, pre") else {
        return Vec::new();
    };

    for element in body.select(&walk) {
        let name = element.value().name();

        // Anything sitting inside boilerplate is skipped wholesale.
        if element
            .ancestors()
            .filter_map(|a| a.value().as_element())
            .any(|e| BOILERPLATE.contains(&e.name()))
        {
            continue;
        }

        let text = element.text().collect::<String>();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }

        if let Some(depth) = name.strip_prefix('h').and_then(|d| d.parse::<usize>().ok())
            && (1..=6).contains(&depth)
        {
            // Close the section that was open.
            let location = ChunkLocation::Heading { title: title.clone(), level };
            let produced = windows(&body_text, &location, sequence);
            sequence += produced.len();
            out.extend(produced);

            title = text;
            level = depth;
            body_text.clear();
            continue;
        }

        body_text.push_str(&text);
        body_text.push('\n');
    }

    let location = ChunkLocation::Heading { title, level };
    let produced = windows(&body_text, &location, sequence);
    out.extend(produced);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<title>What Outsourcing Guidelines 2025 means for you</title>
<meta name="issuer" content="clearpath-systems.invalid">
<meta name="published" content="2025-05-22">
<style>.nav{color:red}</style>
</head><body>
<nav><a href="/">Home</a><a href="/about">About us today</a></nav>
<h1>What Outsourcing Guidelines 2025 means for you</h1>
<p>A plain reading of the rules, without the article numbers.</p>
<h2>Point 1</h2>
<p>In practice the notification period before an outsourcing starts comes to 85 days.</p>
<script>var tracking = "capital buffer requirement analytics";</script>
</body></html>"#;

    #[test]
    fn headings_open_sections_and_text_lands_under_them() {
        let out = parse(PAGE);
        let point_one = out
            .iter()
            .find(|c| matches!(&c.location, ChunkLocation::Heading { title, .. } if title == "Point 1"))
            .expect("the Point 1 section");
        assert!(point_one.text.contains("85 days"), "{:?}", point_one.text);
    }

    #[test]
    fn script_and_style_and_nav_never_reach_the_index() {
        // The whole reason this is parsed rather than regex stripped. The
        // script here contains the words a capital question would match on.
        let out = parse(PAGE);
        let all: String = out.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(!all.contains("tracking"), "script text was indexed");
        assert!(!all.contains("capital buffer requirement analytics"), "script text was indexed");
        assert!(!all.contains("About us today"), "nav text was indexed");
    }

    #[test]
    fn metadata_is_recovered_for_the_source_row() {
        let m = meta(PAGE);
        assert_eq!(m.issuer.as_deref(), Some("clearpath-systems.invalid"));
        assert_eq!(m.published_at.as_deref(), Some("2025-05-22"));
        assert!(m.title.as_deref().unwrap_or_default().contains("Outsourcing Guidelines"));
    }

    #[test]
    fn malformed_html_yields_what_it_can_rather_than_failing() {
        // html5ever recovers; the point is that this returns instead of panicking.
        let out = parse("<html><body><p>An unclosed paragraph<p>and another</body>");
        let all: String = out.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(all.contains("unclosed paragraph"));
    }

    #[test]
    fn an_empty_document_yields_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("<html><body></body></html>").is_empty());
    }
}
