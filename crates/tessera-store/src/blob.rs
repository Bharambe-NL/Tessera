//! Content addressed blob store.
//!
//! Doc 10 section 3: a directory beside the database holding images, sanitised
//! svgs and prompt texts. Doc 01 section 4.6: images are stored once by hash, so
//! a forked board never duplicates bytes. Doc 01 section 6.2: prompts are stored
//! by hash with the full text here, so the audit trail can reproduce any call
//! while the database stays small.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Result, StoreError};

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn hash(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        hex::encode(h.finalize())
    }

    /// Two levels of fan out on the hash prefix, so a profile with a hundred
    /// thousand blobs does not put them all in one directory.
    fn path_for(&self, digest: &str) -> PathBuf {
        self.root.join(&digest[0..2]).join(&digest[2..4]).join(digest)
    }

    /// Write bytes and return their digest. Writing the same bytes twice is a
    /// no-op, which is what makes a bundle import cheap.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let digest = Self::hash(bytes);
        let path = self.path_for(&digest);
        if path.exists() {
            return Ok(digest);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write to a sibling and rename, so a crash mid write cannot leave a
        // truncated blob under a digest that claims to describe it.
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(digest)
    }

    pub fn put_str(&self, s: &str) -> Result<String> {
        self.put(s.as_bytes())
    }

    /// Read bytes back, verifying they still hash to the digest that names them.
    /// Doc 01 section 7 requires this on bundle import; doing it on every read
    /// costs a hash over data already in the page cache and catches bit rot.
    pub fn get(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.path_for(digest);
        if !path.exists() {
            return Err(StoreError::BlobMissing(digest.to_string()));
        }
        let bytes = std::fs::read(&path)?;
        let actual = Self::hash(&bytes);
        if actual != digest {
            return Err(StoreError::BlobCorrupt {
                expected: digest.to_string(),
                actual,
            });
        }
        Ok(bytes)
    }

    pub fn exists(&self, digest: &str) -> bool {
        self.path_for(digest).exists()
    }

    /// Remove a blob. Only called by the garbage collector for blobs nothing
    /// references any more (doc 10 section 4).
    pub fn remove(&self, digest: &str) -> Result<()> {
        let path = self.path_for(digest);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tessera-blob-{}", ulid::Ulid::generate()));
        p
    }

    #[test]
    fn round_trips_and_deduplicates() {
        let dir = temp();
        let store = BlobStore::open(&dir).expect("open");

        let a = store.put(b"the same bytes").expect("put");
        let b = store.put(b"the same bytes").expect("put again");
        assert_eq!(a, b, "identical bytes must land under one digest");

        assert_eq!(store.get(&a).expect("get"), b"the same bytes");
        assert!(store.exists(&a));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_corruption() {
        let dir = temp();
        let store = BlobStore::open(&dir).expect("open");
        let digest = store.put(b"original").expect("put");

        let path = store.path_for(&digest);
        std::fs::write(&path, b"tampered").expect("tamper");

        match store.get(&digest) {
            Err(StoreError::BlobCorrupt { .. }) => {}
            other => panic!("expected BlobCorrupt, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_blob_is_an_error_not_an_empty_read() {
        let dir = temp();
        let store = BlobStore::open(&dir).expect("open");
        match store.get(&"0".repeat(64)) {
            Err(StoreError::BlobMissing(_)) => {}
            other => panic!("expected BlobMissing, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
