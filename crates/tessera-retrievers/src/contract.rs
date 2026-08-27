//! What every retriever shares. Doc 05 sections 4, 5, 9 and 10.
//!
//! Five connectors, one shape. A retriever knows how to turn a query into
//! passages from one kind of place; everything else, the hooks, the trust
//! ranks, the confidence arithmetic, the failure taxonomy, the persistence and
//! the events, lives here and is the same for all of them. That is what lets
//! the harness fan out over a list of assignments without knowing what any
//! entry talks to.
//!
//! Two rules from doc 05 section 5 are enforced here rather than trusted to
//! each connector: passage text is capped, and `trust_rank` comes from doctrine
//! and never from the retriever's own opinion of what it found.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chunking::MAX_PASSAGE_CHARS;

/// One assignment, as doc 05 section 4 shapes it.
#[derive(Debug, Clone, Deserialize)]
pub struct Packet {
    pub run_id: String,
    #[serde(default)]
    pub card_id: Option<String>,
    #[serde(default)]
    pub sq_id: Option<String>,
    pub retriever_id: String,
    pub query: String,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default = "default_max_passages")]
    pub max_passages: usize,
    #[serde(default)]
    pub must_exclude: Vec<String>,
    /// Doc 17 section 5: locators a lesson was told to read first. Not the twin
    /// of `must_exclude`: an exclusion is a rule a retriever may not break, and
    /// this is a place it is told to look. A locator that answers nothing still
    /// contributes nothing.
    #[serde(default)]
    pub must_include: Vec<String>,
    #[serde(default)]
    pub doctrine: Doctrine,
}

