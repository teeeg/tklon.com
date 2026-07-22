# generator

The static-site generator for tklon.com. Builds a single binary named `tklon`,
which reads content from `../site/` and writes to `build/` at the repo root.

## Run

Cargo works from anywhere in the repo — the binary finds the root by walking up
for `site/source/`.

```sh
cargo run -- build
cargo run -- serve                # http://localhost:4567, rebuilds on save
cargo run -- serve --port 8080

cargo install --path generator    # then call `tklon` directly
```

`serve` builds once, then polls `site/source/` and `site/data/` every 500ms.

## What it generates

- Posts from `site/source/posts/{year}/{slug}.md`, reverse-chronological, 5 per index page
- Permalinks `posts/{slug}-{ddmmyyyy}/`, a tag filter page, a 404
- Atom `feed.xml`, `h-entry`/`h-card` microformats, `rel="me"`
- Kramdown-compatible heading ids, smart punctuation, inline-SVG pass-through
- SCSS compiled in-binary via `grass`; content-hashed CSS, JS and font

## Authoring format

Chosen to render in any Markdown preview:

- Images — `![alt](name "caption")`, where `name` is a manifest key. Expands to a
  responsive `<picture>` inside a zoomable `<figure>`. `![](name "caption")` gives `alt=""`.
- Video — `{{< video name="clip-name" caption="…" >}}`, caption optional
- Embed — `{{< embed url="https://…" title="…" >}}`

## Media commands

Both are authoring-time and idempotent — unchanged inputs are a no-op.

`tklon images` reads masters from `site/images/`, writes AVIF (q50) and WebP
(q80) variants at widths 750/1500 into `site/source/images/`, and rewrites
`site/data/images.json`. It never upscales, strips EXIF, and bakes in
orientation. Variant filenames embed `sha256(master)[:8]`, so editing a master
changes the URL. It closes by syncing the gitignored masters in `site/images/`
and `site/videos/` to `s3://…/masters/`, so git carries only the small variants.

`tklon video [src]` encodes with ffmpeg (libx264 crf 23, preset slow, 1080p cap,
faststart, aac 128k), uploads the MP4 to `s3://…/media/`, extracts a poster
frame, then runs `images` for the poster variants and the master sync. The clip
is recorded in `site/data/videos.json`. `tklon video --check` fails if a source
in `site/videos/` is missing from the manifest or differs from what was last
encoded. Requires `ffmpeg`, `ffprobe` and `aws` on PATH.
