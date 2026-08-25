//! The house style, as a check rather than a document.
//!
//! `HANDOFF.md` section 7 states it: "no dashes of any kind, sentence case,
//! verbs name actions, no apologies. The owner's preference: no em dashes
//! anywhere and no 'it is not X, it is Y' constructions. Run these as a lint on
//! UI strings."
//!
//! Two of those five are mechanical, two are nearly so, and one is not. This
//! crate implements the four that can be checked and leaves "verbs name
//! actions" to review, because a checker that guesses at it would cry wolf on
//! every noun label a product legitimately has.
//!
//! Scope is deliberately narrow. It reads strings a user can see, not code and
//! not comments, with one exception: the dash rule runs over whole files,
//! because an em dash is wrong in a comment too and because the rule has no
//! false positives worth the escape hatch.

use std::fmt;

/// One thing the house style forbids, found at one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule: Rule,
    pub line: usize,
    /// The offending text, trimmed to something readable in a failure message.
    pub text: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}: {}", self.line, self.rule.explain(), self.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// An em dash, en dash, figure dash, or horizontal bar.
    Dash,
    /// A hyphen used as sentence punctuation, which is a dash by another name.
    SpacedHyphen,
    /// Title Case Where Sentence Case Belongs.
    TitleCase,
    /// "It is not X, it is Y".
    NotXButY,
    /// An apology.
    Apology,
}

impl Rule {
    pub fn explain(self) -> &'static str {
        match self {
            Rule::Dash => "no dashes of any kind",
            Rule::SpacedHyphen => "a spaced hyphen is a dash",
            Rule::TitleCase => "sentence case, not title case",
            Rule::NotXButY => "no \"it is not X, it is Y\" constructions",
            Rule::Apology => "no apologies",
        }
    }
}

/// Dashes used as punctuation.
///
/// U+2212 MINUS SIGN is absent on purpose. It is a mathematical symbol, and the
/// zoom control uses it as the counterpart to a plus. Forbidding it would fix a
/// glyph that was never prose.
const DASHES: [char; 4] = ['\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}'];

const APOLOGIES: [&str; 6] = [
    "sorry",
    "apologies",
    "apologise",
    "apologize",
    "we regret",
    "unfortunately",
];

/// Words that carry a capital in sentence case because they name something.
const PROPER_NOUNS: [&str; 24] = [
    "tessera",
    "home",
    "library",
    "profile",
    "flags",
    "trash",
    "board",
    "boards",
    "card",
    "cards",
    "fast",
    "deep",
    "research",
    "learn",
    "explore",
    "tidy",
    "router",
    "planner",
    "synthesizer",
    "visualizer",
    "verifier",
    "reader",
    "tutor",
    "exercise",
];

/// Every violation in one file's worth of source.
///
/// `strings` should be the user facing strings already extracted from the file;
/// the dash rules run over `source` in full.
pub fn violations(source: &str, strings: &[(usize, String)]) -> Vec<Violation> {
    let mut found = Vec::new();

    for (i, line) in source.lines().enumerate() {
        if let Some(c) = line.chars().find(|c| DASHES.contains(c)) {
            found.push(Violation {
                rule: Rule::Dash,
                line: i + 1,
                text: format!("{c:?} in: {}", trim(line)),
            });
        }
    }

    for (line, text) in strings {
        if has_spaced_hyphen(text) {
            found.push(Violation { rule: Rule::SpacedHyphen, line: *line, text: trim(text) });
        }
        if is_title_case(text) {
            found.push(Violation { rule: Rule::TitleCase, line: *line, text: trim(text) });
        }
        if has_not_x_but_y(text) {
            found.push(Violation { rule: Rule::NotXButY, line: *line, text: trim(text) });
        }
        if let Some(word) = apology(text) {
            found.push(Violation {
                rule: Rule::Apology,
                line: *line,
                text: format!("{word:?} in: {}", trim(text)),
            });
        }
    }

    found
}

fn trim(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 90 {
        format!("{}...", s.chars().take(87).collect::<String>())
    } else {
        s.to_string()
    }
}

/// A hyphen with a space on both sides is a dash doing punctuation's job.
/// A hyphen inside a word is spelling, so `follow-up` and `read-only` pass.
fn has_spaced_hyphen(s: &str) -> bool {
    s.contains(" - ") || s.ends_with(" -") || s.starts_with("- ")
}

fn apology(s: &str) -> Option<&'static str> {
    let lower = s.to_lowercase();
    APOLOGIES.into_iter().find(|w| lower.contains(w))
}

