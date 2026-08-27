//! The bundle round trip. Doc 12 phase 10's acceptance.
//!
//! Doc 02 line 155 ships three boards marked for export, one of them carrying a
//! Concept term that collides with the importing profile. This runs every board
//! the corpus has through export and import and compares what arrived against
//! what left.
//!
//! Two profiles, always. One profile would prove that the writer and the reader
//! agree with each other, which is true whatever either of them does.

use std::io::Cursor;
use std::path::Path;

use rusqlite::params;
use tessera_bundle::{ExportOptions, export, import};
use tessera_schema::Registry;
use tessera_store::{Store, new_id, now_iso8601};

use crate::boards::Board;
use crate::boards;

/// What one board's round trip found.
pub struct Trip {
    pub board_id: String,
    pub marked_for_export: bool,
    pub sent: Counts,
    pub arrived: Counts,
    pub sources_merged: usize,
    pub concepts_collided: usize,
    pub note: String,
}

impl Trip {
    /// Whole means every row the sender had, the recipient has.
    pub fn whole(&self) -> bool {
        self.arrived.cards == self.sent.cards
            && self.arrived.citations == self.sent.citations
            && self.arrived.concepts >= self.sent.concepts
            // A source the recipient already had merges into theirs rather than
            // arriving again, so the count is what survived the merge.
            && self.arrived.sources + self.sources_merged >= self.sent.sources
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub cards: usize,
    pub citations: usize,
    pub sources: usize,
    pub concepts: usize,
}

fn counts(store: &Store, board_id: &str) -> Counts {
    let one = |sql: &str| -> usize {
        store
            .conn()
            .query_row(sql, [board_id], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    };
    Counts {
        cards: one("SELECT COUNT(*) FROM card WHERE board_id = ?1"),
        citations: one(
            "SELECT COUNT(*) FROM citation ci JOIN card c ON c.id = ci.card_id
             WHERE c.board_id = ?1",
        ),
        sources: one(
            "SELECT COUNT(DISTINCT p.source_id) FROM passage p
             JOIN citation ci ON ci.passage_id = p.id
             JOIN card c ON c.id = ci.card_id WHERE c.board_id = ?1",
        ),
        concepts: one(
            "SELECT COUNT(DISTINCT l.concept_id) FROM concept_link l
             JOIN card c ON c.id = l.target_ref
             WHERE l.target_type = 'card' AND c.board_id = ?1",
        ),
    }
}

/// Run every corpus board out and back, and report what changed.
pub fn run(corpus: &Path, snapshot: &str) -> Result<Vec<Trip>, String> {
    let boards = boards::load(corpus)?;
    if boards.is_empty() {
        return Err(format!("no boards under {}", corpus.display()));
    }
    let registry = Registry::load().map_err(|e| format!("schemas: {e}"))?;

    let (mut sender, ids) = seeded(&boards, snapshot)?;
    let mut trips = Vec::new();

    for board in &boards {
        let Some(board_id) = ids.get(&board.board_id).map(str::to_string) else {
            continue;
        };
        let sent = counts(&sender.store, &board_id);
        if sent.cards == 0 {
            continue;
        }

        // Every local document cleared: the corpus has no real ones, and the
        // checklist itself is covered by the unit tests in tessera-bundle.
        // Leaving it empty here would measure the checklist and call it a round
        // trip.
        let local = sender
            .store
            .conn()
            .prepare("SELECT id FROM source WHERE profile_id = ?1 AND class = 'local_document'")
            .and_then(|mut s| {
                s.query_map([&sender.profile], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();

        let options = ExportOptions {
            with_history: true,
            local_documents: local.into_iter().collect(),
            exported_by: Some("The corpus".into()),
        };

        let mut archive = Cursor::new(Vec::new());
        if let Err(e) = export(&mut sender.store, &registry, &board_id, &options, &mut archive) {
            trips.push(failed(board, sent, format!("export: {e}")));
            continue;
        }
        archive.set_position(0);

        // A fresh recipient per board, carrying the colliding term when the
        // corpus planted one. Doc 01 section 7's merge rule needs something to
        // merge with, and a recipient that already has the board would measure
        // the skip path instead.
        let mut receiver = match recipient(board) {
            Ok(r) => r,
            Err(e) => {
                trips.push(failed(board, sent, e));
                continue;
            }
        };

        let profile = receiver.profile.clone();
        match import(&mut receiver.store, &profile, archive) {
            Ok(outcome) => trips.push(Trip {
                board_id: board.board_id.clone(),
                marked_for_export: board.export_as_bundle,
                sent,
                arrived: counts(&receiver.store, &board_id),
                sources_merged: outcome.sources_merged,
                concepts_collided: outcome.concepts_collided,
                note: String::new(),
            }),
            Err(e) => trips.push(failed(board, sent, format!("import: {e}"))),
        }
    }

    Ok(trips)
}

fn failed(board: &Board, sent: Counts, note: String) -> Trip {
    Trip {
        board_id: board.board_id.clone(),
        marked_for_export: board.export_as_bundle,
        sent,
        arrived: Counts::default(),
        sources_merged: 0,
        concepts_collided: 0,
        note,
    }
}

struct Profile {
    store: Store,
    profile: String,
    pack: String,
}

fn empty_profile() -> Result<Profile, String> {
    let store = Store::open_in_memory().map_err(|e| e.to_string())?;
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());
    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'finance-eu-synthetic', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .map_err(|e| e.to_string())?;
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Profile { store, profile, pack })
}

/// The sender: every corpus board, with its concepts, under real ids.
///
/// Seeded here rather than through `boards::seed`, because the corpus files
/// boards under readable ids (`B-01`, `B-01-C03`) and doc 01 line 79 says
/// identifiers are ULIDs, "safe to merge across machines when bundles are
/// imported". The bundle is the first place that sentence has teeth: the
/// manifest schema types every id as a ULID and rejected the corpus spelling on
/// the first run. Translating here keeps the sweep's ids exactly as they were,
/// where readable ids are what makes a failure legible, and gives the round trip
/// the ids the product actually writes.
fn seeded(boards: &[Board], snapshot: &str) -> Result<(Profile, Ids), String> {
    let p = empty_profile()?;
    let mut ids = Ids::default();
    let now = now_iso8601();

    for board in boards {
        if !board.snapshot.is_empty() && board.snapshot.as_str() > snapshot {
            continue;
        }
        let board_id = ids.of(&board.board_id);
        p.store
            .conn()
            .execute(
                "INSERT INTO board (id, profile_id, title, named_by_user, doctrine_pack_id,
                     default_depth, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, 'deep', ?5, ?6, ?6)",
                params![
                    board_id,
                    p.profile,
                    board.title,
                    p.pack,
                    if board.trashed { "trashed" } else { "active" },
                    now
                ],
            )
            .map_err(|e| format!("board {}: {e}", board.board_id))?;

        for card in &board.cards {
            let card_id = ids.of(&card.card_id);
            p.store
                .conn()
                .execute(
                    "INSERT INTO card (id, board_id, kind, question, depth, answer, findings,
                         status, confidence, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                    params![
                        card_id,
                        board_id,
                        if card.kind.is_empty() { "root" } else { card.kind.as_str() },
                        card.question,
                        card.depth,
                        card.answer,
                        serde_json::to_string(&card.findings).unwrap_or_else(|_| "[]".into()),
                        card.status,
                        card.confidence,
                        now
                    ],
                )
                .map_err(|e| format!("card {}: {e}", card.card_id))?;

            for citation in &card.citations {
                // One Source per locator, as doc 01 section 4.9's dedupe key
                // says: two cards citing the same page cite one Source.
                let source_id = ids.of(&format!("src:{}", citation.locator));
                p.store
                    .conn()
                    .execute(
                        "INSERT INTO source (id, profile_id, class, title, locator, retrieved_at,
                             freshness_class, trust_rank, dedupe_key, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'slow', 50, ?7, ?6)
                         ON CONFLICT (id) DO NOTHING",
                        params![
                            source_id,
                            p.profile,
                            citation.source_class,
                            citation.source_title,
                            citation.locator,
                            now,
                            citation.locator
                        ],
                    )
                    .map_err(|e| format!("source {}: {e}", citation.locator))?;
                let passage_id = ids.of(&citation.passage_id);
                p.store
                    .conn()
                    .execute(
                        "INSERT INTO passage (id, source_id, text, retrieved_by, created_at)
                         VALUES (?1, ?2, ?3, 'corpus', ?4) ON CONFLICT (id) DO NOTHING",
                        params![passage_id, source_id, citation.source_title, now],
                    )
                    .map_err(|e| format!("passage {}: {e}", citation.passage_id))?;
                p.store
                    .conn()
                    .execute(
                        "INSERT INTO citation (id, card_id, ordinal, passage_id, claim_span,
                             binding, verifier_verdict, created_at)
                         VALUES (?1, ?2, ?3, ?4, '{}', 'answer', ?5, ?6)
                         ON CONFLICT (card_id, ordinal) DO NOTHING",
                        params![
                            new_id(),
                            card_id,
                            citation.ordinal,
                            passage_id,
                            if citation.verdict.is_empty() {
                                "unchecked"
                            } else {
                                citation.verdict.as_str()
                            },
                            now
                        ],
                    )
                    .map_err(|e| format!("citation {}: {e}", card.card_id))?;
            }
        }

        for concept in &board.concepts {
            let concept_id = ids.of(&concept.concept_id);
            write_concept(&p, &concept_id, &concept.term, "confirmed")?;
            for card in &concept.linked_cards {
                link_concept(&p, &concept_id, &ids.of(card))?;
            }
        }
    }

