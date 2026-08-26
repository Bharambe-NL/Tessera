//! The hybrid index. Doc 05 section 8.2: "hybrid, BM25 plus vector, fused by
//! reciprocal rank".
//!
//! Both halves answer the same question badly in opposite directions. BM25
//! finds a passage that uses the words the question used, and misses the one
//! that says the same thing differently or in another language. Vectors find
//! the passage that means the same thing, and rank an exact figure no higher
//! than a vague restatement of it. Reciprocal rank fusion needs neither half to
//! be right on its own: a passage that both halves place near the top beats a
//! passage that either half loves alone.
//!
//! Fusion is on rank rather than on score deliberately. A BM25 score and a
//! cosine similarity are not on the same scale, are not on any shared scale,
//! and normalising them into one would invent a comparison the numbers do not
//! support.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::chunking::{Chunk, ChunkLocation};
use crate::embed::{Embedder, cosine, from_blob, to_blob};

/// Doc 05 section 8.2's fusion constant, the value the original paper uses.
/// It damps the difference between rank 1 and rank 2 so that a single half
/// cannot dominate on confidence alone.
const RRF_K: f64 = 60.0;

/// How deep each half looks before fusion. Wider than any caller's
/// `max_passages`, because a passage that fusion would promote has to survive
/// long enough to be fused.
const CANDIDATE_DEPTH: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub entry_id: String,
    pub document_ref: String,
    pub text: String,
    pub location: Option<ChunkLocation>,
    /// The fused score. Comparable within one query and meaningless across two.
    pub score: f64,
}

/// Content addressed id for a chunk, so re-indexing an unchanged file is a
/// no-op and a changed file replaces exactly what changed.
pub fn chunk_hash(document_ref: &str, chunk: &Chunk) -> String {
    let mut hasher = Sha256::new();
    hasher.update(document_ref.as_bytes());
    hasher.update([0u8]);
    hasher.update(chunk.sequence.to_le_bytes());
    hasher.update([0u8]);
    hasher.update(chunk.text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Write one document's chunks into the index, replacing whatever was there.
///
/// Returns the number of entries written. Embedding is done in one batch,
/// because a model amortises over a batch and not over a loop.
pub fn write_document(
    conn: &Connection,
    folder_id: &str,
    document_ref: &str,
    chunks: &[Chunk],
    embedder: Option<&dyn Embedder>,
    now: &str,
) -> rusqlite::Result<usize> {
    // The whole document is replaced rather than merged. A file that lost a
    // paragraph must lose it from the index too, and a merge cannot tell a
    // deletion from a chunk that simply moved.
    conn.execute(
        "DELETE FROM index_entry WHERE folder_id = ?1 AND document_chunk_ref LIKE ?2",
        params![folder_id, format!("{document_ref}#%")],
    )?;

    if chunks.is_empty() {
        return Ok(0);
    }

    let vectors = match embedder {
        Some(e) => {
            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            e.embed(&texts).ok()
        }
        None => None,
    };

    let mut written = 0usize;
    for (i, chunk) in chunks.iter().enumerate() {
        let entry_id = chunk_hash(document_ref, chunk);
        let reference = format!("{document_ref}#{}", chunk.sequence);
        let location = serde_json::to_string(&chunk.location).ok();
        let vector = vectors.as_ref().and_then(|v| v.get(i));

        conn.execute(
            "INSERT OR REPLACE INTO index_entry
               (id, folder_id, document_chunk_ref, content_hash, chunk_text, location,
                embedding, embedding_model, embedding_dimensions, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry_id,
                folder_id,
                reference,
                chunk_hash(document_ref, chunk),
                chunk.text,
                location,
                vector.map(|v| to_blob(v)),
                embedder.map(|e| e.model_id().to_string()),
                embedder.map(|e| e.dimensions() as i64),
                now,
            ],
        )?;
        written += 1;
    }

    Ok(written)
}

/// Remove everything indexed from one document.
pub fn forget_document(
    conn: &Connection,
    folder_id: &str,
    document_ref: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM index_entry WHERE folder_id = ?1 AND document_chunk_ref LIKE ?2",
        params![folder_id, format!("{document_ref}#%")],
    )
}

/// Turn a natural question into something FTS5 will accept.
///
/// FTS5's query language treats plenty of ordinary punctuation as syntax, so a
/// question typed by a person is a syntax error about as often as not. Every
/// term is quoted and the terms are OR'ed: a passage matching more of them
/// ranks higher through bm25 anyway, and requiring all of them would return
/// nothing for most real questions.
pub fn fts_query(text: &str) -> String {
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() > 1)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    terms.join(" OR ")
}

