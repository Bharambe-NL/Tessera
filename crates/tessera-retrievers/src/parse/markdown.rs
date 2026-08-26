//! Markdown, and the article aware path the regulatory retriever needs.
//!
//! Doc 05 section 8.1 chunks "by heading then by 800 character windows". Doc 05
//! section 8.3 adds that regulatory passage location is "article and
//! paragraph". Both are markdown here, so one parser serves both: a heading
//! that names an article turns its numbered items into paragraphs, and every
//! other heading is an ordinary section.
//!
//! This matters for citations rather than for retrieval. "Article 3, paragraph
//! 1" is something a reader can open the regulation and check. A character
//! offset is not.
//!
//! The numbering comes from the list structure and not from the text. A
//! regulation writes its paragraphs as `1.` and `2.`, which markdown reads as
//! an ordered list, so by the time the text reaches us the marker is gone and
//! only the parser still knows which item this was.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::chunking::{Chunk, ChunkLocation, windows};

/// A heading that names an article, for example `## Article 12`.
fn article_of(title: &str) -> Option<String> {
    let t = title.trim();
    let rest = t.strip_prefix("Article ").or_else(|| t.strip_prefix("article "))?;
    let number: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (!number.is_empty()).then_some(number)
}

/// One paragraph or list item of a section.
struct Block {
    text: String,
    /// `Some(n)` when this was item n of an ordered list.
    ordinal: Option<usize>,
}

struct Section {
    title: String,
    level: usize,
    blocks: Vec<Block>,
}

impl Section {
    fn empty() -> Self {
        Self { title: String::new(), level: 1, blocks: Vec::new() }
    }
    fn is_empty(&self) -> bool {
        self.title.is_empty() && self.blocks.is_empty()
    }
}

fn sections(source: &str) -> Vec<Section> {
    let parser = Parser::new_ext(source, Options::all());
    let mut out: Vec<Section> = Vec::new();
    let mut current = Section::empty();

    let mut heading_level: Option<usize> = None;
    let mut heading_text = String::new();
    let mut buffer = String::new();
    // The ordinal counter for each open ordered list, innermost last.
    let mut list_counters: Vec<Option<usize>> = Vec::new();
    let mut item_ordinal: Option<usize> = None;
    let mut in_item = false;

    let flush = |buffer: &mut String, ordinal: Option<usize>, blocks: &mut Vec<Block>| {
        let text = buffer.split_whitespace().collect::<Vec<_>>().join(" ");
        buffer.clear();
        if !text.is_empty() {
            blocks.push(Block { text, ordinal });
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut buffer, item_ordinal, &mut current.blocks);
                heading_level = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if !current.is_empty() {
                    out.push(std::mem::replace(&mut current, Section::empty()));
                }
                current.title = heading_text.trim().to_string();
                current.level = heading_level.take().unwrap_or(1);
            }

            Event::Start(Tag::List(start)) => list_counters.push(start.map(|s| s as usize)),
            Event::End(TagEnd::List(_)) => {
                list_counters.pop();
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                item_ordinal = match list_counters.last_mut() {
                    Some(Some(counter)) => {
                        let n = *counter;
                        *counter += 1;
                        Some(n)
                    }
                    _ => None,
                };
            }
            Event::End(TagEnd::Item) => {
                flush(&mut buffer, item_ordinal, &mut current.blocks);
                in_item = false;
                item_ordinal = None;
            }

            Event::End(TagEnd::Paragraph) => {
                // Inside a list item the paragraph is the item's body, and the
                // item end is what flushes it, so the ordinal is not lost.
                if !in_item {
                    flush(&mut buffer, None, &mut current.blocks);
                }
            }

            Event::Text(t) | Event::Code(t) => {
                if heading_level.is_some() {
                    heading_text.push_str(&t);
                } else {
                    buffer.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => buffer.push(' '),
            _ => {}
        }
    }

    flush(&mut buffer, item_ordinal, &mut current.blocks);
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn parse(source: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut sequence = 0usize;

    for section in sections(source) {
        match article_of(&section.title) {
            // Regulatory: each numbered item is its own citable paragraph, and
            // the unnumbered lead in is paragraph 0. That is the chapeau, the
            // sentence that qualifies everything under the article. Numbering
            // it 1 would collide with the real paragraph 1 and leave a citation
            // to "article 3, paragraph 1" pointing at either of two passages.
            Some(article) => {
                let mut last = 0usize;
                for block in &section.blocks {
                    let paragraph = block.ordinal.unwrap_or(last);
                    last = paragraph;
                    let location =
                        ChunkLocation::ArticleParagraph { article: article.clone(), paragraph };
                    let produced = windows(&block.text, &location, sequence);
                    sequence += produced.len();
                    out.extend(produced);
                }
            }
            None => {
                let body = section
                    .blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let location =
                    ChunkLocation::Heading { title: section.title.clone(), level: section.level };
                let produced = windows(&body, &location, sequence);
                sequence += produced.len();
                out.extend(produced);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGULATION: &str = "# Capital Adequacy Regulation 3 (v1)\n\n\
Issued by the Central Authority for Prudential Oversight.\n\n\
## Article 3\n\n\
This Article applies to institutions authorised under this Regulation.\n\n\
1. The minimum own funds requirement is 8.4 %. An institution shall document the assumptions.\n\n\
2. An institution shall keep an inventory of every model in regulatory use.\n";

    fn paragraphs(source: &str) -> Vec<(String, usize, String)> {
        parse(source)
            .into_iter()
            .filter_map(|c| match c.location {
                ChunkLocation::ArticleParagraph { article, paragraph } => {
                    Some((article, paragraph, c.text))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_numbered_item_keeps_the_number_the_regulation_gave_it() {
        let found = paragraphs(REGULATION);
        let one = found.iter().find(|(_, p, _)| *p == 1).expect("paragraph 1");
        assert_eq!(one.0, "3");
        assert!(one.2.contains("8.4 %"), "the planted value is missing: {:?}", one.2);

        let two = found.iter().find(|(_, p, _)| *p == 2).expect("paragraph 2");
        assert!(two.2.contains("inventory"), "{:?}", two.2);
    }

    #[test]
    fn the_chapeau_is_paragraph_zero_and_never_collides_with_paragraph_one() {
        let found = paragraphs(REGULATION);
        let zero = found.iter().find(|(_, p, _)| *p == 0).expect("the chapeau");
        assert!(zero.2.contains("applies to institutions"), "{:?}", zero.2);
        assert_eq!(
            found.iter().filter(|(_, p, _)| *p == 1).count(),
            1,
            "two passages both claim to be paragraph 1"
        );
    }

    #[test]
    fn prose_outside_an_article_keeps_its_heading() {
        assert!(parse(REGULATION).iter().any(|c| matches!(
            &c.location,
            ChunkLocation::Heading { title, .. } if title.contains("Capital Adequacy")
        )));
    }

    #[test]
    fn a_heading_that_merely_mentions_an_article_is_not_one() {
        let out = parse("## Articles of association\n\nSome prose about the company.\n");
        assert!(
            out.iter().all(|c| !matches!(c.location, ChunkLocation::ArticleParagraph { .. })),
            "a section was mistaken for a regulation article"
        );
    }

    #[test]
    fn an_empty_document_yields_nothing_rather_than_failing() {
        assert!(parse("").is_empty());
        assert!(parse("   \n\n  ").is_empty());
    }

    #[test]
    fn sequence_numbers_are_unique_across_a_document() {
        let out = parse(REGULATION);
        let mut seen: Vec<usize> = out.iter().map(|c| c.sequence).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "two chunks share a sequence number");
    }
}