    Ok((p, ids))
}

/// Corpus ids to ULIDs, stable within one run.
///
/// A map rather than a hash of the name, because a ULID has to be a ULID and
/// nothing about `B-01` is one.
#[derive(Default)]
pub struct Ids(std::collections::BTreeMap<String, String>);

impl Ids {
    fn of(&mut self, corpus_id: &str) -> String {
        self.0
            .entry(corpus_id.to_string())
            .or_insert_with(new_id)
            .clone()
    }

    /// The id this run gave a corpus name, if it gave it one.
    pub fn get(&self, corpus_id: &str) -> Option<&str> {
        self.0.get(corpus_id).map(String::as_str)
    }
}

/// The recipient, holding the colliding term when the corpus planted one.
fn recipient(board: &Board) -> Result<Profile, String> {
    let p = empty_profile()?;
    if let Some(term) = &board.concept_collision {
        // A different id for the same word, which is exactly the case doc 01
        // section 7 rules on: two people meaning something by one term.
        write_concept(&p, &new_id(), term, "confirmed")?;
    }
    Ok(p)
}

fn write_concept(p: &Profile, id: &str, term: &str, status: &str) -> Result<(), String> {
    p.store
        .conn()
        .execute(
            "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (id) DO NOTHING",
            params![id, p.profile, term, p.pack, status, now_iso8601()],
        )
        .map(|_| ())
        .map_err(|e| format!("concept {term}: {e}"))
}

