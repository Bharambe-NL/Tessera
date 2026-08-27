#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The web retriever over a real socket. Doc 05 section 8.1 and phase 13e.
//!
//! The unit tests in `web.rs` inject a fetcher over a map, which covers the
//! extraction and the ranking and says nothing about HTTP. This one serves the
//! shape `gen serve` serves, over loopback, and drives `HttpFetcher` through
//! it: a directory listing of links, pages behind them, and one response that
//! is not a page.
//!
//! Loopback is the whole point. The server binds 127.0.0.1 on a port the OS
//! picks, the seed is that address, and discovery never leaves a seed's host,
//! so this test cannot reach the internet even if something in it is wrong.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use serde_json::json;
use tessera_retrievers::contract::Packet;
use tessera_retrievers::web::{self, HttpFetcher, WebConfig};

/// One page of the synthetic web, in the shape the generator writes.
fn page(title: &str, issuer: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>{title}</title>\
         <meta name=\"issuer\" content=\"{issuer}\">\
         <meta name=\"published\" content=\"2025-05-22\">\
         </head><body><h1>{title}</h1><p>{body}</p></body></html>"
    )
}

/// The directory listing `SimpleHTTPRequestHandler` produces, near enough.
const LISTING: &str = "<!DOCTYPE HTML><html><head><title>Directory listing</title></head>\
     <body><h1>Directory listing for /</h1><hr><ul>\
     <li><a href=\"buffers.html\">buffers.html</a></li>\
     <li><a href=\"outsourcing.html\">outsourcing.html</a></li>\
     <li><a href=\"report.pdf\">report.pdf</a></li>\
     </ul><hr></body></html>";

fn body_for(path: &str) -> Option<(&'static str, String)> {
    match path {
        "/" => Some(("text/html", LISTING.to_string())),
        "/buffers.html" => Some((
            "text/html",
            page(
                "Capital buffers explained",
                "ledgerline.invalid",
                "The capital conservation buffer is 2.5 per cent of risk weighted assets.",
            ),
        )),
        "/outsourcing.html" => Some((
            "text/html",
            page(
                "Outsourcing notification",
                "ledgerline.invalid",
                "The notification period before an outsourcing starts comes to 117 days.",
            ),
        )),
        // Not a page. Doc 05 section 8.1 reads html; anything else is refused
        // before it is read rather than parsed into a passage of nothing.
        "/report.pdf" => Some(("application/pdf", "%PDF-1.4 not really".to_string())),
        _ => None,
    }
}

fn serve(stream: TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // Drain the headers so the client sees a clean exchange.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.trim().is_empty() => break,
            Ok(_) => continue,
            Err(_) => return,
        }
    }

    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut stream = stream;
    match body_for(&path) {
        Some((content_type, body)) => {
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
        }
        None => {
            let _ = write!(
                stream,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    }
    let _ = stream.flush();
}

/// Start the server and return its base URL and a handle that stops it.
fn loopback() -> (String, mpsc::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("addr").port();
    let (stop, stopped) = mpsc::channel::<()>();

    thread::spawn(move || {
        for stream in listener.incoming() {
            if stopped.try_recv().is_ok() {
                return;
            }
            match stream {
                Ok(s) => serve(s),
                Err(_) => return,
            }
        }
    });

    (format!("http://127.0.0.1:{port}/"), stop)
}

fn packet(query: &str) -> Packet {
    serde_json::from_value(json!({
        "run_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "retriever_id": "web",
        "query": query,
        "max_passages": 4,
        "doctrine": { "trust_ranks": [{ "class": "web", "rank": 6 }] }
    }))
    .expect("packet")
}

#[test]
fn the_web_retriever_walks_a_loopback_listing_and_cites_the_page_that_answers() {
    let (base, stop) = loopback();
    let config = WebConfig::new(vec![base.clone()]);
    let out = web::retrieve(
        &HttpFetcher::new(),
        &config,
        &packet("capital conservation buffer"),
    );

    assert!(!out.passages.is_empty(), "nothing came back over the socket");
    let first = &out.passages[0];
    assert!(
        first.text.contains("2.5 per cent"),
        "the page that answers did not rank first: {:?}",
        first.text
    );
    assert_eq!(first.source.class, "web");
    assert!(first.source.locator.starts_with(&base));
    assert_eq!(
        first.source.issuer.as_deref(),
        Some("ledgerline.invalid"),
        "doc 05 section 8.1's post hook reads the issuer from the page"
    );
    assert_eq!(first.source.published_at.as_deref(), Some("2025-05-22"));
    assert_eq!(first.source.content_hash.len(), 64, "a sha256 of the body");

    // Doc 05 section 8.1 reads pages. The pdf in the listing was refused before
    // it was read, so it is a fetch error rather than a passage of nothing.
    assert_eq!(out.fetch_errors, 1, "the non page was not refused");
    assert!(
        out.passages.iter().all(|p| !p.source.locator.ends_with(".pdf")),
        "a pdf became a web passage"
    );

    let _ = stop.send(());
}

#[test]
fn two_runs_over_the_same_server_produce_the_same_rows() {
    // The property the whole staleness story rests on, asserted over HTTP
    // rather than over a map: a sweep that moved between runs would make every
    // number downstream of it unreadable.
    let (base, stop) = loopback();
    let config = WebConfig::new(vec![base]);
    let fetcher = HttpFetcher::new();
    let ids = |r: &tessera_retrievers::Retrieved| {
        r.passages
            .iter()
            .map(|p| {
                (
                    p.passage_id.clone(),
                    p.source.content_hash.clone(),
                    p.text.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    let first = web::retrieve(&fetcher, &config, &packet("capital buffer"));
    let second = web::retrieve(&fetcher, &config, &packet("capital buffer"));
    assert!(!ids(&first).is_empty());
    assert_eq!(ids(&first), ids(&second));

    let _ = stop.send(());
}

#[test]
fn a_denied_domain_is_never_opened_over_a_real_socket() {
    // Doc 05 section 15's pre hook, with a server that would answer if asked.
    // The assertion that matters is the absence: nothing was fetched, so
    // nothing can have been cited.
    let (base, stop) = loopback();
    let config = WebConfig::new(vec![base]);
    let mut p = packet("capital buffer");
    p.doctrine.denied_domains = vec!["127.0.0.1".into()];

    let out = web::retrieve(&HttpFetcher::new(), &config, &p);
    assert!(out.passages.is_empty(), "a denied domain answered");
    assert_eq!(out.fetch_errors, 0, "a denied domain was opened and then failed");
    assert!(
        out.caveats.iter().any(|c| c.contains("denied")),
        "the denial was not reported: {:?}",
        out.caveats
    );

    let _ = stop.send(());
}
