//! The index-backed retrievers: local, regulatory, and boards.
//!
//! Doc 05 sections 8.2, 8.3 and 8.5 describe three retrievers that differ in
//! what they point at and agree on everything else. Local reads watched
//! folders, regulatory reads subscribed corpora, boards reads the profile's own
//! verified cards, and all three answer with the same hybrid query over the
//! same index. One implementation, three configurations.
//!
//! What they do not share is what a passage means afterwards. A regulatory
//! passage is evidence at trust rank 1; a boards passage is a prior card, and
//! doc 15 section 2 is emphatic that it is context and never evidence. That
//! difference is carried in the source class and enforced by the Verifier, not
//! by giving boards its own query path.

use rusqlite::Connection;
use serde_json::json;

use crate::contract::{Coverage, Packet, Passage, Retrieved, Source, cap};
use crate::embed::Embedder;
use crate::index;

/// What distinguishes one index-backed retriever from another.
#[derive(Debug, Clone)]
pub struct IndexedConfig {
    /// The folders this retriever may read. Doc 05 section 8.2: an excluded
    /// folder is never opened, which here means never named.
    pub folder_ids: Vec<String>,
    /// The class every Source it creates carries. Doc 01 section 4.8.
    pub source_class: String,
    /// The freshness class doctrine uses to decide when a citation goes stale.
    pub freshness_class: String,
}

impl IndexedConfig {
    /// Doc 05 section 8.3. Regulatory corpora are subscriptions, so the folder
    /// is the corpus id.
    pub fn regulatory(corpus: impl Into<String>) -> Self {
        Self {
            folder_ids: vec![corpus.into()],
            source_class: "regulatory".into(),
            freshness_class: "regulation".into(),
        }
    }

    /// Doc 05 section 8.2.
    pub fn local(folder_ids: Vec<String>) -> Self {
        Self {
            folder_ids,
            source_class: "local_document".into(),
            freshness_class: "internal_policy".into(),
        }
    }

    /// Doc 05 section 8.5. A prior card enters as a source of its own class so
    /// that the Verifier can single it out.
    pub fn boards() -> Self {
        Self {
            folder_ids: vec!["boards".into()],
            source_class: "own_card".into(),
            freshness_class: "internal_memo".into(),
        }
    }
}

/// Doc 05 section 8.5: "Returns at most three passages".
///
/// The cap is on the retriever rather than on the Planner's request, because
/// the reason for it is what a prior card is rather than how many the caller
/// wants. Memory is meant to remind, not to crowd out the sources.
const BOARDS_MAX_PASSAGES: usize = 3;

/// Run one assignment against the index.
pub fn retrieve(
    conn: &Connection,
    config: &IndexedConfig,
    packet: &Packet,
    embedder: Option<&dyn Embedder>,
) -> rusqlite::Result<Retrieved> {
    let mut folder_ids = config.folder_ids.clone();

    // Doc 05 section 8.2's folder filter, and the only way a caller narrows
    // which watched folder answers. A filter naming a folder this retriever
    // does not have leaves nothing, which is the safe direction: it returns no
    // passages rather than silently reading a folder it was not given.
    if let Some(folder) = &packet.filters.folder {
        folder_ids.retain(|f| f == folder);
    }

    let limit = if config.source_class == "own_card" {
        packet.max_passages.min(BOARDS_MAX_PASSAGES)
    } else {
        packet.max_passages
    };

    let hits = index::search(conn, &folder_ids, &packet.query, embedder, limit)?;

    let mut passages = Vec::with_capacity(hits.len());
    for hit in hits {
        // The document reference is `<path>#<sequence>`; the locator is the
        // document, because a citation points at a place in a document and not
        // at a chunk boundary this build happened to choose.
        let locator = hit
            .document_ref
            .split('#')
            .next()
            .unwrap_or(&hit.document_ref)
            .to_string();
        let issuer = issuer_of(conn, &folder_ids, &locator);

        let trust_rank = packet.doctrine.rank_for(&config.source_class, issuer.as_deref());

        passages.push(Passage {
            passage_id: hit.entry_id.clone(),
            source_id: locator.clone(),
            text: cap(&hit.text),
            location: hit.location.map(|l| json!(l)).unwrap_or(json!({})),
            score: hit.score,
            source: Source {
                class: config.source_class.clone(),
                title: title_of(&locator),
                locator,
                issuer,
                published_at: None,
                trust_rank,
                freshness_class: config.freshness_class.clone(),
                version_ref: packet.filters.version_ref.clone(),
                content_hash: hit.entry_id,
            },
        });
    }

    // Doc 05 section 5. `full` only when the retriever filled the request it
    // was given: anything less is `partial`, and that costs four tenths of the
    // confidence in section 9, which is the point.
    let coverage = if passages.is_empty() {
        Coverage::None
    } else if passages.len() >= limit {
        Coverage::Full
    } else {
        Coverage::Partial
    };

    Ok(Retrieved {
        passages,
        coverage,
        ..Default::default()
    })
}

