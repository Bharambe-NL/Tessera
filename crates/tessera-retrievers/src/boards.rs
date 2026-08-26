//! Indexing the profile's own cards. Doc 05 section 8.5, doc 15.
//!
//! The boards retriever queries the same index as local, so the only thing
//! that needed building is what goes in and when. Doc 05 section 8.5: "Indexes
//! the profile's own cards: question, answer, findings, visual labels,
//! embedded with the local alias, updated on `card.answered.v1`."
//!
//! Eligibility is the load-bearing part and doc 15 section 3 states it flatly:
//! "Only verified cards remember: done, deep or research, no open block flags,
//! board not trashed." Every clause is there for a reason. A fast card was
//! never checked against anything. A flagged card is one the user has not
//! decided about. A trashed board is one they threw away, and having it answer
//! questions afterwards would be the product ignoring a deletion.
//!
//! What gets indexed is a digest carrying the card's own citations, because
//! doc 15 section 2 is the whole design: a prior card is context, never
//! evidence, and the citations it carries are what the Verifier will demand
//! instead.

use rusqlite::{Connection, params};

use crate::chunking::{Chunk, ChunkLocation};
use crate::embed::Embedder;
use crate::index;

/// The folder id every card is indexed under. One folder, because a card is
/// not on disk and the boards retriever asks for all of them at once.
pub const BOARDS_FOLDER: &str = "boards";

/// Doc 15 section 3, as a query rather than as prose.
///
/// Written as SQL so the rule is evaluated where the data is, rather than
/// fetched and filtered in Rust where the four clauses could drift apart.
const ELIGIBLE: &str = "\
SELECT c.id, c.board_id, c.question, c.answer, c.findings
  FROM card c
  JOIN board b ON b.id = c.board_id
 WHERE c.id = ?1
   AND c.status = 'done'
   AND c.depth IN ('deep', 'research')
   AND b.status != 'trashed'
   AND NOT EXISTS (
        SELECT 1 FROM flag f
         WHERE f.card_id = c.id AND f.status = 'open' AND f.severity = 'block')";

/// Make sure the boards folder exists before anything is written into it.
pub fn ensure_folder(conn: &Connection, profile_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
         VALUES (?1, ?2, 'boards', 'Prior cards', ?3)
         ON CONFLICT DO NOTHING",
        params![BOARDS_FOLDER, profile_id, tessera_store::now_iso8601()],
    )?;
    Ok(())
}

/// Index one card if doc 15 section 3 says it may be remembered.
///
/// Returns whether it was indexed. A card that is not eligible is not an
/// error: most cards are not, and the ones that stop being eligible are
/// removed rather than left behind.
pub fn index_card(
    conn: &Connection,
    profile_id: &str,
    card_id: &str,
    embedder: Option<&dyn Embedder>,
) -> rusqlite::Result<bool> {
    ensure_folder(conn, profile_id)?;

    let row = conn
        .query_row(ELIGIBLE, params![card_id], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .ok();

    let Some((board_id, question, answer, findings)) = row else {
        // Not eligible, and it may have been eligible a moment ago: a card
        // that has just been flagged has to leave the index, not merely stop
        // being added to it.
        forget_card(conn, card_id)?;
        return Ok(false);
    };

    let digest = digest(&question, answer.as_deref(), findings.as_deref(), &citations(conn, card_id)?);

    index::write_document(
        conn,
        BOARDS_FOLDER,
        &reference(&board_id, card_id),
        &[Chunk::new(digest, ChunkLocation::Whole, 0)],
        embedder,
        &tessera_store::now_iso8601(),
    )?;
    Ok(true)
}

/// Take a card out of the index. Doc 15 section 3's eligibility can stop being
/// true after the fact: a flag is raised, a board is trashed.
pub fn forget_card(conn: &Connection, card_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM index_entry WHERE folder_id = ?1 AND document_chunk_ref LIKE ?2",
        params![BOARDS_FOLDER, format!("%/{card_id}#%")],
    )?;
    Ok(())
}