/// "It is not a warning, it is a block."
///
/// Matches the comma form the owner named. "not X but Y" without the comma is a
/// different sentence and is left alone.
fn has_not_x_but_y(s: &str) -> bool {
    let lower = s.to_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(" not ") {
        let start = from + rel + " not ".len();
        // A copula has to come before the "not" for this to be the construction
        // rather than an ordinary negation.
        let before = &lower[..from + rel];
        let copula = ["is", "are", "was", "were", "it's", "that's", "this is"]
            .iter()
            .any(|c| before.trim_end().ends_with(c));
        if copula {
            // The X runs to a comma, and a copula has to follow it.
            if let Some(comma) = lower[start..].find(',') {
                let x = &lower[start..start + comma];
                if !x.is_empty() && x.len() <= 80 && !x.contains('.') {
                    let after = lower[start + comma + 1..].trim_start();
                    let follows = ["is ", "are ", "was ", "were ", "it is", "it's", "this is", "that is"]
                        .iter()
                        .any(|c| after.starts_with(c))
                        || after
                            .split_whitespace()
                            .take(2)
                            .any(|w| ["is", "are", "was", "were", "it's"].contains(&w));
                    if follows {
                        return true;
                    }
                }
            }
        }
        from = start;
    }
    false
}

/// Title Case Detected By Counting Capitals.
///
/// Two or more capitalised words after the first, none of which name anything,
/// is title case. One is a proper noun nobody listed; zero is sentence case.
///
/// Only headings and labels are examined. The rule is about a heading written
/// Like This, and a paragraph is not a heading: run it over one and every word
/// that opens a second sentence reads as a capital in the middle of a line.
fn is_title_case(s: &str) -> bool {
    let s = s.trim();
    let heading_shaped = s.chars().count() <= 60
        && !s.contains(". ")
        && !s.ends_with('.')
        && !s.contains('?')
        && !s.contains('!');
    if !heading_shaped {
        return false;
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() < 3 {
        return false;
    }
    let capitalised = words
        .iter()
        .skip(1)
        .filter(|w| {
            let bare = w.trim_matches(|c: char| !c.is_alphanumeric());
            bare.chars().count() >= 2
                && bare.chars().next().is_some_and(char::is_uppercase)
                && bare.chars().skip(1).any(char::is_lowercase)
                && !PROPER_NOUNS.contains(&bare.to_lowercase().as_str())
        })
        .count();
    capitalised >= 2
}

/// What kind of file the strings are coming out of.
///
/// This exists because a heuristic that guesses is a lint nobody keeps. The
/// first version of this crate ran the HTML text node rule over TypeScript,
/// where `=>` reads as a closing tag, and reported four violations in code that
/// no user will ever see. Knowing the surface is what makes the extraction
/// sound rather than lucky.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Text nodes and the attributes that are read aloud or shown on hover.
    Html,
    /// Double quoted literals. Rust's `'` is a lifetime, so it is left alone.
    Rust,
    /// A designated copy module. Applied to nothing else, because a general
    /// TypeScript file is mostly selectors, keys and path data.
    Copy,
    /// A doctrine pack. Only the fields the product puts on screen.
    Pack,
}

/// Pull the strings a user could see out of a source file.
pub fn extract(source: &str, surface: Surface) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    match surface {
        Surface::Html => {
            for (i, line) in source.lines().enumerate() {
                for candidate in html_text(line).into_iter().chain(label_attributes(line)) {
                    if looks_like_prose(&candidate) {
                        out.push((i + 1, candidate));
                    }
                }
            }
        }
        Surface::Rust => {
            for (i, line) in source.lines().enumerate() {
                for candidate in double_quoted_runs(line) {
                    if looks_like_prose(&candidate) {
                        out.push((i + 1, candidate));
                    }
                }
            }
        }
        Surface::Copy => {
            for (i, line) in source.lines().enumerate() {
                for candidate in quoted_runs(line) {
                    let candidate = without_interpolation(&candidate);
                    if looks_like_prose(&candidate) {
                        out.push((i + 1, candidate));
                    }
                }
            }
        }
        Surface::Pack => out.extend(pack_copy(source)),
    }
    out
}

/// The fields of a doctrine pack that reach the screen.
///
/// `name` and `description` appear on the Doctrine page, and a flag rule's
/// `description` is the reason shown in the Flags queue. Everything else in a
/// pack is machine data: `issuer_pattern` holds a regulator's name, which is
/// title case because that is what the institution is called.
const PACK_COPY_KEYS: [&str; 3] = ["name", "description", "label"];

fn pack_copy(source: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        for key in PACK_COPY_KEYS {
            let prefix = format!("\"{key}\":");
            if let Some(rest) = trimmed.strip_prefix(&prefix)
                && let Some(value) = double_quoted_runs(rest).into_iter().next()
            {
                out.push((i + 1, value));
            }
        }
    }
    out
}

