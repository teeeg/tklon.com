//! tklon — the tklon.com static site generator.
//!
//! Subcommands:
//!   tklon build [--out DIR]            render the site (default: <root>/build)
//!   tklon serve [--out DIR] [--port N] build, serve, and rebuild on change
//!   tklon images                       (re)generate image variants + images.json
//!   tklon video [src]                  encode + upload video(s)
//!   tklon video --check                verify sources match videos.json
//!   tklon video --prune [--yes]        delete orphaned /media/ objects on S3
//!   tklon ingest <post.md> <media…>    fill a post's `@` slots with attachments

mod build;
mod config;
mod ingest;
mod markdown;
mod media;
mod model;
mod serve;

use model::Res;
use std::env;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Res<()> {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("build");
    let root = find_root()?;
    let out = flag(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("build"));

    match cmd {
        "build" => build::build(&root, &out),
        "serve" => {
            let port = flag(&args, "--port")
                .and_then(|p| p.parse().ok())
                .unwrap_or(4567);
            serve::serve(&root, &out, port)
        }
        "images" => media::images(&root),
        "video" => {
            let has = |flag: &str| args.iter().any(|a| a == flag);
            if has("--check") {
                media::check_videos(&root)
            } else if has("--prune") {
                media::prune(&root, has("--yes"))
            } else {
                let positional = args.get(2).filter(|a| !a.starts_with("--")).cloned();
                media::video(&root, positional)
            }
        }
        "ingest" => {
            let post = args.get(2).ok_or("usage: tklon ingest <post.md> <media…>")?;
            let media: Vec<PathBuf> = args[3..].iter().map(PathBuf::from).collect();
            ingest::ingest(&root, Path::new(post), &media)
        }
        other => Err(format!("unknown command '{other}' (build|serve|images|video)").into()),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Walk up from the cwd until we find the repo root (the dir containing site/source/).
fn find_root() -> Res<PathBuf> {
    let mut dir = env::current_dir()?;
    loop {
        if dir.join("site/source").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("could not find repo root (looked for site/source/ upward from cwd)".into());
        }
    }
}
