//! Load the corpus's prior boards so memory has something to remember.
//!
//! Doc 02 section 6 ships twenty boards of already answered cards, and doc 15
//! measures the boards retriever against them: prior card recall 0.85, own card
//! sole support 0, stale propagation 0.95. Nothing loaded them, so every one of
//! those gates measured an empty index and reported 0.000, which reads as a
//! broken retriever rather than an unasked question. That is BN-019's shape and
//! its fifth appearance.
//!
//! These rows are fixtures standing in for history, so they are written
//! directly rather than replayed through events. A card the user answered last
//! month has an event trail on the machine that answered it, and inventing one
//! here would put fabricated provenance in the log the audit trail is supposed
//! to be. Importing someone else's cards with their ids is a real product path,
//! doc 01 section 7's bundle merge, and it arrives with M12.
//!
//! The ids are the corpus's own, `B-01` and `B-01-C03`, because doc 15's ground
//! truth names prior cards as `board_id/card_id` and the boards retriever's
//! document reference is exactly that. Keeping them makes the two comparable
//! without a translation table that could itself be wrong.

use std::path::Path;

use rusqlite::params;
use serde::Deserialize;
use serde_json::Value;
use tessera_store::Store;

#[derive(Debug, Deserialize)]
pub struct Board {
    pub board_id: String,
    pub title: String,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub snapshot: String,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub flags: Vec<Flag>,
    /// Doc 02 section 6's concepts, which the bundle round trip needs and the
    /// retrieval seed does not: memory indexes cards, not terms.
    #[serde(default)]
    pub concepts: Vec<Concept>,
    /// Doc 02 line 155: three boards ship as bundles.
    #[serde(default)]
    pub export_as_bundle: bool,
    /// The term this board's bundle collides with on import, when the corpus
    /// planted one. Doc 01 section 7's merge rule is what it tests.
    #[serde(default)]
    pub concept_collision: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Concept {
    pub concept_id: String,
    pub term: String,
    #[serde(default)]
    pub linked_cards: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Card {
    pub card_id: String,
    pub question: String,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub findings: Vec<Value>,
    pub depth: String,
    pub status: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub parent_card_id: Option<String>,
    #[serde(default)]
    pub anchor_text: Option<String>,
    #[serde(default)]
    pub builds_on: Vec<Value>,
    #[serde(default)]
    pub citations: Vec<Citation>,
    /// Doc 15 section 3's answer, planted by the generator. The retriever has to
    /// arrive at the same one from the card's own state.
    #[serde(default)]
    pub memory_eligible: bool,
    /// The ledger facts this card states. A re-verification carries them into
    /// its run record, because doc 02 section 10.2 scores staleness detection
    /// against cards whose facts were superseded, and without them that metric
    /// has an empty denominator and reports n/a forever.
    #[serde(default)]
    pub fact_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Citation {
    pub ordinal: i64,
    pub passage_id: String,
    pub locator: String,
    pub source_class: String,
    pub source_title: String,
    #[serde(default)]
    pub verdict: String,
}

#[derive(Debug, Deserialize)]
pub struct Flag {
    pub flag_id: String,
    pub card_id: String,
    pub rule_id: String,
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub reason: String,
}

/// What one pass of seeding did, so a run that loaded nothing says so.
#[derive(Debug, Default)]
pub struct SeedReport {
    pub boards: usize,
    pub cards: usize,
    pub indexed: usize,
    /// Cards the corpus called eligible that the retriever's own rule rejected,
    /// or the other way round. Doc 15 section 3 is the rule; a disagreement is
    /// a finding rather than something to paper over by trusting the label.
    pub eligibility_disagreements: Vec<String>,
}

pub fn load(corpus: &Path) -> Result<Vec<Board>, String> {
    let root = corpus.join("boards");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Sorted, so two runs seed in one order and the index is reproducible.
    dirs.sort();

    let mut boards = Vec::new();
    for dir in dirs {
        let path = dir.join("board.json");
        if !path.exists() {
            continue;
        }
        let body = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        boards.push(
            serde_json::from_str::<Board>(&body).map_err(|e| format!("{}: {e}", path.display()))?,
        );
    }
    Ok(boards)
}

/// Write the boards into the store and index the eligible ones.
///
/// Only boards at or before the run's snapshot. A T1 run must not remember a
/// card written at T3, which would be the harness handing the retriever an
/// answer from the future.
pub fn seed(
    store: &mut Store,
    profile_id: &str,
    pack_id: &str,
    boards: &[Board],
    snapshot: &str,
    embedder: Option<&dyn tessera_retrievers::embed::Embedder>,
) -> Result<SeedReport, String> {
    let now = tessera_store::now_iso8601();
    let mut report = SeedReport::default();

    for board in boards {
        if !at_or_before(&board.snapshot, snapshot) {
            continue;
        }
        let conn = store.conn();
        conn.execute(
            "INSERT INTO board (id, profile_id, title, named_by_user, doctrine_pack_id,
                 default_depth, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, 'deep', ?5, ?6, ?6)
             ON CONFLICT (id) DO NOTHING",
            params![
                board.board_id,
                profile_id,
                board.title,
                pack_id,
                if board.trashed { "trashed" } else { "active" },
                now
            ],
        )
        .map_err(|e| format!("board {}: {e}", board.board_id))?;
        report.boards += 1;

        for card in &board.cards {
            write_card(store, profile_id, &board.board_id, card, &now)
                .map_err(|e| format!("card {}: {e}", card.card_id))?;
            report.cards += 1;
        }

        // Flags after cards, because a block flag is what makes a card
        // ineligible and the eligibility check reads them.
        for flag in &board.flags {
            store
                .conn()
                .execute(
                    "INSERT INTO flag (id, card_id, rule_id, severity, target, reason,
                         status, created_at)
                     VALUES (?1, ?2, ?3, ?4, '{\"kind\":\"whole_card\"}', ?5, ?6, ?7)
                     ON CONFLICT (id) DO NOTHING",
                    params![
                        flag.flag_id,
                        flag.card_id,
                        flag.rule_id,
                        flag.severity,
                        flag.reason,
                        flag.status,
                        now
                    ],
                )
                .map_err(|e| format!("flag {}: {e}", flag.flag_id))?;
        }
    }

    // Indexing last, so every flag and every board status the eligibility rule
    // reads is already in place. Indexing as we went would remember a card that
    // a flag written moments later disqualifies.
    for board in boards {
        if !at_or_before(&board.snapshot, snapshot) {
            continue;
        }
        for card in &board.cards {
            let indexed = tessera_retrievers::boards::index_card(
                store.conn(),
                profile_id,
                &card.card_id,
                embedder,
            )
            .map_err(|e| format!("index {}: {e}", card.card_id))?;
            report.indexed += usize::from(indexed);
            if indexed != card.memory_eligible {
                report.eligibility_disagreements.push(format!(
                    "{} corpus says {}, doc 15 section 3 says {}",
                    card.card_id, card.memory_eligible, indexed
                ));
            }
        }
    }

    Ok(report)
}

fn write_card(
    store: &mut Store,
    profile_id: &str,
    board_id: &str,
    card: &Card,
    now: &str,
) -> rusqlite::Result<()> {
    let conn = store.conn();
    let findings = serde_json::to_string(&card.findings).unwrap_or_else(|_| "[]".into());
    let builds_on = serde_json::to_string(&card.builds_on).unwrap_or_else(|_| "[]".into());
    let kind = if card.kind.is_empty() { "root" } else { card.kind.as_str() };

    conn.execute(
        "INSERT INTO card (id, board_id, parent_card_id, kind, anchor_text, question, depth,
             answer, findings, status, confidence, builds_on, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
         ON CONFLICT (id) DO NOTHING",
        params![
            card.card_id,
            board_id,
            card.parent_card_id,
            kind,
            card.anchor_text,
            card.question,
            card.depth,
            card.answer,
            findings,
            card.status,
            card.confidence,
            builds_on,
            now
        ],
    )?;

    for citation in &card.citations {
        // One Source per locator, which is what doc 01 section 4.9's dedupe key
        // means: two cards citing the same page cite one Source.
        //
        // The locator is the one a retriever would record, relative to the
        // folder it indexes, not the one the corpus files it under. The corpus
        // writes `regulatory/reg-car3-v1.md` and the regulatory retriever
        // reaching the same file records `reg-car3-v1.md`. Importing the corpus
        // spelling would leave two Source rows for one document, so a card
        // answered today would never inherit the staleness a re-verification
        // found on the card that cited it first.
        let locator = retriever_locator(&citation.locator);
        let source_id = format!("src-{}", tessera_store::repo::normalise_locator(locator));
        conn.execute(
            "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
                 freshness_class, trust_rank, dedupe_key, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'slow', 50, ?7, ?6)
             ON CONFLICT (id) DO NOTHING",
            params![
                source_id,
                profile_id,
                citation.source_class,
                citation.source_title,
                locator,
                now,
                tessera_store::repo::normalise_locator(locator)
            ],
        )?;
        conn.execute(
            "INSERT INTO passage (id, source_id, text, retrieved_by, created_at)
             VALUES (?1, ?2, ?3, 'corpus', ?4)
             ON CONFLICT (id) DO NOTHING",
            params![citation.passage_id, source_id, Option::<String>::None, now],
        )?;
        conn.execute(
            "INSERT INTO citation (id, card_id, ordinal, passage_id, claim_span, binding,
                 verifier_verdict, created_at)
             VALUES (?1, ?2, ?3, ?4, '{}', 'answer', ?5, ?6)
             ON CONFLICT (card_id, ordinal) DO NOTHING",
            params![
                format!("{}-cite-{}", card.card_id, citation.ordinal),
                card.card_id,
                citation.ordinal,
                citation.passage_id,
                if citation.verdict.is_empty() { "unchecked" } else { citation.verdict.as_str() },
                now
            ],
        )?;
    }
    Ok(())
}

