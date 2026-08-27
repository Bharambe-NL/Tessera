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

/// Keys from the environment, for a headless runner and nothing else.
///
/// A CI runner has no OS keychain: on Linux `keyring` wants a Secret Service
/// over D-Bus, which a runner has no session for, so the eval could not reach a
/// key at all and doc 12 phase 11's nightly could not exist.
///
/// What this does not do is loosen the rule. A secret still never lands in a
/// file and never becomes an argument, which is what the rule is protecting
/// against: an argument shows up in `ps`, in a crash dump and in the runner's
/// own echo of the command it ran. An environment variable set for one step
/// from a repository secret is the narrowest thing that works.
///
/// **Not exposed to the shell.** [`crate::build_provider`] and the desktop app
/// take [`OsKeychain`], and this type is reachable only from the eval binary.
/// A user's machine has a keychain, so on a user's machine there is no reason
/// to read a key from anywhere else, and a fallback that quietly worked there
/// would be a second place a key could live.
pub struct EnvKeyStore;

impl EnvKeyStore {
    /// `anthropic-default` becomes `TESSERA_KEY_ANTHROPIC`.
    ///
    /// The provider rather than the whole ref, because a ref carries a label a
    /// person chose (`anthropic-team`, `anthropic-mine`) and a runner has one
    /// account per provider. Everything after the first dash is that label.
    fn var_for(key_ref: &str) -> String {
        let provider = key_ref.split('-').next().unwrap_or(key_ref);
        format!(
            "TESSERA_KEY_{}",
            provider
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                })
                .collect::<String>()
        )
    }
}

impl KeyStore for EnvKeyStore {
    fn get(&self, key_ref: &str) -> Result<String> {
        let var = Self::var_for(key_ref);
        match std::env::var(&var) {
            Ok(secret) if !secret.trim().is_empty() => Ok(secret),
            // Naming the variable and never its value. A message carrying a
            // prefix of the key would put a secret in a CI log, which is the
            // most public place this build has.
            _ => Err(ProviderError::Keychain(format!(
                "no key in `{var}` for `{key_ref}`"
            ))),
        }
    }

    fn set(&self, _key_ref: &str, _secret: &str) -> Result<()> {
        // A process cannot set an environment variable for the job that follows
        // it, and pretending otherwise would report a key stored that is not.
        Err(ProviderError::Keychain(
            "the environment keystore is read only; set the repository secret instead".into(),
        ))
    }

    fn delete(&self, _key_ref: &str) -> Result<()> {
        Err(ProviderError::Keychain(
            "the environment keystore is read only; remove the repository secret instead".into(),
        ))
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
    fn an_environment_key_is_found_by_its_provider_and_never_by_its_label() {
        // A runner has one account per provider, and the label after the first
        // dash is a name a person chose on their own machine.
        assert_eq!(EnvKeyStore::var_for("anthropic-default"), "TESSERA_KEY_ANTHROPIC");
        assert_eq!(EnvKeyStore::var_for("anthropic-team"), "TESSERA_KEY_ANTHROPIC");
        assert_eq!(EnvKeyStore::var_for("moonshot-default"), "TESSERA_KEY_MOONSHOT");
        assert_eq!(EnvKeyStore::var_for("openai"), "TESSERA_KEY_OPENAI");
    }

    #[test]
    fn a_missing_environment_key_names_the_variable_and_not_the_value() {
        // A CI log is the most public place this build has, so the message says
        // which variable to set and nothing about what any key contains.
        let message = EnvKeyStore
            .get("moonshot-default")
            .expect_err("an unset variable is an error")
            .to_string();
        assert!(message.contains("TESSERA_KEY_MOONSHOT"), "{message}");
    }

    #[test]
    fn the_environment_keystore_refuses_to_write() {
        // Nothing a process exports reaches the step after it, and reporting a
        // key stored that is not would send someone looking for it later.
        assert!(EnvKeyStore.set("moonshot-default", "sk-test").is_err());
        assert!(EnvKeyStore.delete("moonshot-default").is_err());
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
