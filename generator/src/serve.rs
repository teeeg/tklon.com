//! `tklon serve` — build once, serve `build/` over HTTP, and rebuild on change.

use crate::build::build;
use crate::model::Res;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};
use tiny_http::{Header, Response, Server};

pub fn serve(root: &Path, out: &Path, port: u16) -> Res<()> {
    build(root, out)?;

    // background poll-and-rebuild so edits show up on refresh
    let watch_root = root.to_path_buf();
    let watch_out = out.to_path_buf();
    thread::spawn(move || {
        let source = watch_root.join("site/source");
        let data = watch_root.join("site/data");
        let templates = watch_root.join("site/templates");
        let config = watch_root.join("site/config.yml");
        let dirs = [source, data, templates, config];
        let mut last = fingerprint(&dirs);
        loop {
            thread::sleep(Duration::from_millis(500));
            let now = fingerprint(&dirs);
            if now != last {
                last = now;
                match build(&watch_root, &watch_out) {
                    Ok(()) => {}
                    Err(e) => eprintln!("rebuild failed: {e}"),
                }
            }
        }
    });

    let server = Server::http(("0.0.0.0", port)).map_err(|e| e.to_string())?;
    println!("serving {} at http://localhost:{port}", out.display());
    for request in server.incoming_requests() {
        let response = handle(out, request.url());
        let _ = request.respond(response);
    }
    Ok(())
}

fn handle(out: &Path, url: &str) -> Response<Cursor<Vec<u8>>> {
    let path_part = url.split('?').next().unwrap_or("/");
    let rel = path_part.trim_start_matches('/');
    // rel == "" resolves to `out` itself (a dir); trailing-slash urls also land
    // on a dir — so a single is_dir check covers directory-index resolution.
    let mut fp = out.join(rel);
    if fp.is_dir() {
        fp = fp.join("index.html");
    }

    let (status, bytes, ctype) = match fs::read(&fp) {
        Ok(b) => (200u16, b, content_type(&fp)),
        Err(_) => {
            let body = fs::read(out.join("404.html")).unwrap_or_else(|_| b"404 Not Found".to_vec());
            (404, body, "text/html; charset=utf-8")
        }
    };
    let header = Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap();
    Response::from_data(bytes)
        .with_status_code(status)
        .with_header(header)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("xml") => "application/atom+xml; charset=utf-8",
        Some("json") => "application/json",
        Some("woff2") => "font/woff2",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// Sum of file modification times under `paths` — changes when any file is saved.
fn fingerprint(paths: &[PathBuf]) -> u128 {
    let mut acc: u128 = 0;
    for p in paths {
        walk_mtime(p, &mut acc);
    }
    acc
}

fn walk_mtime(path: &Path, acc: &mut u128) {
    if path.is_file() {
        if let Ok(m) = fs::metadata(path).and_then(|md| md.modified()) {
            if let Ok(dur) = m.duration_since(SystemTime::UNIX_EPOCH) {
                *acc = acc.wrapping_add(dur.as_millis());
            }
        }
        return;
    }
    let Ok(rd) = fs::read_dir(path) else { return };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            walk_mtime(&entry.path(), acc);
        } else if let Ok(m) = entry.metadata().and_then(|md| md.modified()) {
            if let Ok(dur) = m.duration_since(SystemTime::UNIX_EPOCH) {
                *acc = acc.wrapping_add(dur.as_millis());
            }
        }
    }
}