/// `aria-label`, `title`, `placeholder` and `alt`: read aloud or shown on hover.
fn label_attributes(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["aria-label=", "title=", "placeholder=", "alt="] {
        let mut rest = line;
        while let Some(at) = rest.find(attr) {
            rest = &rest[at + attr.len()..];
            if let Some(value) = double_quoted_runs(rest).into_iter().next() {
                out.push(value);
            }
        }
    }
    out
}

fn double_quoted_runs(line: &str) -> Vec<String> {
    quoted_runs_with(line, &['"'])
}

fn quoted_runs(line: &str) -> Vec<String> {
    quoted_runs_with(line, &['"', '\'', '`'])
}

fn quoted_runs_with(line: &str, delimiters: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if delimiters.contains(&c) {
            let mut j = i + 1;
            let mut buf = String::new();
            while j < chars.len() && chars[j] != c {
                if chars[j] == '\\' {
                    j += 1;
                }
                if j < chars.len() {
                    buf.push(chars[j]);
                }
                j += 1;
            }
            if j < chars.len() {
                out.push(buf);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn html_text(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(close) = rest.find('>') {
        rest = &rest[close + 1..];
        let end = rest.find('<').unwrap_or(rest.len());
        let text = rest[..end].trim();
        if !text.is_empty() {
            out.push(text.to_string());
        }
        rest = &rest[end..];
        if rest.is_empty() {
            break;
        }
    }
    out
}

/// Take the interpolation holes out of a template literal.
///
/// `Open ${PRODUCT_NAME} to ask a question` is one sentence with a name in the
/// middle, and the style rules are about the sentence. Left in, the hole reads
/// as a shouting word and the capital counting gets confused by it.
fn without_interpolation(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find("${") {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Two or more alphabetic words, and nothing that gives it away as machinery.
fn looks_like_prose(s: &str) -> bool {
    let s = s.trim();
    if s.contains("://") || s.starts_with('/') || s.starts_with('.') || s.starts_with('#') {
        return false;
    }
    // An event name or a dotted identifier, not a sentence.
    if s.contains(".v1") || s.contains("${") && !s.contains(' ') {
        return false;
    }
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .collect();
    words.len() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(s: &str) -> Vec<(usize, String)> {
        vec![(1, s.to_string())]
    }

    fn rules(source: &str, s: &str) -> Vec<Rule> {
        violations(source, &strings(s)).into_iter().map(|v| v.rule).collect()
    }

    #[test]
    fn an_em_dash_fails_wherever_it_appears() {
        assert_eq!(rules("a \u{2014} b", ""), vec![Rule::Dash]);
        assert_eq!(rules("// a comment \u{2013} with an en dash", ""), vec![Rule::Dash]);
    }

    #[test]
    fn a_minus_sign_passes_because_it_is_a_symbol() {
        // The zoom control's counterpart to a plus.
        assert!(rules("<button>\u{2212}</button>", "").is_empty());
    }

    #[test]
    fn a_hyphen_inside_a_word_passes_and_one_used_as_punctuation_does_not() {
        assert!(rules("", "Ask a follow-up question on this read-only board").is_empty());
        assert_eq!(rules("", "Verified - see sources"), vec![Rule::SpacedHyphen]);
    }

    #[test]
    fn title_case_fails_and_sentence_case_passes() {
        assert_eq!(rules("", "Export This Board As A Bundle"), vec![Rule::TitleCase]);
        assert!(rules("", "Export this board as a bundle").is_empty());
    }

    #[test]
    fn a_product_noun_does_not_make_a_string_title_case() {
        assert!(rules("", "Open this card in Library").is_empty());
        assert!(rules("", "Move this board to Trash").is_empty());
    }

    #[test]
    fn the_construction_the_owner_named_fails() {
        assert_eq!(
            rules("", "This is not a warning, it is a block"),
            vec![Rule::NotXButY]
        );
    }

    #[test]
    fn an_ordinary_negation_passes() {
        assert!(rules("", "This card is not verified").is_empty());
        assert!(rules("", "Fast is not available for regulatory questions").is_empty());
        assert!(rules("", "No sources were found, so nothing was cited").is_empty());
    }

    #[test]
    fn an_apology_fails() {
        assert_eq!(rules("", "Sorry, that folder could not be read"), vec![Rule::Apology]);
        assert_eq!(
            rules("", "Unfortunately the search key is missing"),
            vec![Rule::Apology]
        );
    }

    #[test]
    fn a_plain_report_of_failure_passes() {
        assert!(rules("", "That folder could not be read").is_empty());
    }

    #[test]
    fn a_violation_says_where_and_why() {
        let found = violations("", &[(7, "Export This Board Now".into())]);
        let message = found[0].to_string();
        assert!(message.contains("line 7"), "{message}");
        assert!(message.contains("sentence case"), "{message}");
    }
}
