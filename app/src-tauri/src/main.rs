// Doc 11 section 5: the shell is a single window holding the canvas.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tessera_core::{Core, Router};
use tessera_providers::{AnthropicProvider, KeyStore, MockProvider, ModelProvider, OsKeychain};

/// Set to a card count to run the doc 12 phase 0 acceptance gate instead of the
/// normal board, print the result to stdout, and exit with the gate's verdict.
const GATE_ENV: &str = "TESSERA_GATE";

/// File the gate result is written to, so a windowed release build can report.
const GATE_OUT_ENV: &str = "TESSERA_GATE_OUT";

/// The keychain entry the shipped model policy names. Doc 01 section 4.16: the
/// database holds this reference, never the secret.
const KEY_REF: &str = "anthropic-default";

struct Shell {
    core: Arc<Mutex<Core>>,
    router: Arc<Router<Core>>,
}

/// The whole shell surface is this one command.
///
/// Doc 10 section 2 keeps the RPC boundary clean so the web client can later
/// talk to the identical protocol over a socket. A Tauri command that reached
/// into the store directly would be a shortcut that client cannot take, so
/// there is exactly one command and it forwards.
#[tauri::command]
async fn rpc(state: tauri::State<'_, Shell>, request: String) -> Result<String, String> {
    let core = Arc::clone(&state.core);
    let router = Arc::clone(&state.router);

    // A card run blocks on the core's own runtime, so it must not sit on the
    // webview's async executor.
    tokio::task::spawn_blocking(move || {
        let mut core = core.lock().map_err(|_| "The core is unavailable.".to_string())?;
        Ok(router
            .dispatch_str(&mut core, &request)
            .unwrap_or_else(|| String::from("{}")))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The profile folder. Doc 10 section 15: one folder is the unit of backup,
/// restore, and "open profile from folder".
fn profile_root() -> std::path::PathBuf {
    if let Ok(explicit) = std::env::var("TESSERA_PROFILE") {
        return std::path::PathBuf::from(explicit);
    }
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(std::path::PathBuf::from)
    } else {
        std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join(".local/share"))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("Tessera")
        .join("default")
}

/// Without a key the app still opens; it just cannot answer a card. The failure
/// then arrives as `policy_unresolvable` with a message that says where to fix
/// it, which is better than refusing to start.
fn build_provider() -> Arc<dyn ModelProvider> {
    match OsKeychain.get(KEY_REF) {
        Ok(key) => match AnthropicProvider::new(key) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                eprintln!("could not build the Anthropic provider: {e}");
                Arc::new(MockProvider::new())
            }
        },
        Err(_) => Arc::new(MockProvider::new()),
    }
}

/// Called once by the webview when the gate finishes. Printing here rather than
/// in the webview means the numbers land in the terminal and in CI, which is
/// where a regression has to be visible.
#[tauri::command]
fn report_gate(app: tauri::AppHandle, text: String, passed: bool, raw: serde_json::Value) {
    println!("{text}");
    // A release build on Windows has no console, so the file is the real channel.
    write_gate_file(&text, &raw);
    app.exit(if passed { 0 } else { 1 });
}

/// Write the gate result where the runner and CI can read it.
fn write_gate_file(text: &str, raw: &serde_json::Value) {
    let Ok(path) = std::env::var(GATE_OUT_ENV) else {
        return;
    };
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::json!({ "text": text, "result": raw });
    match serde_json::to_string_pretty(&body) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("could not write {path}: {e}");
            }
        }
        Err(e) => eprintln!("could not serialise the gate result: {e}"),
    }
}

/// Called by the webview when the gate could not run at all, for instance
/// because the window never became visible.
#[tauri::command]
fn report_gate_error(app: tauri::AppHandle, message: String) {
    eprintln!("gate could not run: {message}");
    write_gate_file(
        &format!("gate could not run: {message}"),
        &serde_json::Value::Null,
    );
    app.exit(2);
}

fn main() -> ExitCode {
    let gate = std::env::var(GATE_ENV).ok().filter(|v| !v.is_empty());
    let gating = gate.is_some();

    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![rpc, report_gate, report_gate_error])
        .setup(move |app| {
            // The gate measures the canvas, not the core, so it opens no
            // profile: a fixture board needs no store.
            if !gating {
                let root = profile_root();
                match Core::open(&root, Box::new(OsKeychain), build_provider(), KEY_REF) {
                    Ok(core) => {
                        app.manage(Shell {
                            core: Arc::new(Mutex::new(core)),
                            router: Arc::new(tessera_core::build_router()),
                        });
                    }
                    // A profile that will not open is worth saying out loud, and
                    // the window still comes up so the user can read why.
                    Err(e) => eprintln!("could not open the profile at {}: {e}", root.display()),
                }
            }

            let url = match &gate {
                Some(n) => format!("index.html?gate={n}"),
                None => "index.html".to_string(),
            };
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(url.into()))
                .title("Tessera")
                .inner_size(1440.0, 900.0)
                .min_inner_size(880.0, 600.0)
                .visible(true)
                .build()?;

            // The gate measures animation frames, which a background or minimised
            // window does not schedule. Force the window forward before the
            // webview starts driving the pan.
            if gating {
                let _ = win.set_focus();
                let _ = win.unminimize();
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tessera failed to start: {e}");
            ExitCode::from(2)
        }
    }
}
