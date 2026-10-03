//! Tessera in a browser: the real core, the real profile, the keys the
//! keychain holds, and the built UI on one local address.
//!
//! ```text
//! pnpm --dir app/ui build
//! cargo run --release -p tessera-core --bin tessera-web
//! ```
//!
//! Then open http://127.0.0.1:8740. The profile is the desktop app's own
//! folder, so boards made in one show in the other; run one of them at a time.
//!
//! Options: `--ui <dir>` (default `app/ui/dist`), `--port <n>` (default 8740),
//! `--profile <dir>` (default the desktop profile, or `TESSERA_PROFILE`).

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use tessera_core::{Core, WebServer, build_router, default_profile_root};
use tessera_providers::OsKeychain;

/// The keychain entry the shipped model policy names, as in the desktop shell.
const KEY_REF: &str = "anthropic-default";

struct Args {
    ui: PathBuf,
    port: u16,
    profile: PathBuf,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        ui: PathBuf::from("app/ui/dist"),
        port: 8740,
        profile: default_profile_root(),
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut it = raw.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--ui" => out.ui = PathBuf::from(value),
            "--port" => out.port = value.parse().map_err(|_| "--port takes a number".to_string())?,
            "--profile" => out.profile = PathBuf::from(value),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if !args.ui.join("index.html").is_file() {
        eprintln!(
            "No index.html under {}. Run `pnpm --dir app/ui build` first.",
            args.ui.display()
        );
        return ExitCode::from(2);
    }

    // Without a key the core still opens: the first run screen asks for one,
    // and saving it there rebuilds the provider without a restart.
    let core = match Core::open_live(&args.profile, Box::new(OsKeychain), KEY_REF) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("Could not open the profile at {}: {e}", args.profile.display());
            return ExitCode::from(1);
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", args.port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Could not listen on 127.0.0.1:{}: {e}", args.port);
            return ExitCode::from(1);
        }
    };
    println!(
        "Tessera is serving {} at http://127.0.0.1:{}",
        args.profile.display(),
        args.port
    );

    WebServer::new(&args.ui, core, build_router()).serve(listener);
    ExitCode::SUCCESS
}
