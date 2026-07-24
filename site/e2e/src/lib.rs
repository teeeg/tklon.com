//! Test harness for the link checks in `tests/`.
//!
//! Walks an already-built site on disk, following internal links breadth-first
//! from `/` and collecting external ones. Nothing here knows how the site was
//! produced — it only needs a directory of files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Absolute self-links (canonical, og:url) carry this origin. They are checked
/// against the local build, not the live site — a freshly written post's
/// canonical URL exists here before it exists in production.
const ORIGIN: &str = "https://tklon.com";

/// Where the built site lives: `TKLON_BUILD_DIR`, else `<repo>/build`.
/// CARGO_MANIFEST_DIR is `<repo>/site/e2e`, so the repo root is two up.
pub fn build_dir() -> PathBuf {
    let dir = match std::env::var_os("TKLON_BUILD_DIR") {
        Some(p) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("manifest dir has no grandparent")
            .join("build"),
    };
    assert!(
        dir.join("index.html").is_file(),
        "no built site at {} — run `cargo run -- build` first \
         (or point TKLON_BUILD_DIR at a build elsewhere)",
        dir.display()
    );
    dir
}

/// The file the production server would serve for `/path`, if any.
fn resolve(dir: &Path, path: &str) -> Option<PathBuf> {
    let mut fp = dir.join(path.trim_start_matches('/'));
    if fp.is_dir() {
        fp.push("index.html");
    }
    fp.is_file().then_some(fp)
}

/// What a crawl from `/` turned up.
pub struct Crawl {
    /// Internal paths with no file behind them in the build.
    pub broken: Vec<String>,
    /// Off-site URLs encountered, deduplicated and sorted.
    pub external: Vec<String>,
}

/// Follows every internal link breadth-first from `/`.
pub fn crawl() -> Crawl {
    let dir = build_dir();
    let mut to_visit = vec!["/".to_string()];
    let mut visited = HashSet::new();
    let mut broken = Vec::new();
    let mut external = HashSet::new();

    while let Some(path) = to_visit.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        // /media/* is uploaded to S3 out of band by `tklon video` and is never
        // written into build/, so its absence is expected rather than a defect.
        if path.starts_with("/media/") {
            continue;
        }
        let Some(file) = resolve(&dir, &path) else {
            broken.push(path);
            continue;
        };
        if file.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let body = std::fs::read_to_string(&file).unwrap_or_default();
        for href in extract_hrefs(&body) {
            let href = match href.strip_prefix(ORIGIN) {
                Some("") => "/".to_string(),
                Some(rest) if rest.starts_with('/') => rest.to_string(),
                _ => href,
            };
            if href.starts_with("http://") || href.starts_with("https://") {
                external.insert(href);
            } else if href.starts_with('/') {
                let clean = href.split(['?', '#']).next().unwrap_or(&href).to_string();
                if !visited.contains(&clean) {
                    to_visit.push(clean);
                }
            }
            // Anything else — #fragment, mailto:, a relative path — is not a
            // link this site emits; the generator writes root-relative paths.
        }
    }
    let mut external: Vec<String> = external.into_iter().collect();
    external.sort();
    Crawl { broken, external }
}

fn extract_hrefs(html: &str) -> Vec<String> {
    let mut out = attr_values(html, "href");
    out.extend(attr_values(html, "src"));
    out
}

/// Every value of `attr` (e.g. "href") across the document, in order.
/// minify-html drops quotes around attribute values that don't need them, so
/// the built output is mostly `href=/feed.xml`, not `href="/feed.xml"`.
/// Handles bare, double-quoted and single-quoted forms.
pub fn attr_values(html: &str, attr: &str) -> Vec<String> {
    let pat = format!("{attr}=");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(&pat) {
        rest = &rest[i + pat.len()..];
        let end = match rest.as_bytes().first() {
            Some(q @ (b'"' | b'\'')) => {
                let quote = *q as char;
                rest = &rest[1..];
                match rest.find(quote) {
                    Some(e) => e,
                    None => break,
                }
            }
            _ => rest
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(rest.len()),
        };
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// ureq turns 4xx/5xx into `Err` by default — exactly the codes link checking
/// needs to read — so requests go through an agent with that turned off, and
/// with a timeout so one hanging host can't stall the run.
pub fn http_client() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    )
}