/// Re-index every card on a board, for when the board itself changed.
pub fn reindex_board(
    conn: &Connection,
    profile_id: &str,
    board_id: &str,
    embedder: Option<&dyn Embedder>,
) -> rusqlite::Result<usize> {
    let mut stmt = conn.prepare("SELECT id FROM card WHERE board_id = ?1")?;
    let ids: Vec<String> = stmt
        .query_map(params![board_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut indexed = 0;
    for id in ids {
        if index_card(conn, profile_id, &id, embedder)? {
            indexed += 1;
        }
    }
    Ok(indexed)
}

/// `board_id/card_id`, which is what doc 15's ground truth names a prior card
/// by and what `Card.builds_on` records.
pub fn reference(board_id: &str, card_id: &str) -> String {
    format!("{board_id}/{card_id}")
}

fn citations(conn: &Connection, card_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT s.title, s.locator FROM citation c
           JOIN passage p ON p.id = c.passage_id
           JOIN source s ON s.id = p.source_id
          WHERE c.card_id = ?1 ORDER BY c.ordinal",
    )?;
    stmt.query_map(params![card_id], |r| {
        Ok(format!("{} ({})", r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?
    .collect()
}

/// The text a prior card is remembered as.
///
/// The citations are in the digest on purpose. Doc 05 section 8.5: the passage
/// is "a rendered digest of the card with its own citations listed", and doc 15
/// section 2 says why: any number or rule in a new card must cite the original
/// passage, "which the boards passage carries in its digest". Without them the
/// prior card is a dead end, and citing it would be the loop the whole memory
/// rule exists to prevent.
pub fn digest(
    question: &str,
    answer: Option<&str>,
    findings: Option<&str>,
    citations: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Prior work on this profile's own board, for context only.\n");
    out.push_str("Question: ");
    out.push_str(question);
    out.push('\n');

    if let Some(answer) = answer {
        out.push_str("Answer: ");
        out.push_str(answer);
        out.push('\n');
    }

    if let Some(findings) = findings
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(findings)
        && let Some(items) = parsed.as_array()
    {
        for item in items {
            if let Some(text) = item["text"].as_str() {
                out.push_str("Finding: ");
                out.push_str(text);
                out.push('\n');
            }
        }
    }

    if citations.is_empty() {
        out.push_str("This card cited nothing, so it supports nothing.\n");
    } else {
        out.push_str("It was built from: ");
        out.push_str(&citations.join("; "));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tessera_store::{Store, repo};

    fn setup() -> (Store, String, String) {
        let mut store = Store::open_in_memory().expect("store");
        let now = tessera_store::now_iso8601();
        let (profile, pack) = (tessera_store::new_id(), tessera_store::new_id());
        store
            .conn()
            .execute(
                "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                     freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
                 VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
                params![pack, now],
            )
            .expect("pack");
        store
            .conn()
            .execute(
                "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                     retriever_config, created_at, updated_at)
                 VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
                params![profile, pack, now],
            )
            .expect("profile");
        let board = repo::create_board(
            &mut store,
            repo::NewBoard {
                profile_id: &profile,
                title: "A board",
                doctrine_pack_id: &pack,
                default_depth: "deep",
                named_by_user: false,
                parent_board_id: None,
                seed_label: None,
                context: None,
            },
        )
        .expect("board");
        (store, profile, board)
    }

    fn card(store: &mut Store, board: &str, depth: &str, status: &str) -> String {
        let id = repo::create_card(
            store,
            repo::NewCard {
                board_id: board,
                parent_card_id: None,
                kind: "root",
                question: "What is the capital conservation buffer?",
                depth,
                anchor_text: None,
                anchor_block_ref: None,
                audience_id: None,
            },
        )
        .expect("card");
        store
            .conn()
            .execute(
                "UPDATE card SET status = ?1, answer = 'The buffer is 2.5 %.' WHERE id = ?2",
                params![status, id],
            )
            .expect("status");
        id
    }

    #[test]
    fn a_done_deep_card_is_remembered() {
        let (mut store, profile, board) = setup();
        let id = card(&mut store, &board, "deep", "done");
        assert!(index_card(store.conn(), &profile, &id, None).expect("index"));

        let hits = index::search(store.conn(), &[BOARDS_FOLDER.into()], "capital conservation buffer", None, 5)
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].document_ref.starts_with(&format!("{board}/{id}")));
    }

    #[test]
    fn a_fast_card_is_never_remembered() {
        // Doc 15 section 3. A fast card was checked against nothing, so
        // recalling it later would launder model knowledge into evidence.
        let (mut store, profile, board) = setup();
        let id = card(&mut store, &board, "fast", "done");
        assert!(!index_card(store.conn(), &profile, &id, None).expect("index"));
    }

    #[test]
    fn a_flagged_card_is_never_remembered() {
        let (mut store, profile, board) = setup();
        let id = card(&mut store, &board, "deep", "flagged");
        assert!(!index_card(store.conn(), &profile, &id, None).expect("index"));
    }

    #[test]
    fn a_card_with_an_open_block_flag_is_never_remembered() {
        let (mut store, profile, board) = setup();
        let id = card(&mut store, &board, "deep", "done");
        store
            .conn()
            .execute(
                "INSERT INTO flag (id, card_id, rule_id, severity, target, reason, status, created_at)
                 VALUES (?1, ?2, 'own_card_sole_support', 'block', '{}', 'r', 'open', 'now')",
                params![tessera_store::new_id(), id],
            )
            .expect("flag");
        assert!(!index_card(store.conn(), &profile, &id, None).expect("index"));
    }

    #[test]
    fn a_card_on_a_trashed_board_is_forgotten_rather_than_left_behind() {
        // The failure this prevents: a user throws a board away and its cards
        // keep answering questions, which is the product ignoring a deletion.
        let (mut store, profile, board) = setup();
        let id = card(&mut store, &board, "deep", "done");
        assert!(index_card(store.conn(), &profile, &id, None).expect("index"));

        store
            .conn()
            .execute("UPDATE board SET status = 'trashed' WHERE id = ?1", params![board])
            .expect("trash");
        assert!(!index_card(store.conn(), &profile, &id, None).expect("reindex"));

        let hits = index::search(store.conn(), &[BOARDS_FOLDER.into()], "capital conservation buffer", None, 5)
            .expect("search");
        assert!(hits.is_empty(), "a trashed board still answered");
    }

    #[test]
    fn the_digest_carries_the_cards_own_citations() {
        // Doc 15 section 2: any number in a new card must cite the original
        // passage, which the boards passage carries in its digest. Without
        // them the prior card is a dead end and citing it is the loop the
        // memory rule exists to prevent.
        let text = digest(
            "What is the buffer?",
            Some("The buffer is 2.5 %."),
            Some(&json!([{ "text": "It applies from 2026." }]).to_string()),
            &["CAR3 v1 (reg-car3-v1.md)".to_string()],
        );
        assert!(text.contains("context only"), "{text}");
        assert!(text.contains("CAR3 v1"), "the original citation is missing: {text}");
        assert!(text.contains("It applies from 2026."));
    }

    #[test]
    fn a_card_that_cited_nothing_says_so() {
        let text = digest("A question", Some("An answer."), None, &[]);
        assert!(text.contains("supports nothing"), "{text}");
    }
}
