//! The vault retriever. Doc 16 section 3.3.
//!
//! "The local retriever indexes `vault/` like any folder, so pages are
//! retrievable with no new retriever. Their source class is `page`."
//!
//! No new retriever *implementation*: this is [`crate::indexed`] over another
//! folder, exactly as the boards retriever is. What it needs of its own is the
//! id, because one [`IndexedConfig`] carries one source class, and a folder set
//! holding both a person's documents and their pages could not label them
//! apart. Doc 16 section 3.3 is emphatic that it must: a page is context and
//! the passages it carries are the evidence.
//!
//! And the text indexed is the page's body from the row rather than the file on
//! disk, because the row is what the app has just agreed with the file, and
//! because a wikilink is punctuation to a lexical index. `[[Liquidity risk]]`
//! indexed verbatim matches a query for brackets and misses the sentence.

use rusqlite::{Connection, params};

use crate::embed::Embedder;
use crate::index;
use crate::parse::markdown;

/// The folder id the vault's pages are indexed under.
pub const VAULT_FOLDER: &str = "vault";

/// Make sure the vault folder exists before anything is written into it.
///
/// A row in `watched_folder` like the boards index has, because that table is
/// what the index joins to for a passage's issuer, and because the Retrievers
/// page reads the same list.
pub fn ensure_folder(conn: &Connection, profile_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO watched_folder (id, profile_id, root, label, created_at)
         VALUES (?1, ?2, 'vault', 'My pages', ?3)
         ON CONFLICT DO NOTHING",
        params![VAULT_FOLDER, profile_id, tessera_store::now_iso8601()],
    )?;
    Ok(())
}

/// Index one page's body.
///
/// `strip` is the caller's: the core knows what a wikilink is and this crate
/// does not, so the text arrives already readable.
pub fn index_page(
    conn: &Connection,
    profile_id: &str,
    page_id: &str,
    title: &str,
    text: &str,
    embedder: Option<&dyn Embedder>,
) -> rusqlite::Result<usize> {
    ensure_folder(conn, profile_id)?;

    // The title leads, because a page called "Liquidity risk" whose body never
    // repeats the phrase is still the page somebody asking about liquidity risk
    // wants, and a lexical index only knows what it was given.
    let source = format!("# {title}\n\n{text}");
    let chunks = markdown::parse(&source);
    if chunks.is_empty() {
        forget_page(conn, page_id)?;
        return Ok(0);
    }

    index::write_document(
        conn,
        VAULT_FOLDER,
        page_id,
        &chunks,
        embedder,
        &tessera_store::now_iso8601(),
    )
}

/// Take a page out of the index. Doc 16 section 2.1: deleting a page must not
/// reach into an answer that cited it, and it does not, because a citation
/// names a Passage that carries its own text. What it must do is stop the page
/// being retrieved again.
pub fn forget_page(conn: &Connection, page_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM index_entry WHERE folder_id = ?1 AND document_chunk_ref LIKE ?2",
        params![VAULT_FOLDER, format!("{page_id}#%")],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexed::IndexedConfig;

    #[test]
    fn a_page_is_indexed_under_its_own_class() {
        let config = IndexedConfig::pages(vec![VAULT_FOLDER.to_string()]);
        assert_eq!(config.source_class, "page");
        assert_eq!(
            config.folder_ids,
            vec![VAULT_FOLDER.to_string()],
            "the vault is one folder, and it is not the person's documents"
        );
    }
}
