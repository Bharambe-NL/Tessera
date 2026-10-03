//! The UI and the core over HTTP, so Tessera runs in a browser.
//!
//! Doc 10 section 2 keeps the RPC boundary clean so a web client can talk to the
//! identical protocol over a socket, and this is that socket. It serves the
//! built UI and one `POST /rpc` endpoint, dispatched through the same router
//! the desktop shell registers, against a real profile with the keys the
//! keychain holds. Nothing here knows a verb.
//!
//! Every connection gets its own thread, and an idle one gives up after a short
//! wait, so a browser that opens a socket it never uses cannot hold the server
//! for everybody else. The core itself is one `&mut`, so requests still take
//! their turn at the lock: a card run in progress makes the next request wait,
//! as it does in the desktop shell.
//!
//! It listens on loopback only and carries no login. Two checks stand in for
//! one: the `Host` header must name this machine, which defeats a page that
//! rebinds its own domain to 127.0.0.1, and `/rpc` takes only a JSON body from
//! this origin, which a foreign page cannot send without a preflight this
//! server never answers.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Core, Router};

/// How long a connection may sit without sending a byte before it is dropped.
const IDLE: Duration = Duration::from_secs(15);

/// The largest request body accepted. A pasted image arrives base64 encoded, so
/// this sits well above the core's own image ceiling.
const MAX_BODY: usize = 64 * 1024 * 1024;

const TEXT: &str = "text/plain; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

/// The profile folder the desktop app uses, so the browser and the app read
/// and write the same boards. `TESSERA_PROFILE` overrides it.
pub fn default_profile_root() -> PathBuf {
    if let Ok(explicit) = std::env::var("TESSERA_PROFILE") {
        return PathBuf::from(explicit);
    }
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    } else {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".local/share"))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("Tessera")
        .join("default")
}

/// A web front for one core.
pub struct WebServer {
    ui: PathBuf,
    core: Arc<Mutex<Core>>,
    router: Arc<Router<Core>>,
}

impl WebServer {
    pub fn new(ui: impl Into<PathBuf>, core: Core, router: Router<Core>) -> Self {
        Self {
            ui: ui.into(),
            core: Arc::new(Mutex::new(core)),
            router: Arc::new(router),
        }
    }

    /// Serve until the listener stops. Each connection runs on its own thread.
    pub fn serve(self, listener: TcpListener) {
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let me = Arc::new(self);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let me = Arc::clone(&me);
                    std::thread::spawn(move || me.handle(stream, port));
                }
                Err(e) => eprintln!("connection failed: {e}"),
            }
        }
    }

    fn handle(&self, mut stream: TcpStream, port: u16) {
        let _ = stream.set_read_timeout(Some(IDLE));
        let Ok(clone) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(clone);

        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() || request_line.trim().is_empty() {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("GET").to_string();
        let path = parts.next().unwrap_or("/").to_string();

        let mut head = Head::default();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            head.read(&line);
        }

        if !head.host_is_local(port) {
            respond(
                &mut stream,
                "421 Misdirected Request",
                TEXT,
                b"This server answers only to this machine.",
            );
            return;
        }

        if path == "/rpc" {
            if method != "POST" {
                respond(&mut stream, "405 Method Not Allowed", TEXT, b"POST only.");
                return;
            }
            if !head.json || !head.origin_is_local(port) {
                respond(&mut stream, "403 Forbidden", TEXT, b"Not from this page.");
                return;
            }
            if head.length > MAX_BODY {
                respond(&mut stream, "413 Payload Too Large", TEXT, b"Too large.");
                return;
            }
            let mut body = vec![0u8; head.length];
            if reader.read_exact(&mut body).is_err() {
                respond(&mut stream, "400 Bad Request", TEXT, b"Short body.");
                return;
            }
            let raw = String::from_utf8_lossy(&body);
            // A handler that panicked leaves the lock poisoned. The core's state
            // lives in the database, which a panic does not half write, so the
            // next request carries on rather than every later one failing.
            let reply = {
                let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
                self.router
                    .dispatch_str(&mut core, &raw)
                    .unwrap_or_else(|| "{}".to_string())
            };
            respond(&mut stream, "200 OK", JSON, reply.as_bytes());
            return;
        }

        if method != "GET" && method != "HEAD" {
            respond(&mut stream, "405 Method Not Allowed", TEXT, b"GET only.");
            return;
        }
        let file = resolve(&self.ui, &path);
        match std::fs::read(&file) {
            Ok(body) => respond(&mut stream, "200 OK", content_type(&file), &body),
            Err(_) => respond(&mut stream, "404 Not Found", TEXT, b"Not here."),
        }
    }
}

