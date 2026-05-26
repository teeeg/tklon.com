// Constants + helpers that build-videos.mjs and check-videos.mjs MUST agree on.
// Keep this file minimal — only things where drift between the two would silently
// break the pre-push guarantee (e.g. check-videos failing to match a source the
// encoder picked up, or hashing source bytes differently than the encoder did).

import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SRC_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const VIDEO_DIR = path.join(SRC_ROOT, "videos");
export const MANIFEST = path.join(SRC_ROOT, "data", "videos.json");

// What counts as a video source, and how its filename maps to a manifest key.
export const INPUT_RE = /\.(mov|mp4|m4v|mkv|webm)$/i;
export const nameOf = (file) => file.replace(INPUT_RE, "");

// 16-hex-char sha256 prefix of the source bytes. Stored on each manifest entry
// as `sourceHash` so re-runs can skip unchanged sources and pre-push can verify
// the committed encode actually corresponds to the on-disk source.
export async function hashSource(filePath) {
  return createHash("sha256").update(await readFile(filePath)).digest("hex").slice(0, 16);
}