/// A readable title from a document reference.
fn title_of(locator: &str) -> String {
    locator
        .rsplit('/')
        .next()
        .unwrap_or(locator)
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(locator)
        .replace(['-', '_'], " ")
}

/// The folder's label, which is the closest thing to an issuer an indexed
/// document has. Doc 05 section 8.1 takes an issuer from page metadata; a file
/// on disk has none, so the folder stands in and doctrine matches on it.
fn issuer_of(conn: &Connection, folder_ids: &[String], _locator: &str) -> Option<String> {
    let first = folder_ids.first()?;
    conn.query_row(
        "SELECT label FROM watched_folder WHERE id = ?1",
        rusqlite::params![first],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunking::{Chunk, ChunkLocation};
    use crate::embed::HashEmbedder;

    fn store() -> tessera_store::Store {
        let s = tessera_store::Store::open_in_memory().expect("store");
        s.conn()
            .execute(
                "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                     retriever_config, created_at, updated_at)
                 VALUES ('p', 'deep', 'pack', '{}', '{}', 'now', 'now')",
                [],
            )
            .expect("profile");
        s
    }

    fn folder(conn: &Connection, id: &str, label: &str) {
        conn.execute(
            "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
             VALUES (?1, 'p', ?1, ?2, 'now')",
            rusqlite::params![id, label],
        )
        .expect("folder");
    }

    fn packet(query: &str, max: usize) -> Packet {
        serde_json::from_value(json!({
            "run_id": "r",
            "retriever_id": "local",
            "query": query,
            "max_passages": max,
            "doctrine": {
                "trust_ranks": [
                    { "class": "regulatory", "rank": 2 },
                    { "class": "local_document", "rank": 4 },
                    { "class": "own_card", "rank": 5 }
                ]
            }
        }))
        .expect("packet")
    }

    fn write(conn: &Connection, folder_id: &str, doc: &str, texts: &[&str]) {
        let chunks: Vec<Chunk> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| Chunk::new(*t, ChunkLocation::Whole, i))
            .collect();
        index::write_document(conn, folder_id, doc, &chunks, None, "now").expect("write");
    }

    #[test]
    fn a_passage_carries_the_class_and_rank_its_configuration_says() {
        let s = store();
        folder(s.conn(), "reg", "Central Authority");
        write(
            s.conn(),
            "reg",
            "car3-v1.md",
            &["The capital buffer is 2.5 percent."],
        );

        let out = retrieve(
            s.conn(),
            &IndexedConfig::regulatory("reg"),
            &packet("capital buffer", 12),
            None,
        )
        .expect("retrieve");

        assert_eq!(out.passages.len(), 1);
        assert_eq!(out.passages[0].source.class, "regulatory");
        assert_eq!(
            out.passages[0].source.trust_rank, 2,
            "doctrine did not set the rank"
        );
        assert_eq!(out.passages[0].source.freshness_class, "regulation");
    }

    #[test]
    fn the_locator_is_the_document_and_not_the_chunk() {
        // A citation points at a place in a document. A chunk boundary is an
        // artefact of this build and would make the citation unresolvable the
        // first time the chunker changed.
        let s = store();
        folder(s.conn(), "local", "Risk");
        write(
            s.conn(),
            "local",
            "int-capital-01.md",
            &["The buffer is 2.5 percent."],
        );

        let out = retrieve(
            s.conn(),
            &IndexedConfig::local(vec!["local".into()]),
            &packet("buffer", 5),
            None,
        )
        .expect("retrieve");
        assert_eq!(out.passages[0].source.locator, "int-capital-01.md");
        assert!(!out.passages[0].source.locator.contains('#'));
    }

    #[test]
    fn boards_never_returns_more_than_three_passages() {
        // Doc 05 section 8.5. Memory reminds; it does not crowd out sources.
        let s = store();
        folder(s.conn(), "boards", "Prior cards");
        let texts: Vec<String> = (0..10)
            .map(|i| format!("Card {i} concerned the capital buffer requirement."))
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        write(s.conn(), "boards", "cards", &refs);

        let out = retrieve(
            s.conn(),
            &IndexedConfig::boards(),
            &packet("capital buffer", 12),
            None,
        )
        .expect("retrieve");
        assert!(out.passages.len() <= 3, "boards returned {}", out.passages.len());
        assert_eq!(out.passages[0].source.class, "own_card");
    }

    #[test]
    fn a_folder_filter_can_narrow_but_never_widen() {
        // Naming a folder the retriever was not given must return nothing
        // rather than reach for it. The Sensitive exclusion is exactly this
        // shape: safety comes from the folder never being in the list.
        let s = store();
        folder(s.conn(), "open", "Open");
        folder(s.conn(), "sensitive", "Sensitive");
        write(s.conn(), "open", "public.md", &["Guidance on buffers."]);
        write(
            s.conn(),
            "sensitive",
            "secret.md",
            &["Confidential guidance on buffers."],
        );

        let mut p = packet("buffers guidance", 12);
        p.filters.folder = Some("sensitive".into());
        let out = retrieve(s.conn(), &IndexedConfig::local(vec!["open".into()]), &p, None).expect("retrieve");

        assert!(
            out.passages.is_empty(),
            "a folder the retriever lacks answered anyway"
        );
        assert_eq!(out.coverage, Coverage::None);
    }

    #[test]
    fn coverage_is_full_only_when_the_request_was_filled() {
        let s = store();
        folder(s.conn(), "local", "Risk");
        write(s.conn(), "local", "a.md", &["The buffer is 2.5 percent."]);

        let partial = retrieve(
            s.conn(),
            &IndexedConfig::local(vec!["local".into()]),
            &packet("buffer", 5),
            None,
        )
        .expect("retrieve");
        assert_eq!(
            partial.coverage,
            Coverage::Partial,
            "one of five is not full coverage"
        );

        let full = retrieve(
            s.conn(),
            &IndexedConfig::local(vec!["local".into()]),
            &packet("buffer", 1),
            None,
        )
        .expect("retrieve");
        assert_eq!(full.coverage, Coverage::Full);
    }

    #[test]
    fn nothing_found_is_coverage_none_and_not_an_error() {
        // Doc 05 section 10: a retriever may return nothing. That is a fact
        // about the corpus, not a failure of the retriever.
        let s = store();
        folder(s.conn(), "local", "Risk");
        write(s.conn(), "local", "a.md", &["Something about lunch."]);

        let out = retrieve(
            s.conn(),
            &IndexedConfig::local(vec!["local".into()]),
            &packet("capital adequacy buffers", 12),
            None,
        )
        .expect("retrieve");
        assert!(out.passages.is_empty());
        assert_eq!(out.coverage, Coverage::None);
    }

    #[test]
    fn the_embedder_is_optional_and_the_lexical_half_still_answers() {
        let s = store();
        folder(s.conn(), "local", "Risk");
        write(
            s.conn(),
            "local",
            "a.md",
            &["The leverage ratio floor is 3 percent."],
        );
        let embedder = HashEmbedder::default();

        for embedder in [None, Some(&embedder as &dyn Embedder)] {
            let out = retrieve(
                s.conn(),
                &IndexedConfig::local(vec!["local".into()]),
                &packet("leverage ratio floor", 12),
                embedder,
            )
            .expect("retrieve");
            assert_eq!(out.passages.len(), 1);
        }
    }
}