fn link_concept(p: &Profile, concept_id: &str, card_id: &str) -> Result<(), String> {
    let exists: i64 = p
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM card WHERE id = ?1", [card_id], |r| r.get(0))
        .unwrap_or(0);
    if exists == 0 {
        return Ok(());
    }
    p.store
        .conn()
        .execute(
            "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation,
                 proposed_by, status, created_at)
             VALUES (?1, ?2, 'card', ?3, 'mentions', 'corpus', 'confirmed', ?4)
             ON CONFLICT (id) DO NOTHING",
            params![new_id(), concept_id, card_id, now_iso8601()],
        )
        .map(|_| ())
        .map_err(|e| format!("link {concept_id}: {e}"))
}

/// The table the run prints. Doc 12 phase 10's acceptance, in one place.
pub fn report(trips: &[Trip]) -> String {
    let mut out = String::from(
        "| Board | Cards | Citations | Sources | Concepts | Merged | Collided | Whole |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for t in trips {
        out.push_str(&format!(
            "| {}{} | {}/{} | {}/{} | {}/{} | {}/{} | {} | {} | {} |\n",
            t.board_id,
            if t.marked_for_export { " *" } else { "" },
            t.arrived.cards,
            t.sent.cards,
            t.arrived.citations,
            t.sent.citations,
            t.arrived.sources,
            t.sent.sources,
            t.arrived.concepts,
            t.sent.concepts,
            t.sources_merged,
            t.concepts_collided,
            if t.whole() { "yes" } else { "no" },
        ));
        if !t.note.is_empty() {
            out.push_str(&format!("| | | | | | | | {} |\n", t.note));
        }
    }
    out
}
