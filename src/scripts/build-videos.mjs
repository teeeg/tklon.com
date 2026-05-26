// Author-only, build-time-adjacent video pipeline. UNLIKE scripts/build-images.mjs,
// this is NOT run by middleman/CI: video sources are too large to commit, so encoding
// happens locally (via `make video`) and the encoded MP4 is uploaded to S3 out-of-band.
//
// Per source in src/videos/ (gitignored) it:
//   1. encodes an H.264 MP4 (faststart, capped at 1080p in BOTH dimensions),
//   2. names it by content hash (src/videos/.out/<name>-<hash>.mp4) so two clips that
//      happen to share a basename can never overwrite each other on S3,
//   3. extracts a poster frame into src/images/<name>-poster.jpg (a committed source
//      image — it then flows through build-images.mjs for responsive avif/webp),
//   4. probes intrinsic dimensions + duration,
//   5. uploads the MP4 to s3://tklon.com-assets/media/ with an immutable cache header,
//   6. records it in src/data/videos.json (COMMITTED, unlike images.json) so the
//      _video.erb partial can render in CI without the source or ffmpeg present.
//
// `--prune` reconciles /media/ against the committed manifest and removes orphans left
// behind by re-encodes / renames / abandoned drafts.
//
// Usage:
//   node scripts/build-videos.mjs                  # encode every source in src/videos/
//   node scripts/build-videos.mjs surf.mov         # encode just one
//   node scripts/build-videos.mjs --prune [--yes]  # delete unreferenced /media/ objects

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdir, mkdir, readFile, writeFile, rename } from "node:fs/promises";
import { createInterface } from "node:readline/promises";
import path from "node:path";

import { SRC_ROOT, VIDEO_DIR, MANIFEST, INPUT_RE, nameOf, hashSource } from "./_videos-shared.mjs";

const OUT_DIR = path.join(VIDEO_DIR, ".out");
const POSTER_DIR = path.join(SRC_ROOT, "images");

const BUCKET = "tklon.com-assets";
const MEDIA_PREFIX = "media/";

// Bound BOTH dimensions so a tall/4K vertical clip can't balloon past 1080p in its long
// edge; decrease keeps aspect ratio; divisible_by=2 keeps dims even for yuv420p/H.264.
const SCALE = "scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2";

// Run a child process, throwing a useful error on failure. capture=true returns stdout
// (for ffprobe / aws queries); otherwise child output streams to the terminal.
function run(cmd, args, { capture = false } = {}) {
  const res = spawnSync(cmd, args, {
    stdio: capture ? ["ignore", "pipe", "pipe"] : ["ignore", "inherit", "inherit"],
    encoding: "utf8",
  });
  if (res.error) {
    if (res.error.code === "ENOENT") throw new Error(`\`${cmd}\` not found — is it installed and on PATH?`);
    throw new Error(`Failed to run ${cmd}: ${res.error.message}`);
  }
  if (res.status !== 0) throw new Error(`${cmd} exited ${res.status}${res.stderr ? `\n${res.stderr}` : ""}`);
  return res.stdout;
}

async function readManifest() {
  try {
    return JSON.parse(await readFile(MANIFEST, "utf8"));
  } catch {
    return {};
  }
}

