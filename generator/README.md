# generator

The Rust static-site generator for tklon.com (§3 of the toolchain-rewrite spec).
This crate lives in `generator/` and builds a single binary named `tklon` — no
Ruby/Node/Sharp/webpack. It reads the site content under `../site/` in place.

## Run

A workspace manifest at the repo root means cargo works from anywhere in the repo
(the binary itself finds the root by looking for `site/source/` upward):

```sh
cargo run -- build                # → build/
cargo run -- serve                # http://localhost:4567, rebuilds on save
cargo run -- serve --port 8080

# or install the command once, then call it directly:
cargo install --path generator
tklon build
tklon serve
```

Output goes to `build/` at the repo root. `serve` builds once, then polls
`site/source/` + `site/data/` every 500ms and rebuilds on change.

## What it does

- Markdown posts (`posts/{year}/{slug}.md`) with front matter, reverse-chronological, 5-per-page index
- Permalinks `posts/{slug}-{ddmmyyyy}/`, tag filter page, 404
- Media authoring format (human-friendly, renders in any Markdown preview):
  - Images: native Markdown `![alt](name "caption")`, where `name` is a bare
    manifest key. Expands to the responsive `<picture>` + zoom `<figure>`.
    Caption-only (`![](name "caption")`) yields `alt=""`.
  - Video: `{{< video name="clip-name" caption="…" >}}` (caption optional)
  - Embed: `{{< embed url="https://…" title="…" >}}`
- Kramdown-compatible heading ids, smart punctuation, inline-SVG pass-through
- SCSS compiled in-binary via `grass`; content-hashed CSS/JS/font
- `tags.js` ported from TypeScript to plain ES2020 (no build step)
- Atom `feed.xml`, `h-entry`/`h-card` microformats + `rel="me"`, site footer

## Media authoring

Both are authoring-time and idempotent (unchanged inputs are a no-op):

- `tklon images` — pure Rust (`image` + `ravif` + `webp`). Reads masters from
  `site/images/`, writes AVIF (q50) / WebP (q80) variants at widths 750/1500
  (never upscaled, EXIF stripped, orientation baked in) into
  `site/source/images/`, and rewrites `site/data/images.json`. Variant filenames
  embed `sha256(master)[:8]`, so a master edit changes the URL automatically.
  Masters are gitignored and synced to `s3://…/masters/` (like video), so git
  carries only the small variants + manifest — never large binaries.
- `tklon video [src]` — shells out to `ffmpeg` (libx264 crf 23, preset slow,
  1080p cap, faststart, aac 128k) + `aws`. Encodes the master, uploads the MP4 to
  `s3://tklon.com-assets/media/`, backs up the untouched master to `…/masters/`,
  extracts a poster frame (then runs `images` for its variants), and records the
  clip in `site/data/videos.json`. `tklon video --check` is the pre-push guard:
  it fails if any source in `site/videos/` is missing from the manifest or differs
  from what was last encoded. Requires `ffmpeg`/`ffprobe`/`aws` on PATH.

## Deferred

- Byte-exact HTML minification / the Middleman-vs-Rust parity harness (spec §7).
- `tklon video --prune` (GC of orphaned `/media/` objects).
