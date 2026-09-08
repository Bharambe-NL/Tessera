//! Retrieval: the folders a profile watches and the passages a retriever keeps.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::sources::normalise_locator;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

// ------------------------------------------------------------- retrieval ---

/// A folder this profile has pointed a retriever at. Doc 05 section 8.2.
///
/// The row is what the local retriever reads from, so the set of retrievers a
/// profile actually has is this list plus the pack's enabled ones. Reading it
/// belongs here rather than in the core, because the core would otherwise hold
/// the only SQL outside this module and the columns would drift.
#[derive(Debug, Clone)]
pub struct WatchedFolder {
    pub id: String,
    pub root: String,
    pub label: String,
    /// Doc 10 section 16: a sensitive folder keeps its text on this machine.
    pub sensitive: bool,
    /// `local` or `provider`. Doc 10 section 3 makes provider embeddings opt in
    /// per folder.
    pub embeddings: String,
    pub last_indexed_at: Option<String>,
}

/// Every folder this profile watches, oldest first.
///
/// The boards index lives in the same table because it is an index like any
/// other (doc 05 section 8.5), and it is returned here with the rest; callers
/// that mean folders on disk filter it out by id.
pub fn watched_folders(store: &Store, profile_id: &str) -> Result<Vec<WatchedFolder>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, root, label, sensitive, embeddings, last_indexed_at
           FROM watched_folder
          WHERE profile_id = ?1
          ORDER BY created_at, id",
    )?;
    Ok(stmt
        .query_map(params![profile_id], |r| {
            Ok(WatchedFolder {
                id: r.get(0)?,
                root: r.get(1)?,
                label: r.get(2)?,
                sensitive: r.get::<_, i64>(3)? == 1,
                embeddings: r.get(4)?,
                last_indexed_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Where a retrieval assignment sits. Doc 05 sections 4 and 7.
#[derive(Clone, Copy)]
pub struct RetrievalRef<'a> {
    pub run_id: &'a str,
    pub board_id: &'a str,
    pub card_id: &'a str,
    pub retriever_id: &'a str,
    pub sq_id: Option<&'a str>,
}

/// One passage a retriever found, with the source it came from.
pub struct NewPassage<'a> {
    pub class: &'a str,
    pub title: &'a str,
    pub locator: &'a str,
    pub issuer: Option<&'a str>,
    pub published_at: Option<&'a str>,
    pub freshness_class: &'a str,
    pub trust_rank: i64,
    pub version_ref: Option<&'a str>,
    pub content_hash: &'a str,
    pub text: &'a str,
    pub location: Value,
    /// Doc 01 open question 2, resolved: a folder marked sensitive stores
    /// offsets rather than verbatim text, and its passages are blocked from
    /// export. The text never reaches the row.
    pub text_withheld: bool,
}

/// What persisting a retrieval produced.
#[derive(Debug, Default, Clone)]
pub struct Retained {
    pub passage_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub sources_created: usize,
    pub sources_deduplicated: usize,
    /// Why each passage's source is stale, in passage order, or `None` where it
    /// is not. A source reached again after a re-verification marked it stale is
    /// still stale, and the Verifier's freshness check reads this rather than
    /// assuming that anything just retrieved is current.
    pub stale: Vec<Option<String>>,
}

/// Doc 05 section 7's `retrieval.started.v1`, emitted before anything is
/// fetched so the audit trail shows an assignment that hung as well as one that
/// returned.
pub fn start_retrieval(store: &mut Store, at: RetrievalRef<'_>, query: &str) -> Result<()> {
    store.append(
        NewEvent::new(
            "retrieval.started.v1",
            json!({
                "retriever_id": at.retriever_id,
                "sq_id": at.sq_id,
                "query": query,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}

/// Mark a cited source stale and say why. Doc 05 section 7's `source.stale.v1`,
/// carrying one of the three reasons that section names: `content_changed`,
/// `locator_gone`, `superseded_version`.
///
/// The number of cards citing the source rides on the event, because doc 07
/// section B14 open question 2 makes a batch of stale citations one notice
/// rather than one per card. Returns that count, so a caller re-verifying a
/// whole corpus can report what it touched.
///
/// Marking is idempotent: re-verifying a source already stale for the same
/// reason writes the same row and appends no second event, so a run repeated
/// against an unchanged corpus does not fill the log with duplicates.
pub fn mark_source_stale(store: &mut Store, source_id: &str, reason: &str, run_id: &str) -> Result<usize> {
    let already: Option<String> = store
        .conn()
        .query_row(
            "SELECT stale_reason FROM source WHERE id = ?1 AND stale = 1",
            params![source_id],
            |r| r.get(0),
        )
        .optional()?;
    let affected: i64 = store.conn().query_row(
        "SELECT COUNT(DISTINCT c.card_id)
           FROM citation c JOIN passage p ON p.id = c.passage_id
          WHERE p.source_id = ?1",
        params![source_id],
        |r| r.get(0),
    )?;
    let affected = affected.max(0) as usize;

    if already.as_deref() == Some(reason) {
        return Ok(affected);
    }

    let owned = (source_id.to_string(), reason.to_string(), now_iso8601());
    store.append_with(
        NewEvent::new(
            "source.stale.v1",
            json!({
                "source_id": source_id,
                "reason": reason,
                "affected_cards": affected,
            }),
            Provenance::retriever("reverify", run_id),
        ),
        move |tx| {
            let (sid, reason, now) = owned;
            tx.execute(
                "UPDATE source SET stale = 1, stale_reason = ?2, last_verified_at = ?3
                  WHERE id = ?1",
                params![sid, reason, now],
            )?;
            Ok(())
        },
    )?;
    Ok(affected)
}

/// Persist what a retriever found, and say so. Doc 05 sections 5 and 7.
///
/// Sources are deduplicated on the normalised locator, so a page reached twice
/// through two spellings is one row and one `source.deduplicated.v1`. That is
/// what doc 05 section 12's zero-duplicates gate measures, and doing it here
/// rather than in each connector means a connector cannot forget.
pub fn record_retrieval(
    store: &mut Store,
    profile_id: &str,
    at: RetrievalRef<'_>,
    passages: &[NewPassage<'_>],
    coverage: &str,
    latency_ms: u128,
) -> Result<Retained> {
    let now = now_iso8601();
    let mut retained = Retained::default();

    for passage in passages {
        let dedupe = normalise_locator(passage.locator);

        let existing: Option<(String, Option<String>)> = store
            .conn()
            .query_row(
                "SELECT id, CASE WHEN stale = 1 THEN stale_reason END
                   FROM source WHERE profile_id = ?1 AND dedupe_key = ?2",
                params![profile_id, dedupe],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let (id, created, stale_reason) = match existing {
            Some((id, reason)) => (id, false, reason),
            None => (new_id(), true, None),
        };
        retained.stale.push(stale_reason);

        if created {
            let owned = (
                id.clone(),
                profile_id.to_string(),
                passage.class.to_string(),
                passage.title.to_string(),
                passage.locator.to_string(),
                passage.issuer.map(str::to_string),
                passage.published_at.map(str::to_string),
                passage.freshness_class.to_string(),
                passage.version_ref.map(str::to_string),
                passage.content_hash.to_string(),
                dedupe.clone(),
                now.clone(),
            );
            let rank = passage.trust_rank;
            store.append_with(
                NewEvent::new(
                    "source.created.v1",
                    json!({
                        "source_id": id,
                        "class": passage.class,
                        "locator": passage.locator
                    }),
                    Provenance::retriever(at.retriever_id, at.run_id),
                )
                .on_board(at.board_id)
                .on_card(at.card_id),
                move |tx| {
                    let (
                        sid,
                        profile,
                        class,
                        title,
                        locator,
                        issuer,
                        published,
                        freshness,
                        version,
                        hash,
                        dedupe,
                        now,
                    ) = owned;
                    tx.execute(
                        "INSERT INTO source (id, profile_id, class, title, locator, site_or_issuer,
                             published_at, retrieved_at, last_verified_at, content_hash,
                             freshness_class, trust_rank, dedupe_key, version_ref, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, ?11, ?12, ?13, ?8)",
                        params![
                            sid, profile, class, title, locator, issuer, published, now, hash, freshness,
                            rank, dedupe, version
                        ],
                    )?;
                    Ok(())
                },
            )?;
            retained.sources_created += 1;
        } else {
            store.append(
                NewEvent::new(
                    "source.deduplicated.v1",
                    json!({ "source_id": id, "locator": passage.locator }),
                    Provenance::retriever(at.retriever_id, at.run_id),
                )
                .on_board(at.board_id)
                .on_card(at.card_id),
            )?;
            retained.sources_deduplicated += 1;
        }

        // A withheld passage keeps its location and loses its text, which is
        // what makes a citation into a sensitive folder checkable by the person
        // who owns the folder and useless to anyone a bundle reaches.
        let text: Option<String> = (!passage.text_withheld).then(|| passage.text.to_string());
        let passage_id = new_id();
        store.conn().execute(
            "INSERT INTO passage (id, source_id, text, location, retrieved_in_run, retrieved_by,
                 text_withheld, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                passage_id,
                id,
                text,
                passage.location.to_string(),
                at.run_id,
                at.retriever_id,
                i64::from(passage.text_withheld),
                now
            ],
        )?;

        retained.passage_ids.push(passage_id);
        if !retained.source_ids.contains(&id) {
            retained.source_ids.push(id);
        }
    }

    store.append(
        NewEvent::new(
            "retrieval.completed.v1",
            json!({
                "retriever_id": at.retriever_id,
                "sq_id": at.sq_id,
                "passage_ids": retained.passage_ids,
                "source_ids": retained.source_ids,
                "coverage": coverage,
                "fetches": passages.len(),
                "latency_ms": latency_ms as u64,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;

    Ok(retained)
}

/// Doc 05 section 7's `hook.denied.v1`.
///
/// A denial names the category and never the item. Doc 05 section 10: the card
/// caveat names the exclusion category without naming the excluded thing,
/// because the whole point of an exclusion is that its contents do not leave.
pub fn record_hook_denial(
    store: &mut Store,
    at: RetrievalRef<'_>,
    hook_id: &str,
    category: &str,
) -> Result<()> {
    store.append(
        NewEvent::new(
            "hook.denied.v1",
            json!({
                "retriever_id": at.retriever_id,
                "hook_id": hook_id,
                "target": category,
            }),
            Provenance::retriever(at.retriever_id, at.run_id),
        )
        .on_board(at.board_id)
        .on_card(at.card_id),
    )?;
    Ok(())
}
