//! Wikilinks. Doc 16 sections 2.2 and 3.1.
//!
//! `[[Title]]` and `[[Title|alias]]` in a page's markdown, parsed on save and
//! stored as `page_link` rows so that backlinks are a query rather than a scan
//! over every body in the vault.
//!
//! Doc 16 section 2.2 names resolving by title string as one of the assessed
//! package's mistakes: renames silently break the links into a page. So a link
//! resolves to a Page by id, or to a Concept by term or alias, and the title in
//! the body is what it displays rather than what it points at. Rename the page
//! and the link still arrives.
//!
//! The parser is pure and knows nothing about the store. What it will not do is
//! read a link out of code: a vault that documents this feature is full of
//! `[[Title]]` in fenced blocks and backticks, and linking those would fill the
//! person's own notes with references they did not write.

/// One `[[...]]` as it appears in a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wikilink {
    /// What the link points at: the text before the pipe.
    pub target_title: String,
    /// What the body shows: the text after the pipe, or the title.
    pub display_text: String,
    /// The byte offset of the opening bracket, so an editor can find it again
    /// without re-parsing.
    pub position: usize,
}

/// Every wikilink in a body, in the order they appear.
///
/// Duplicates are kept. Two links to one page from one body are two links, and
/// a backlink panel that showed one of them would be lying about the second.
pub fn parse(body: &str) -> Vec<Wikilink> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_fence = false;
    let mut in_code = false;
    let mut at_line_start = true;

    while i < bytes.len() {
        // A fence opens or closes at the start of a line. Everything between is
        // the person's example, not their link.
        if at_line_start && bytes[i..].starts_with(b"```") {
            in_fence = !in_fence;
            in_code = false;
            i += 3;
            at_line_start = false;
            continue;
        }
        if bytes[i] == b'\n' {
            at_line_start = true;
            in_code = false;
            i += 1;
            continue;
        }
        at_line_start = false;

        if bytes[i] == b'`' && !in_fence {
            in_code = !in_code;
            i += 1;
            continue;
        }

        if in_fence || in_code || !bytes[i..].starts_with(b"[[") {
            i += 1;
            continue;
        }

        let start = i;
        let Some(end) = find(bytes, i + 2, b"]]") else {
            // An opening with no closing is text, and so is everything after
            // it: there is no second link hiding inside an unterminated one.
            break;
        };
        let Ok(inner) = std::str::from_utf8(&bytes[i + 2..end]) else {
            i += 2;
            continue;
        };

        // A link cannot span a paragraph. `[[` on one line and `]]` three
        // paragraphs later is two pieces of punctuation, not a reference.
        if inner.contains('\n') {
            i += 2;
            continue;
        }

        let (target, display) = match inner.split_once('|') {
            Some((target, alias)) => (target.trim(), alias.trim()),
            None => (inner.trim(), inner.trim()),
        };
        if target.is_empty() {
            i = end + 2;
            continue;
        }

        out.push(Wikilink {
            target_title: target.to_string(),
            display_text: if display.is_empty() { target } else { display }.to_string(),
            position: start,
        });
        i = end + 2;
    }

    out
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|at| from + at)
}

/// Render a body with its links as plain text, for the retrievers and for the
/// markdown a person reads outside the app.
///
/// `[[Title|alias]]` reads as "alias", because that is what the sentence says.
/// A retriever indexing the brackets would match a query for "[[" and miss the
/// sentence the link is part of.
pub fn strip(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for link in parse(body) {
        out.push_str(&body[last..link.position]);
        out.push_str(&link.display_text);
        // The closing brackets, found the same way the parser found them.
        let after = body[link.position..]
            .find("]]")
            .map(|at| link.position + at + 2)
            .unwrap_or(body.len());
        last = after;
    }
    out.push_str(&body[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(body: &str) -> Vec<String> {
        parse(body).into_iter().map(|l| l.target_title).collect()
    }

    #[test]
    fn a_plain_link_points_at_its_title_and_shows_it() {
        let links = parse("See [[Liquidity risk]] for the rule.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_title, "Liquidity risk");
        assert_eq!(links[0].display_text, "Liquidity risk");
        assert_eq!(links[0].position, 4);
    }

    #[test]
    fn an_aliased_link_points_at_one_thing_and_shows_another() {
        let links = parse("See [[Liquidity risk|the liquidity rule]].");
        assert_eq!(links[0].target_title, "Liquidity risk");
        assert_eq!(links[0].display_text, "the liquidity rule");
    }

    #[test]
    fn two_links_to_one_page_are_two_links() {
        // A backlink panel that collapsed them would be lying about the second.
        assert_eq!(
            titles("[[A]] and later [[A]] again"),
            vec!["A".to_string(), "A".to_string()]
        );
    }

    #[test]
    fn a_link_inside_code_is_an_example_rather_than_a_reference() {
        // A vault that documents this feature is full of these.
        assert!(titles("Write `[[Title]]` to link.").is_empty());
        assert!(titles("```\n[[Title]]\n```\n").is_empty());
        assert_eq!(
            titles("```\n[[Ignored]]\n```\nBut [[Real]] counts."),
            vec!["Real".to_string()]
        );
    }

    #[test]
    fn punctuation_that_is_not_a_link_is_left_alone() {
        assert!(titles("[[]] is nothing").is_empty());
        assert!(titles("[[ | ]] is nothing either").is_empty());
        assert!(titles("An unclosed [[link").is_empty());
        assert!(titles("[[A\n\nB]]").is_empty(), "a link cannot span a paragraph");
        assert!(titles("A single [bracket] is a markdown link").is_empty());
    }

    #[test]
    fn stripping_leaves_the_sentence_a_retriever_should_index() {
        assert_eq!(
            strip("See [[Liquidity risk|the rule]] and [[Buffer]]."),
            "See the rule and Buffer."
        );
        assert_eq!(strip("No links here."), "No links here.");
    }

    #[test]
    fn a_position_finds_the_link_again_without_reparsing() {
        let body = "One [[A]] two [[B|b]] three";
        for link in parse(body) {
            assert!(
                body[link.position..].starts_with("[["),
                "the offset does not land on a link"
            );
        }
    }
}