fn default_max_passages() -> usize {
    12
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Filters {
    #[serde(default)]
    pub corpus: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub version_ref: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub exclude_board_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Doctrine {
    #[serde(default)]
    pub trust_ranks: Vec<TrustRank>,
    #[serde(default)]
    pub denied_domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrustRank {
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub issuer_pattern: Option<String>,
    pub rank: i64,
}

impl Doctrine {
    /// The rank doctrine gives a source. Doc 05 section 5: never the
    /// retriever's own judgment.
    ///
    /// The most specific rule wins, which means an issuer pattern beats a bare
    /// class. A pack lists "regulatory from this authority is rank 1" and
    /// "regulatory is rank 2", and reading them in file order would give both
    /// the same answer.
    pub fn rank_for(&self, class: &str, issuer: Option<&str>) -> i64 {
        let mut best: Option<(u8, i64)> = None;
        for rule in &self.trust_ranks {
            let class_matches = rule.class.as_deref().is_none_or(|c| c == class);
            if !class_matches {
                continue;
            }
            let specificity = match (&rule.issuer_pattern, issuer) {
                (Some(pattern), Some(name)) if name.contains(pattern.as_str()) => 2,
                (Some(_), _) => continue,
                (None, _) => 1,
            };
            if best.is_none_or(|(s, _)| specificity > s) {
                best = Some((specificity, rule.rank));
            }
        }
        // Doc 01 section 4.8's hierarchy bottoms out at 8, so an unranked
        // source sorts last rather than first.
        best.map(|(_, rank)| rank).unwrap_or(9)
    }
}

/// One retrieved passage, with the source that produced it.
#[derive(Debug, Clone, Serialize)]
pub struct Passage {
    pub passage_id: String,
    pub source_id: String,
    pub text: String,
    pub location: Value,
    pub score: f64,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub class: String,
    pub title: String,
    pub locator: String,
    pub issuer: Option<String>,
    pub published_at: Option<String>,
    pub trust_rank: i64,
    pub freshness_class: String,
    pub version_ref: Option<String>,
    pub content_hash: String,
}

/// Doc 05 section 5's coverage, which is what confidence is mostly built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Full,
    Partial,
    None,
}

/// What a retriever returns. Doc 05 section 5.
#[derive(Debug, Clone)]
pub struct Retrieved {
    pub passages: Vec<Passage>,
    pub sources_created: usize,
    pub sources_deduplicated: usize,
    pub coverage: Coverage,
    pub exclusions_applied: Vec<String>,
    pub caveats: Vec<String>,
    /// Set when the connector rewrote the query, which costs a tenth of the
    /// confidence. Doc 05 section 9.
    pub query_rewritten: bool,
    pub fetch_errors: usize,
}

impl Default for Retrieved {
    fn default() -> Self {
        Self {
            passages: Vec::new(),
            sources_created: 0,
            sources_deduplicated: 0,
            coverage: Coverage::None,
            exclusions_applied: Vec::new(),
            caveats: Vec::new(),
            query_rewritten: false,
            fetch_errors: 0,
        }
    }
}

impl Retrieved {
    /// Doc 05 section 9, verbatim: coverage full is +0.4, at least one trust
    /// rank 1 or 2 source is +0.3, no fetch errors is +0.2, query not rewritten
    /// is +0.1.
    ///
    /// Deterministic on purpose. A retriever that scored its own work would be
    /// the one agent in the pipeline whose confidence nothing could check, and
    /// doc 05 section 9 is clear that the Synthesizer weighs passages by trust
    /// rank and score rather than by this number.
    pub fn confidence(&self) -> f64 {
        let mut score: f64 = 0.0;
        if self.coverage == Coverage::Full {
            score += 0.4;
        }
        if self.passages.iter().any(|p| p.source.trust_rank <= 2) {
            score += 0.3;
        }
        if self.fetch_errors == 0 {
            score += 0.2;
        }
        if !self.query_rewritten {
            score += 0.1;
        }
        (score * 100.0).round() / 100.0
    }

    /// The output shape doc 05 section 5 declares, ready for the schema guard.
    pub fn to_output(&self, packet: &Packet) -> Value {
        json!({
            "schema_version": "1.0",
            "agent_id": format!("retriever.{}", packet.retriever_id),
            "run_id": packet.run_id,
            "sq_id": packet.sq_id,
            "passages": self.passages,
            "sources_created": self.sources_created,
            "sources_deduplicated": self.sources_deduplicated,
            "coverage": self.coverage,
            "exclusions_applied": self.exclusions_applied,
            "confidence": self.confidence(),
            "caveats": self.caveats,
        })
    }
}

/// Cap passage text. Doc 05 section 5: 1,200 characters, longer spans split.
///
/// Applied centrally rather than per connector, because a cap each connector
/// remembers separately is a cap one of them forgets.
pub fn cap(text: &str) -> String {
    if text.chars().count() <= MAX_PASSAGE_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_PASSAGE_CHARS).collect()
}

/// Re-exported so a connector does not have to know which crate owns it.
///
/// The definition lives in `tessera_store::repo`, because that crate owns the
/// uniqueness constraint the key feeds. Two copies would drift.
pub use tessera_store::repo::normalise_locator as dedupe_key;

#[cfg(test)]
mod tests {
    use super::*;

    fn doctrine() -> Doctrine {
        Doctrine {
            trust_ranks: vec![
                TrustRank {
                    class: Some("regulatory".into()),
                    issuer_pattern: Some("Central Authority".into()),
                    rank: 1,
                },
                TrustRank {
                    class: Some("regulatory".into()),
                    issuer_pattern: None,
                    rank: 2,
                },
                TrustRank {
                    class: Some("local_document".into()),
                    issuer_pattern: None,
                    rank: 4,
                },
            ],
            denied_domains: Vec::new(),
        }
    }

    #[test]
    fn the_more_specific_doctrine_rule_wins() {
        let d = doctrine();
        assert_eq!(
            d.rank_for("regulatory", Some("Central Authority for Prudential Oversight")),
            1
        );
        assert_eq!(d.rank_for("regulatory", Some("Some Other Regulator")), 2);
        assert_eq!(d.rank_for("local_document", None), 4);
    }

    #[test]
    fn an_unranked_source_sorts_last_rather_than_first() {
        // The failure this prevents: a class doctrine says nothing about
        // defaulting to rank 0 and outranking the regulation.
        assert_eq!(doctrine().rank_for("web", None), 9);
    }

    #[test]
    fn mirrored_locators_share_one_dedupe_key() {
        let expected = "ledgerline.invalid/capital/buffers";
        for locator in [
            "https://ledgerline.invalid/capital/buffers",
            "http://www.ledgerline.invalid/capital/buffers/",
            "HTTPS://Ledgerline.Invalid/capital/buffers?utm_source=news",
            "https://ledgerline.invalid/capital/buffers#section-2",
        ] {
            assert_eq!(dedupe_key(locator), expected, "{locator} did not normalise");
        }
    }

    #[test]
    fn different_pages_keep_different_keys() {
        assert_ne!(
            dedupe_key("https://a.invalid/one"),
            dedupe_key("https://a.invalid/two")
        );
    }

    #[test]
    fn confidence_follows_doc_05_section_9_exactly() {
        let passage = |rank: i64| Passage {
            passage_id: "p".into(),
            source_id: "s".into(),
            text: "t".into(),
            location: json!({}),
            score: 1.0,
            source: Source {
                class: "regulatory".into(),
                title: "T".into(),
                locator: "l".into(),
                issuer: None,
                published_at: None,
                trust_rank: rank,
                freshness_class: "regulation".into(),
                version_ref: None,
                content_hash: "h".into(),
            },
        };

        let best = Retrieved {
            passages: vec![passage(1)],
            coverage: Coverage::Full,
            ..Default::default()
        };
        assert!((best.confidence() - 1.0).abs() < 1e-9);

        let no_trusted_source = Retrieved {
            passages: vec![passage(7)],
            coverage: Coverage::Full,
            ..Default::default()
        };
        assert!((no_trusted_source.confidence() - 0.7).abs() < 1e-9);

        let nothing = Retrieved::default();
        // Coverage none, no sources, but no errors and no rewrite either.
        assert!((nothing.confidence() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_fetch_error_and_a_rewrite_each_cost_what_the_spec_says() {
        let base = Retrieved {
            coverage: Coverage::Full,
            ..Default::default()
        };
        assert!((base.confidence() - 0.7).abs() < 1e-9);

        let with_error = Retrieved {
            fetch_errors: 1,
            ..base.clone()
        };
        assert!((with_error.confidence() - 0.5).abs() < 1e-9);

        let rewritten = Retrieved {
            query_rewritten: true,
            ..base.clone()
        };
        assert!((rewritten.confidence() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn passage_text_is_capped_centrally() {
        let long = "x".repeat(MAX_PASSAGE_CHARS + 500);
        assert_eq!(cap(&long).chars().count(), MAX_PASSAGE_CHARS);
        assert_eq!(cap("short"), "short");
    }
}
