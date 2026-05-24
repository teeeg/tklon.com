// Build-time responsive image generation.
//
// Reads source originals from src/images/, emits AVIF/WebP variants at a fixed
// set of widths into src/source/images/ (so Middleman copies them into the
// build), and writes src/data/images.json so the _image.erb partial knows which
// widths exist and each image's intrinsic dimensions.
//
// sharp drops all input metadata by default (we never call .keepMetadata/
// .withMetadata), so the published variants carry no EXIF/GPS/location data.
//
// Originals are committed to git; the generated variants and manifest are not
// (see .gitignore) — they are rebuilt on every `make build`.

import sharp from "sharp";
import { readdir, mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SRC_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_DIR = path.join(SRC_ROOT, "images");
const OUT_DIR = path.join(SRC_ROOT, "source", "images");
const MANIFEST = path.join(SRC_ROOT, "data", "images.json");

// Matches the srcset breakpoints in source/partials/_image.erb. The content
// column is 750px, so 750 covers 1x displays and 1500 covers 2x (retina).
const WIDTHS = [750, 1500];

// AVIF first (smallest), WebP as the universally-supported fallback. JPEG is
// omitted: every browser that lacks both AVIF and WebP is long EOL.
const FORMATS = [
  { ext: "avif", opts: { quality: 50 } },
  { ext: "webp", opts: { quality: 80 } },
];

const INPUT_RE = /\.(jpe?g|png|tiff?)$/i;

async function main() {
  let files;
  try {
    files = (await readdir(SOURCE_DIR)).filter((f) => INPUT_RE.test(f));
  } catch {
    console.error(`No source images directory at ${SOURCE_DIR}`);
    process.exit(1);
  }

  // Rebuild from scratch so renamed/removed originals don't leave stragglers.
  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(OUT_DIR, { recursive: true });
  await mkdir(path.dirname(MANIFEST), { recursive: true });

  const manifest = {};

  for (const file of files) {
    const name = file.replace(INPUT_RE, "");
    const input = path.join(SOURCE_DIR, file);

    // metadata().width/height are the raw header dims and ignore EXIF
    // orientation; meta.autoOrient holds the corrected dims that match what
    // .rotate() produces in the output variants below.
    const meta = await sharp(input).metadata();
    const { width: nativeWidth, height: nativeHeight } = meta.autoOrient ?? meta;

    // Never upscale: cap the requested widths at the native width.
    let widths = WIDTHS.filter((w) => w <= nativeWidth);
    if (widths.length === 0) widths = [nativeWidth];

    for (const width of widths) {
      const pipeline = sharp(input).rotate().resize({ width, withoutEnlargement: true });
      for (const fmt of FORMATS) {
        const out = path.join(OUT_DIR, `${name}-${width}.${fmt.ext}`);
        await pipeline
          .clone()
          .toFormat(fmt.sharpFormat ?? fmt.ext, fmt.opts)
          .toFile(out);
      }
    }

    manifest[name] = {
      width: nativeWidth,
      height: nativeHeight,
      widths,
    };
    console.log(`✓ ${name}  (${nativeWidth}×${nativeHeight}) → ${widths.join(", ")} px`);
  }

  await writeFile(MANIFEST, JSON.stringify(manifest, null, 2) + "\n");
  console.log(`\nWrote ${Object.keys(manifest).length} image(s) to manifest ${path.relative(SRC_ROOT, MANIFEST)}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