async function processOne(file) {
  const name = nameOf(file);
  const input = path.join(VIDEO_DIR, file);

  await mkdir(OUT_DIR, { recursive: true });
  await mkdir(POSTER_DIR, { recursive: true });
  await mkdir(path.dirname(MANIFEST), { recursive: true });

  // 0. Short-circuit if the source's content matches what the manifest was last
  // encoded from. ffmpeg/aws are both expensive enough that running `make video`
  // on a clean tree shouldn't re-do work; this also makes the pipeline safe to
  // invoke defensively (e.g. from a pre-push hook).
  const sourceHash = await hashSource(input);
  const existing = (await readManifest())[name];
  if (existing?.sourceHash === sourceHash) {
    console.log(`✓ ${name}  (unchanged — skipping encode/upload)`);
    return;
  }

  // 1. Encode to a temp name; rename once we know the content hash.
  const tmp = path.join(OUT_DIR, `${name}.tmp.mp4`);
  console.log(`→ encoding ${file}`);
  run("ffmpeg", [
    "-y", "-i", input,
    "-vf", SCALE,
    "-c:v", "libx264", "-crf", "23", "-preset", "slow", "-pix_fmt", "yuv420p",
    "-movflags", "+faststart",
    "-c:a", "aac", "-b:a", "128k",
    tmp,
  ]);

  // 2. Content-hash → final filename.
  const hash = createHash("sha256").update(await readFile(tmp)).digest("hex").slice(0, 8);
  const finalName = `${name}-${hash}.mp4`;
  const finalPath = path.join(OUT_DIR, finalName);
  await rename(tmp, finalPath);

  // 3. Poster: small seek + thumbnail filter dodges fade-in / black opening frames.
  const poster = `${name}-poster`;
  run("ffmpeg", [
    "-y", "-ss", "1", "-i", input,
    "-vf", "thumbnail", "-frames:v", "1", "-q:v", "2",
    path.join(POSTER_DIR, `${poster}.jpg`),
  ]);

  // 4. Probe the ENCODED file so dims match what's actually served.
  const probe = JSON.parse(run("ffprobe", [
    "-v", "error", "-select_streams", "v:0",
    "-show_entries", "stream=width,height", "-show_entries", "format=duration",
    "-of", "json", finalPath,
  ], { capture: true }));
  const { width, height } = probe.streams[0];
  const duration = Math.round(Number(probe.format.duration) * 10) / 10;

  // 5. Upload (immutable: the filename is content-addressed).
  console.log(`→ uploading ${finalName}`);
  run("aws", [
    "s3", "cp", finalPath, `s3://${BUCKET}/${MEDIA_PREFIX}${finalName}`,
    "--content-type", "video/mp4",
    "--cache-control", "public, max-age=31536000, immutable",
  ]);

  // 6. Record in the committed manifest only after a successful upload. Read-modify-write
  // a single key and write back sorted to keep diffs/merge conflicts minimal.
  const manifest = await readManifest();
  manifest[name] = { src: finalName, width, height, duration, poster, sourceHash };
  const sorted = {};
  for (const k of Object.keys(manifest).sort()) sorted[k] = manifest[k];
  await writeFile(MANIFEST, JSON.stringify(sorted, null, 2) + "\n");

  console.log(`✓ ${name}  (${width}×${height}, ${duration}s) → ${finalName}\n`);
}

async function encode(positional) {
  let files;
  if (positional) {
    files = [path.basename(positional)]; // sources are flat in src/videos/
  } else {
    try {
      files = (await readdir(VIDEO_DIR)).filter((f) => INPUT_RE.test(f));
    } catch {
      console.error(`No video source directory at ${VIDEO_DIR}`);
      process.exit(1);
    }
    if (files.length === 0) {
      console.log(`No videos in ${path.relative(SRC_ROOT, VIDEO_DIR)} — nothing to do.`);
      return;
    }
  }
  for (const file of files) await processOne(file);
  console.log(`Wrote ${files.length} video(s) to manifest ${path.relative(SRC_ROOT, MANIFEST)}`);
}

async function prune(yes) {
  const manifest = await readManifest();
  const referenced = new Set(Object.values(manifest).map((v) => v.src));

  const out = run("aws", [
    "s3api", "list-objects-v2", "--bucket", BUCKET, "--prefix", MEDIA_PREFIX,
    "--query", "Contents[].Key", "--output", "text",
  ], { capture: true }).trim();

  const keys = out && out !== "None" ? out.split(/\s+/) : [];
  const orphans = keys.map((k) => k.slice(MEDIA_PREFIX.length)).filter((f) => f && !referenced.has(f));

  if (orphans.length === 0) {
    console.log("No orphaned media objects — /media/ matches the manifest.");
    return;
  }

  console.log("Orphaned /media/ objects (not referenced by data/videos.json):");
  for (const o of orphans) console.log(`  ${o}`);

  if (!yes) {
    if (!process.stdin.isTTY) {
      console.log(`\nRe-run with --yes to delete the ${orphans.length} object(s) above.`);
      return;
    }
    const rl = createInterface({ input: process.stdin, output: process.stdout });
    const ans = (await rl.question(`\nDelete ${orphans.length} object(s)? [y/N] `)).trim().toLowerCase();
    rl.close();
    if (ans !== "y") {
      console.log("Aborted — nothing deleted.");
      return;
    }
  }

  for (const o of orphans) run("aws", ["s3", "rm", `s3://${BUCKET}/${MEDIA_PREFIX}${o}`]);
  console.log(`Deleted ${orphans.length} object(s).`);
}

async function main() {
  const args = process.argv.slice(2);
  const positional = args.find((a) => !a.startsWith("--"));
  if (args.includes("--prune")) {
    await prune(args.includes("--yes"));
  } else {
    await encode(positional);
  }
}

main().catch((err) => {
  console.error(err.message ?? err);
  process.exit(1);
});
