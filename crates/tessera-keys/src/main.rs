//! Key management, standing in for the Profile's Models page until M9.
//!
//! Doc 01 section 4.16: "Model keys live in the OS keychain (macOS Keychain,
//! Windows Credential Manager). The Profile stores a `ModelKey` list of
//! `{key_ref, provider, label, active}` where `key_ref` names the keychain
//! entry. The database never holds a secret." Doc 12 operating principle 7 and
//! its definition of done say it twice more: no secret in any file except the
//! keychain.
//!
//! So this tool never takes a key as an argument. A key on a command line lands
//! in the shell history, in the process table, and in any terminal scrollback
//! that gets pasted somewhere later. It is read from the terminal without echo,
//! written straight to the keychain, and never printed back.
//!
//! `check` calls the provider's models endpoint. That does two jobs: it proves
//! the key works before a 400 question sweep spends anything, and it lists the
//! model ids the provider actually has, which is better than guessing at names
//! that move.

use std::io::{IsTerminal, Read};

use clap::{Parser, Subcommand};
use tessera_providers::{
    AnthropicProvider, KeyStore, ModelProvider, OpenAiCompatProvider, OsKeychain, endpoint_for,
};

/// The key_refs the shipped model policy names.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "anthropic-default",
        "anthropic",
        "Anthropic, for the small, medium and frontier aliases",
    ),
    (
        "moonshot-default",
        "moonshot",
        "Moonshot Kimi, for the bulk eval sweep",
    ),
    ("openai-default", "openai", "OpenAI"),
    ("search-default", "search", "The web search provider, from M6"),
];

#[derive(Parser)]
#[command(
    name = "tessera-keys",
    about = "Put a provider key in the OS keychain. The key is never written to a file."
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read a key from the terminal and store it. Nothing is echoed.
    Set {
        /// The keychain entry to write, for example `moonshot-default`.
        key_ref: String,
    },
    /// Show which keys exist. Values are never printed.
    List,
    /// Confirm a key works and list the models the provider offers.
    Check {
        key_ref: String,
        /// Override the provider guessed from the key_ref.
        #[arg(long)]
        provider: Option<String>,
    },
    /// Remove a key.
    Remove { key_ref: String },
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let store = OsKeychain;

    match args.command {
        Command::Set { key_ref } => set(&store, &key_ref),
        Command::List => list(&store),
        Command::Check { key_ref, provider } => check(&store, &key_ref, provider.as_deref()),
        Command::Remove { key_ref } => remove(&store, &key_ref),
    }
}

fn set(store: &OsKeychain, key_ref: &str) -> std::process::ExitCode {
    let known = KNOWN.iter().find(|(r, _, _)| *r == key_ref);
    match known {
        Some((_, provider, description)) => {
            println!("Storing a key for {provider}: {description}");
        }
        None => {
            println!("Storing `{key_ref}`. It is not one the shipped policy names,");
            println!("so remember to point an alias at it in the model policy.");
        }
    }

    let secret = match read_secret() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read the key: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let secret = secret.trim();
    if secret.is_empty() {
        eprintln!("Nothing was entered, so nothing was stored.");
        return std::process::ExitCode::from(2);
    }

    match store.set(key_ref, secret) {
        Ok(()) => {
            // The length is safe to print and is the one thing that catches a
            // truncated paste, which is otherwise indistinguishable from a bad
            // key when the provider rejects it.
            println!("Stored {} characters under `{key_ref}`.", secret.chars().count());
            println!("Check it with: tessera-keys check {key_ref}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("The keychain refused the write: {e}");
            std::process::ExitCode::from(2)
        }
    }
}

/// Read without echo from a terminal, or from a pipe when there is no terminal.
///
/// The pipe path is what makes this usable from CI, where the key comes from a
/// secret store rather than from a person.
fn read_secret() -> std::io::Result<String> {
    if std::io::stdin().is_terminal() {
        print!("Paste the key and press enter. It will not be shown: ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let secret = rpassword::read_password()?;
        println!();
        return Ok(secret);
    }
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

fn list(store: &OsKeychain) -> std::process::ExitCode {
    println!("{:<22} {:<12} status", "key_ref", "provider");
    println!("{}", "-".repeat(60));
    for (key_ref, provider, _) in KNOWN {
        let status = if store.has(key_ref) { "stored" } else { "not set" };
        println!("{key_ref:<22} {provider:<12} {status}");
    }
    println!();
    println!("Values are never shown. Set one with: tessera-keys set <key_ref>");
    std::process::ExitCode::SUCCESS
}

fn remove(store: &OsKeychain, key_ref: &str) -> std::process::ExitCode {
    match store.delete(key_ref) {
        Ok(()) => {
            println!("Removed `{key_ref}`.");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Could not remove `{key_ref}`: {e}");
            std::process::ExitCode::from(2)
        }
    }
}

fn check(store: &OsKeychain, key_ref: &str, provider_override: Option<&str>) -> std::process::ExitCode {
    let Ok(secret) = store.get(key_ref) else {
        eprintln!("No key stored under `{key_ref}`. Set one with: tessera-keys set {key_ref}");
        return std::process::ExitCode::from(2);
    };

    let provider = provider_override
        .map(str::to_string)
        .or_else(|| {
            KNOWN
                .iter()
                .find(|(r, _, _)| *r == key_ref)
                .map(|(_, p, _)| (*p).to_string())
        })
        .unwrap_or_else(|| "anthropic".to_string());

    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Could not start a runtime: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    println!("Checking `{key_ref}` against {provider}.");

    if provider == "anthropic" {
        // Anthropic has no models list on this adapter, so the check is the
        // cheapest possible real call: a one token completion.
        let Ok(client) = AnthropicProvider::new(secret) else {
            eprintln!("Could not build the Anthropic client.");
            return std::process::ExitCode::from(2);
        };
        // Deliberately the model the `small` alias resolves to, because that is
        // the one the Router calls on every card and the one whose parameter
        // support differs from the rest.
        let request = tessera_providers::CompletionRequest::new("claude-haiku-4-5", "healthcheck")
            .user("Reply with the single word: ok")
            .max_tokens(16);

        return match runtime.block_on(client.complete(&request)) {
            Ok(c) => {
                println!("The key works. {} answered in {} ms.", c.model, c.latency_ms);
                println!("Aliases: small claude-haiku-4-5, medium claude-sonnet-5, frontier claude-opus-5.");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("The provider refused: {e}");
                std::process::ExitCode::from(1)
            }
        };
    }

    let Some(endpoint) = endpoint_for(&provider) else {
        eprintln!("No adapter for `{provider}`.");
        return std::process::ExitCode::from(2);
    };
    let Ok(client) = OpenAiCompatProvider::new(endpoint, secret) else {
        eprintln!("Could not build the {provider} client.");
        return std::process::ExitCode::from(2);
    };

    match runtime.block_on(client.list_models()) {
        Ok(models) if models.is_empty() => {
            println!("The key works, but the provider listed no models.");
            std::process::ExitCode::SUCCESS
        }
        Ok(models) => {
            println!("The key works. {} offers {} models:", provider, models.len());
            for m in &models {
                println!("  {m}");
            }
            println!();
            println!("Point an alias at one of these in the model policy.");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("The provider refused: {e}");
            std::process::ExitCode::from(1)
        }
    }
}