/// The request headers this server reads.
#[derive(Default)]
struct Head {
    length: usize,
    host: Option<String>,
    origin: Option<String>,
    json: bool,
}

impl Head {
    fn read(&mut self, line: &str) {
        let Some((name, value)) = line.split_once(':') else {
            return;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => self.length = value.parse().unwrap_or(0),
            "host" => self.host = Some(value.to_ascii_lowercase()),
            "origin" => self.origin = Some(value.to_ascii_lowercase()),
            "content-type" => {
                self.json = value.to_ascii_lowercase().starts_with("application/json");
            }
            _ => {}
        }
    }

    fn local_hosts(port: u16) -> [String; 2] {
        [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
    }

    fn host_is_local(&self, port: u16) -> bool {
        self.host
            .as_ref()
            .is_some_and(|h| Self::local_hosts(port).contains(h))
    }

    /// A browser sends `Origin` on every POST. Its absence means a client that
    /// is not a web page, which the loopback bind already confines to this
    /// machine.
    fn origin_is_local(&self, port: u16) -> bool {
        match &self.origin {
            None => true,
            Some(o) => Self::local_hosts(port)
                .iter()
                .any(|h| *o == format!("http://{h}")),
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => JSON,
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

/// Resolve a request path inside the UI directory. Every `..` segment and every
/// drive prefix is dropped rather than resolved, so a request cannot leave it.
fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let mut out = root.to_path_buf();
    for segment in path.split(['/', '\\']) {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(':') {
            continue;
        }
        out.push(segment);
    }
    if out == root || out.is_dir() {
        out.push("index.html");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_providers::MockProvider;

    fn server() -> u16 {
        let core = Core::in_memory(Arc::new(MockProvider::new())).expect("core");
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let web = WebServer::new(std::env::temp_dir(), core, crate::build_router());
        std::thread::spawn(move || web.serve(listener));
        port
    }

    fn rpc(port: u16, host: &str, origin: Option<&str>, kind: &str) -> String {
        let body = r#"{"jsonrpc":"2.0","method":"board.list","params":{},"id":1}"#;
        let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        let raw = format!(
            "POST /rpc HTTP/1.1\r\nHost: {host}\r\n{origin}Content-Type: {kind}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.write_all(raw.as_bytes()).expect("write");
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn a_silent_connection_does_not_hold_the_server() {
        let port = server();
        // The browser preconnect that wedged the one connection test server.
        let _idle = TcpStream::connect(("127.0.0.1", port)).expect("idle");
        let local = format!("127.0.0.1:{port}");
        let reply = rpc(port, &local, Some(&format!("http://{local}")), "application/json");
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert!(reply.contains("\"boards\""), "{reply}");
    }

    #[test]
    fn a_foreign_page_cannot_reach_the_core() {
        let port = server();
        let local = format!("127.0.0.1:{port}");
        let foreign = rpc(port, &local, Some("http://evil.example"), "application/json");
        assert!(foreign.starts_with("HTTP/1.1 403"), "{foreign}");
        let simple = rpc(port, &local, None, "text/plain");
        assert!(simple.starts_with("HTTP/1.1 403"), "{simple}");
        let rebound = rpc(port, &format!("evil.example:{port}"), None, "application/json");
        assert!(rebound.starts_with("HTTP/1.1 421"), "{rebound}");
    }

    #[test]
    fn a_path_cannot_leave_the_ui_folder() {
        let root = Path::new("ui");
        assert_eq!(resolve(root, "/../../secret.txt"), root.join("secret.txt"));
        assert_eq!(resolve(root, "/..\\..\\x.js?v=1"), root.join("x.js"));
        assert_eq!(resolve(root, "/C:/Windows/x"), root.join("Windows").join("x"));
    }
}