/// The locator a retriever would record for a corpus path.
///
/// The corpus files a document under its folder, `regulatory/reg-car3-v1.md`,
/// and each retriever indexes one of those folders, so what it records is the
/// path from that folder down. Stripping the first segment turns one into the
/// other. A path with no folder segment is already in retriever form.
fn retriever_locator(corpus_path: &str) -> &str {
    match corpus_path.split_once('/') {
        Some(("regulatory" | "internal" | "web", rest)) => rest,
        _ => corpus_path,
    }
}

/// Snapshot labels are `T1`, `T2`, `T3`, so string order is time order.
///
/// A board with no snapshot predates all of them and is always in scope.
fn at_or_before(board: &str, run: &str) -> bool {
    board.is_empty() || board <= run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corpus_path_becomes_the_locator_a_retriever_records() {
        assert_eq!(retriever_locator("regulatory/reg-car3-v1.md"), "reg-car3-v1.md");
        assert_eq!(retriever_locator("internal/Risk/int-model-09.pdf"), "Risk/int-model-09.pdf");
        assert_eq!(retriever_locator("web/site.invalid/page.html"), "site.invalid/page.html");
        assert_eq!(retriever_locator("reg-car3-v1.md"), "reg-car3-v1.md");
        assert_eq!(retriever_locator("B-01/B-01-C03"), "B-01/B-01-C03");
    }

    #[test]
    fn snapshots_order_as_time() {
        assert!(at_or_before("T1", "T1"));
        assert!(at_or_before("T1", "T3"));
        assert!(!at_or_before("T3", "T1"), "a T1 run remembered a card from the future");
        assert!(at_or_before("", "T1"), "an unlabelled board is older than every snapshot");
    }
}