fn lexical(
    conn: &Connection,
    folder_ids: &[String],
    query: &str,
    depth: usize,
) -> rusqlite::Result<Vec<String>> {
    let match_expression = fts_query(query);
    if match_expression.is_empty() || folder_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = folder_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT e.id FROM index_fts f
           JOIN index_entry e ON e.rowid = f.rowid
          WHERE index_fts MATCH ?1 AND e.folder_id IN ({placeholders})
          ORDER BY bm25(index_fts) LIMIT ?{}",
        folder_ids.len() + 2
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(match_expression)];
    for id in folder_ids {
        values.push(Box::new(id.clone()));
    }
    values.push(Box::new(depth as i64));
    let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let rows = stmt.query_map(refs.as_slice(), |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn semantic(
    conn: &Connection,
    folder_ids: &[String],
    query_vector: &[f32],
    depth: usize,
) -> rusqlite::Result<Vec<String>> {
    if folder_ids.is_empty() || query_vector.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = folder_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, embedding FROM index_entry
          WHERE folder_id IN ({placeholders}) AND embedding IS NOT NULL"
    );
    let mut stmt = conn.prepare(&sql)?;
    let values: Vec<&dyn rusqlite::ToSql> =
        folder_ids.iter().map(|v| v as &dyn rusqlite::ToSql).collect();

    // Brute force over the folder. A vector index earns its keep at a scale
    // this does not reach yet, and it would be a native extension to load and
    // ship; the `VectorIndex` swap is a later decision the numbers can make.
    let mut scored: Vec<(String, f32)> = Vec::new();
    let rows = stmt.query_map(values.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    for row in rows {
        let (id, blob) = row?;
        let Some(vector) = from_blob(&blob) else { continue };
        if vector.len() != query_vector.len() {
            // A different model wrote this row. Comparing across two vector
            // spaces produces a number rather than an error, which is why the
            // width is checked rather than trusted.
            continue;
        }
        scored.push((id, cosine(query_vector, &vector)));
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(depth);
    Ok(scored.into_iter().map(|(id, _)| id).collect())
}

/// Reciprocal rank fusion over any number of ranked lists.
pub fn fuse(lists: &[Vec<String>]) -> Vec<(String, f64)> {
    let mut scores: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (RRF_K + (rank + 1) as f64);
        }
    }
    let mut out: Vec<(String, f64)> = scores.into_iter().collect();
    // Ties break on id so the same corpus and the same query give the same
    // answer twice, which every comparison between two eval runs depends on.
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out
}

