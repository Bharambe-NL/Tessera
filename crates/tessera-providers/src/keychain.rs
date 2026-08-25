//! Secrets live in the OS keychain and nowhere else.
//!
//! Doc 01 section 4.16: "Model keys live in the OS keychain (macOS Keychain,
//! Windows Credential Manager). The Profile stores a `ModelKey` list of
//! `{key_ref, provider, label, active}` where `key_ref` names the keychain
//! entry. The database never holds a secret."
//!
//! Doc 12 operating principle 7 and its definition of done say the same twice
//! over: no secret in any file except the keychain.

use crate::error::{ProviderError, Result};

const SERVICE: &str = "com.tessera.app";

/// Reads and writes keys by reference. A `key_ref` is the only thing that ever
/// crosses into the database, an event, a bundle, or a log line.
pub trait KeyStore: Send + Sync {
    fn get(&self, key_ref: &str) -> Result<String>;
    fn set(&self, key_ref: &str, secret: &str) -> Result<()>;
    fn delete(&self, key_ref: &str) -> Result<()>;
    fn has(&self, key_ref: &str) -> bool {
        self.get(key_ref).is_ok()
    }
}

/// The real one. macOS Keychain and Windows Credential Manager through
/// `keyring`, unlocked by the OS user (doc 10 section 8).
pub struct OsKeychain;

impl KeyStore for OsKeychain {
    fn get(&self, key_ref: &str) -> Result<String> {
        keyring::Entry::new(SERVICE, key_ref)
            .and_then(|e| e.get_password())
            .map_err(|e| ProviderError::Keychain(e.to_string()))
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, key_ref)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| ProviderError::Keychain(e.to_string()))
    }

    fn delete(&self, key_ref: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, key_ref)
            .and_then(|e| e.delete_credential())
            .map_err(|e| ProviderError::Keychain(e.to_string()))
    }
}

/// In memory, for tests and for the eval harness.
///
/// Deliberately not exposed to the app: a keystore that survives only as long as
/// the process cannot be mistaken for the real one, and it never writes a secret
/// to disk.
#[derive(Default)]
pub struct MemoryKeyStore {
    entries: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(key_ref: &str, secret: &str) -> Self {
        let s = Self::new();
        let _ = s.set(key_ref, secret);
        s
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, key_ref: &str) -> Result<String> {
        self.entries
            .lock()
            .ok()
            .and_then(|e| e.get(key_ref).cloned())
            .ok_or_else(|| ProviderError::Keychain(format!("no entry for `{key_ref}`")))
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<()> {
        self.entries
            .lock()
            .map(|mut e| {
                e.insert(key_ref.to_string(), secret.to_string());
            })
            .map_err(|_| ProviderError::Keychain("poisoned".into()))
    }

    fn delete(&self, key_ref: &str) -> Result<()> {
        self.entries
            .lock()
            .map(|mut e| {
                e.remove(key_ref);
            })
            .map_err(|_| ProviderError::Keychain("poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_entry_is_an_error_not_an_empty_string() {
        // An empty key would reach the provider and come back as an auth error,
        // which points the user at the wrong problem.
        let s = MemoryKeyStore::new();
        assert!(s.get("anthropic-team").is_err());
        assert!(!s.has("anthropic-team"));
    }

    #[test]
    fn round_trips_and_deletes() {
        let s = MemoryKeyStore::with("anthropic-team", "sk-test");
        assert_eq!(s.get("anthropic-team").expect("get"), "sk-test");
        assert!(s.has("anthropic-team"));
        s.delete("anthropic-team").expect("delete");
        assert!(!s.has("anthropic-team"));
    }
}
