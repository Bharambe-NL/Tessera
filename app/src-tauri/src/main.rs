// Doc 11 section 5: the shell is a single window holding the canvas.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::sync::Mutex;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Set to a card count to run the doc 12 phase 0 acceptance gate instead of the
/// normal board, print the result to stdout, and exit with the gate's verdict.
const GATE_ENV: &str = "TESSERA_GATE";

/// File the gate result is written to, so a windowed release build can report.
const GATE_OUT_ENV: &str = "TESSERA_GATE_OUT";

#[derive(Default)]
struct GateOutcome(Mutex<Option<bool>>);

/// Called once by the webview when the gate finishes. Printing here rather than
/// in the webview means the numbers land in the terminal and in CI, which is
/// where a regression has to be visible.
#[tauri::command]
fn report_gate(app: tauri::AppHandle, text: String, passed: bool, raw: serde_json::Value) {
    println!("{text}");
    // A release build on Windows has no console, so the file is the real channel.
    write_gate_file(&text, &raw);
    if let Some(state) = app.try_state::<GateOutcome>() {
        if let Ok(mut slot) = state.0.lock() {
            *slot = Some(passed);
        }
    }
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
    write_gate_file(&format!("gate could not run: {message}"), &serde_json::Value::Null);
    app.exit(2);
}

fn main() -> ExitCode {
    let gate = std::env::var(GATE_ENV).ok().filter(|v| !v.is_empty());

    let result = tauri::Builder::default()
        .manage(GateOutcome::default())
        .invoke_handler(tauri::generate_handler![report_gate, report_gate_error])
        .setup(move |app| {
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
            if gate.is_some() {
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
