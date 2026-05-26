// Pre-push guard: every source in src/videos/ must be reflected in
// src/data/videos.json, and its content must match what was last encoded.
// Catches the "added/modified a video but forgot `make video`" footgun —
// pushing past it would either break the build (missing manifest entry) or,
// worse, silently serve the previous encode (manifest still points at the old
// content hash).
//
// No ffmpeg, no network: we hash the source bytes and compare against the
// sourceHash recorded by build-videos.mjs. Legacy entries (encoded before
// sourceHash existed) fall back to an mtime check until next re-encode.

import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";

import { VIDEO_DIR, MANIFEST, INPUT_RE, nameOf, hashSource } from "./_videos-shared.mjs";

let sources;
try {
  sources = (await readdir(VIDEO_DIR)).filter((f) => INPUT_RE.test(f));
} catch {
  // No src/videos/ on this machine (e.g. fresh clone with no drafts) — nothing
  // to check. The manifest is authoritative for CI; absence of sources is fine.
  process.exit(0);
}

if (sources.length === 0) process.exit(0);

const manifest = JSON.parse(await readFile(MANIFEST, "utf8"));
const manifestMtime = (await stat(MANIFEST)).mtimeMs;

const problems = [];
for (const file of sources) {
  const name = nameOf(file);
  const entry = manifest[name];
  if (!entry) {
    problems.push(`  ${file} → no entry in data/videos.json`);
    continue;
  }
  const sourcePath = path.join(VIDEO_DIR, file);
  if (entry.sourceHash) {
    // Exact check — hash matches iff the source bytes are what we last encoded.
    if ((await hashSource(sourcePath)) !== entry.sourceHash) {
      problems.push(`  ${file} → content differs from last encode (sourceHash mismatch)`);
    }
  } else {
    // Legacy entry: no recorded hash, fall back to mtime. Strictly weaker
    // (mtime resets on checkout/cp -p) but still catches local edits.
    const srcMtime = (await stat(sourcePath)).mtimeMs;
    if (srcMtime > manifestMtime) {
      problems.push(`  ${file} → modified since last encode (source newer than manifest)`);
    }
  }
}

if (problems.length > 0) {
  console.error("✗ video sources are out of sync with data/videos.json:\n");
  for (const p of problems) console.error(p);
  console.error("\nRun `make video` to re-encode and upload, then commit the manifest.");
  process.exit(1);
}