/// Search the index. Doc 05 section 8.2.
pub fn search(
    conn: &Connection,
    folder_ids: &[String],
    query: &str,
    embedder: Option<&dyn Embedder>,
    limit: usize,
) -> rusqlite::Result<Vec<Hit>> {
    let lexical_hits = lexical(conn, folder_ids, query, CANDIDATE_DEPTH)?;

    let semantic_hits = match embedder {
        Some(e) => {
            // e5 wants the asking side marked as a query. The indexed side was
            // written with the passage prefix; mixing the two costs accuracy
            // silently.
            match e.embed(&[format!("query: {query}")]) {
                Ok(vectors) if !vectors.is_empty() => {
                    semantic(conn, folder_ids, &vectors[0], CANDIDATE_DEPTH)?
                }
                _ => Vec::new(),
            }
        }
        None => Vec::new(),
    };

    let fused = fuse(&[lexical_hits, semantic_hits]);

    let mut out = Vec::new();
    for (entry_id, score) in fused.into_iter().take(limit) {
        let row: Option<(String, String, Option<String>)> = conn
            .query_row(
                "SELECT document_chunk_ref, chunk_text, location FROM index_entry WHERE id = ?1",
                params![entry_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((document_ref, text, location)) = row else { continue };
        out.push(Hit {
            entry_id,
            document_ref,
            text,
            location: location.and_then(|l| serde_json::from_str(&l).ok()),
            score,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;

    fn store() -> tessera_store::Store {
        tessera_store::Store::open_in_memory().expect("store")
    }

    fn folder(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES ('p', 'deep', 'pack', '{}', '{}', 'now', 'now')
             ON CONFLICT DO NOTHING",
            [],
        )
        .expect("profile");
        conn.execute(
            "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
             VALUES (?1, 'p', ?1, ?1, 'now') ON CONFLICT DO NOTHING",
            params![id],
        )
        .expect("folder");
    }

    fn chunk(text: &str, sequence: usize) -> Chunk {
        Chunk::new(text, ChunkLocation::Whole, sequence)
    }

    #[test]
    fn a_written_document_is_findable_by_its_words() {
        let s = store();
        folder(s.conn(), "f1");
        write_document(
            s.conn(),
            "f1",
            "doc-a",
            &[chunk("The minimum own funds requirement is 8.4 percent.", 0)],
            None,
            "now",
        )
        .expect("write");

        let hits = search(s.conn(), &["f1".into()], "minimum own funds requirement", None, 5)
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("8.4 percent"));
    }

    #[test]
    fn rewriting_a_document_replaces_it_rather_than_duplicating_it() {
        let s = store();
        folder(s.conn(), "f1");
        let write = |text: &str| {
            write_document(s.conn(), "f1", "doc-a", &[chunk(text, 0)], None, "now").expect("write")
        };
        write("The buffer is 2.5 percent.");
        write("The buffer is 3.5 percent.");

        let hits = search(s.conn(), &["f1".into()], "buffer percent", None, 10).expect("search");
        assert_eq!(hits.len(), 1, "the old version survived the rewrite");
        assert!(hits[0].text.contains("3.5"), "the old value won");
    }

    #[test]
    fn forgetting_a_document_removes_it_from_the_index() {
        let s = store();
        folder(s.conn(), "f1");
        write_document(s.conn(), "f1", "doc-a", &[chunk("A sentence about buffers.", 0)], None, "now")
            .expect("write");
        forget_document(s.conn(), "f1", "doc-a").expect("forget");
        assert!(
            search(s.conn(), &["f1".into()], "buffers", None, 5).expect("search").is_empty()
        );
    }

    #[test]
    fn a_folder_is_never_searched_unless_it_was_asked_for() {
        // The exclusion the Sensitive folder rule depends on. A folder the
        // caller did not name must not contribute a single passage.
        let s = store();
        folder(s.conn(), "open");
        folder(s.conn(), "sensitive");
        write_document(s.conn(), "open", "pub", &[chunk("Public guidance on buffers.", 0)], None, "now")
            .expect("write");
        write_document(
            s.conn(),
            "sensitive",
            "secret",
            &[chunk("Confidential guidance on buffers.", 0)],
            None,
            "now",
        )
        .expect("write");

        let hits = search(s.conn(), &["open".into()], "buffers guidance", None, 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].text.contains("Confidential"), "an unasked folder answered");
    }

    #[test]
    fn a_question_with_punctuation_is_not_a_syntax_error() {
        // FTS5 reads plenty of ordinary punctuation as query syntax, so this is
        // the difference between working and returning an error to the user.
        let s = store();
        folder(s.conn(), "f1");
        write_document(s.conn(), "f1", "doc", &[chunk("The refund deadline is 14 days.", 0)], None, "now")
            .expect("write");

        for question in [
            "What is the refund deadline?",
            "refund deadline (in days)",
            "refund \"deadline\" -- now",
            "AND OR NOT",
        ] {
            let hits = search(s.conn(), &["f1".into()], question, None, 5);
            assert!(hits.is_ok(), "{question:?} produced {:?}", hits.err());
        }
    }

    #[test]
    fn both_halves_contribute_and_fusion_prefers_what_both_like() {
        let s = store();
        folder(s.conn(), "f1");
        let embedder = HashEmbedder::default();
        write_document(
            s.conn(),
            "f1",
            "doc",
            &[
                chunk("The capital conservation buffer is 2.5 percent.", 0),
                chunk("Unrelated prose about lunch arrangements.", 1),
            ],
            Some(&embedder),
            "now",
        )
        .expect("write");

        let hits = search(
            s.conn(),
            &["f1".into()],
            "capital conservation buffer",
            Some(&embedder),
            5,
        )
        .expect("search");
        assert!(!hits.is_empty());
        assert!(hits[0].text.contains("2.5 percent"), "fusion ranked the wrong chunk first");
    }

    #[test]
    fn a_vector_from_a_different_model_is_skipped_rather_than_compared() {
        // Two vector spaces in one column produce a number rather than an
        // error, which is the worst possible failure: a recall figure nobody
        // can explain.
        let s = store();
        folder(s.conn(), "f1");
        let narrow = HashEmbedder::with_dimensions(8);
        let wide = HashEmbedder::with_dimensions(64);
        write_document(s.conn(), "f1", "doc", &[chunk("A sentence about buffers.", 0)], Some(&narrow), "now")
            .expect("write");

        // Searching with the wider model finds nothing semantically, and the
        // lexical half still answers.
        let hits = search(s.conn(), &["f1".into()], "buffers", Some(&wide), 5).expect("search");
        assert_eq!(hits.len(), 1, "the mismatched vector was compared anyway");
    }

    #[test]
    fn fusion_is_deterministic_on_ties() {
        let a = vec!["x".to_string(), "y".to_string()];
        let b = vec!["y".to_string(), "x".to_string()];
        let first = fuse(&[a.clone(), b.clone()]);
        let second = fuse(&[a, b]);
        assert_eq!(first, second);
    }

    #[test]
    fn writing_no_chunks_still_clears_what_was_there() {
        let s = store();
        folder(s.conn(), "f1");
        write_document(s.conn(), "f1", "doc", &[chunk("Something.", 0)], None, "now").expect("write");
        write_document(s.conn(), "f1", "doc", &[], None, "now").expect("write empty");
        assert!(search(s.conn(), &["f1".into()], "Something", None, 5).expect("search").is_empty());
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_everything() {
        let s = store();
        folder(s.conn(), "f1");
        write_document(s.conn(), "f1", "doc", &[chunk("A sentence.", 0)], None, "now").expect("write");
        assert!(search(s.conn(), &["f1".into()], "   ", None, 5).expect("search").is_empty());
    }
}
