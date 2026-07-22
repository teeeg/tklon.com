//! Authoring-time media pipelines:
//!   `tklon images`        — pure-Rust responsive image variants + images.json
//!   `tklon video [src]`   — ffmpeg encode + S3 upload + videos.json
//!   `tklon video --check` — pre-push parity guard
//!
//! Behaviour matches the retired Node scripts: variant filenames embed
//! `sha256(master)[:8]`, widths 750/1500 (never upscaled), AVIF q50 / WebP q80,
//! EXIF stripped, orientation baked in.

use crate::config;
use crate::model::{ImageMeta, Res};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io};

const WIDTHS: [u32; 2] = [750, 1500];
const AVIF_QUALITY: f32 = 50.0;
const WEBP_QUALITY: f32 = 80.0;
const AVIF_SPEED: u8 = 6;

const IMAGE_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "tif", "tiff"];
const VIDEO_EXTS: [&str; 5] = ["mov", "mp4", "m4v", "mkv", "webm"];

/// First `n` hex chars of the sha256 of `bytes`.
pub fn sha256_prefix(bytes: &[u8], n: usize) -> String {
    let digest = Sha256::new().chain_update(bytes).finalize();
    let mut hex = String::with_capacity(n);
    for b in digest.iter() {
        if hex.len() >= n {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(n);
    hex
}

// ===========================================================================
// tklon images
// ===========================================================================

/// Regenerate AVIF/WebP variants + site/data/images.json from site/images/.
/// Idempotent: a variant whose file already exists is left untouched.
pub fn images(root: &Path) -> Res<()> {
    let bucket = config::load(root)?.bucket;
    let site = root.join("site");
    let masters = site.join("images");
    let out = site.join("source/images");
    fs::create_dir_all(&out)?;

    let mut files: Vec<PathBuf> = fs::read_dir(&masters)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| has_ext(p, &IMAGE_EXTS))
        .collect();
    files.sort();

    // Merge into the existing manifest (append-only) so a run that only has the
    // newly-added masters — e.g. CI processing a phone attachment — keeps every
    // other image's entry instead of dropping it.
    let manifest_path = site.join("data/images.json");
    let mut manifest: BTreeMap<String, ImageMeta> = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let (mut encoded, mut skipped) = (0u32, 0u32);

    for path in &files {
        let name = stem(path)?;
        let bytes = fs::read(path)?;
        let digest = sha256_prefix(&bytes, 8);

        let (img, nw, nh) = decode_oriented(path)?;
        let mut widths: Vec<u32> = WIDTHS.into_iter().filter(|w| *w <= nw).collect();
        if widths.is_empty() {
            widths = vec![nw];
        }

        for &w in &widths {
            let th = ((nh as u64 * w as u64 + nw as u64 / 2) / nw as u64) as u32;
            let avif_path = out.join(format!("{name}-{w}-{digest}.avif"));
            let webp_path = out.join(format!("{name}-{w}-{digest}.webp"));
            if avif_path.exists() && webp_path.exists() {
                skipped += 1;
                continue;
            }
            let resized = if w == nw {
                img.clone()
            } else {
                img.resize_exact(w, th, image::imageops::FilterType::Lanczos3)
            };
            if !avif_path.exists() {
                fs::write(&avif_path, encode_avif(&resized)?)?;
            }
            if !webp_path.exists() {
                fs::write(&webp_path, encode_webp(&resized))?;
            }
            encoded += 1;
        }

        let thumbhash = Some(compute_thumbhash(&img));
        let (camera, settings) = read_exif(&bytes);
        println!(
            "✓ {name}  ({nw}×{nh}) → {} px  [{digest}]{}",
            join_widths(&widths),
            camera.as_deref().map(|c| format!("  · {c}")).unwrap_or_default()
        );
        manifest.insert(
            name,
            ImageMeta { width: nw, height: nh, widths, digest, thumbhash, camera, settings },
        );
    }

    fs::create_dir_all(manifest_path.parent().unwrap())?;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    println!(
        "\nwrote {} image(s) to {} ({encoded} variant group(s) encoded, {skipped} unchanged)",
        manifest.len(),
        manifest_path.display()
    );
    backup_masters(&masters, &bucket);
    Ok(())
}

/// Back up the (gitignored) masters to S3, mirroring `tklon video`. Best-effort:
/// never fails variant generation, but warns loudly if the originals — which no
/// longer live in git — aren't protected.
fn backup_masters(dir: &Path, bucket: &str) {
    let dest = format!("s3://{bucket}/masters/");
    let ok = Command::new("aws")
        .args(["s3", "sync", &dir.to_string_lossy(), &dest, "--size-only", "--exclude", ".*"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("✓ masters backed up to {dest}");
    } else {
        eprintln!(
            "⚠ masters NOT backed up to S3 — originals live only in {} \
(run `tklon images` with aws creds to protect them)",
            dir.display()
        );
    }
}

/// Decode an image, bake in EXIF orientation, and return it with its
/// (orientation-corrected) intrinsic dimensions.
fn decode_oriented(path: &Path) -> Res<(DynamicImage, u32, u32)> {
    let mut decoder = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    let (w, h) = (img.width(), img.height());
    Ok((img, w, h))
}

fn encode_avif(img: &DynamicImage) -> Res<Vec<u8>> {
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let pixels: Vec<rgb::RGB8> = rgb.pixels().map(|p| rgb::RGB8::new(p[0], p[1], p[2])).collect();
    let encoded = ravif::Encoder::new()
        .with_quality(AVIF_QUALITY)
        .with_speed(AVIF_SPEED)
        .encode_rgb(imgref::Img::new(&pixels[..], w, h))?;
    Ok(encoded.avif_file)
}

fn encode_webp(img: &DynamicImage) -> Vec<u8> {
    let encoder = webp::Encoder::from_image(img).expect("unsupported pixel layout for webp");
    encoder.encode(WEBP_QUALITY).to_vec()
}

/// Compute a base64 ThumbHash from a ≤100px thumbnail of the image.
fn compute_thumbhash(img: &DynamicImage) -> String {
    let small = img.thumbnail(100, 100);
    let rgba = small.to_rgba8();
    let hash = thumbhash::rgba_to_thumb_hash(rgba.width() as usize, rgba.height() as usize, &rgba);
    B64.encode(hash)
}

/// Average colour of a base64 ThumbHash as `#rrggbb` — a near-free placeholder
/// that keeps the HTML lean. This is the default (see markdown.rs).
pub fn thumbhash_to_color(hash_b64: &str) -> Option<String> {
    let bytes = B64.decode(hash_b64).ok()?;
    let (r, g, b, _a) = thumbhash::thumb_hash_to_average_rgba(&bytes).ok()?;
    let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!("#{:02x}{:02x}{:02x}", q(r), q(g), q(b)))
}

/// Read a safe subset of EXIF from the master (camera + capture settings).
/// GPS and other location tags are never queried.
fn read_exif(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let mut cur = Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cur) else {
        return (None, None);
    };
    let field = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| clean(&f.display_value().to_string()))
            .filter(|s| !s.is_empty())
    };

    let camera = combine_camera(field(exif::Tag::Make), field(exif::Tag::Model));

    let mut parts = Vec::new();
    if let Some(f) = field(exif::Tag::FocalLengthIn35mmFilm).or_else(|| field(exif::Tag::FocalLength)) {
        parts.push(format!("{f} mm"));
    }
    if let Some(f) = field(exif::Tag::FNumber) {
        parts.push(format!("f/{f}"));
    }
    if let Some(f) = field(exif::Tag::ExposureTime) {
        parts.push(format!("{f} s"));
    }
    if let Some(f) = field(exif::Tag::PhotographicSensitivity) {
        parts.push(format!("ISO {f}"));
    }
    let settings = (!parts.is_empty()).then(|| parts.join(" · "));
    (camera, settings)
}

fn combine_camera(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(mk), Some(md)) => {
            if md.to_lowercase().starts_with(&mk.to_lowercase()) {
                Some(md)
            } else {
                Some(format!("{mk} {md}"))
            }
        }
        (None, Some(md)) => Some(md),
        (Some(mk), None) => Some(mk),
        (None, None) => None,
    }
}

fn clean(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

// ===========================================================================
// tklon video
// ===========================================================================

// Bound BOTH dimensions so a tall/4K vertical clip can't balloon past 1080p in
// its long edge; decrease keeps aspect; divisible_by=2 keeps dims even for H.264.
const SCALE: &str =
    "scale='min(1920,iw)':'min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2";

/// Encode one source (or every source in site/videos/ when `positional` is None),
/// upload the MP4 + master to S3, and record it in videos.json.
pub fn video(root: &Path, positional: Option<String>) -> Res<()> {
    let bucket = config::load(root)?.bucket;
    let site = root.join("site");
    let video_dir = site.join("videos");

    let files: Vec<String> = match positional {
        Some(p) => vec![Path::new(&p)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("bad video path")?
            .to_string()],
        None => {
            let mut v: Vec<String> = match fs::read_dir(&video_dir) {
                Ok(rd) => rd
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| has_ext(p, &VIDEO_EXTS))
                    .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
                    .collect(),
                Err(_) => return Err(format!("no video directory at {}", video_dir.display()).into()),
            };
            v.sort();
            if v.is_empty() {
                println!("no videos in {} — nothing to do.", video_dir.display());
                return Ok(());
            }
            v
        }
    };

    for file in &files {
        process_video(&site, &video_dir, file, &bucket)?;
    }
    // Posters were just written into site/images/; generate their variants + manifest.
    images(root)?;
    Ok(())
}

fn process_video(site: &Path, video_dir: &Path, file: &str, bucket: &str) -> Res<()> {
    let name = strip_known_ext(file, &VIDEO_EXTS);
    let input = video_dir.join(file);
    let out_dir = video_dir.join(".out");
    let poster_dir = site.join("images");
    let manifest_path = site.join("data/videos.json");
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&poster_dir)?;
    fs::create_dir_all(manifest_path.parent().unwrap())?;

    // 0. Short-circuit unchanged sources (ffmpeg + aws are both expensive).
    let source_hash = sha256_prefix(&fs::read(&input)?, 16);
    let mut manifest = read_json_map(&manifest_path);
    if manifest
        .get(&name)
        .and_then(|e| e.get("sourceHash"))
        .and_then(|h| h.as_str())
        == Some(source_hash.as_str())
    {
        println!("✓ {name}  (unchanged — skipping encode/upload)");
        return Ok(());
    }

    // 1. Encode to a temp name, then content-hash → final name.
    let tmp = out_dir.join(format!("{name}.tmp.mp4"));
    println!("→ encoding {file}");
    run("ffmpeg", &[
        "-y", "-i", input.to_str().unwrap(),
        "-vf", SCALE,
        "-c:v", "libx264", "-crf", "23", "-preset", "slow", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart",
        "-c:a", "aac", "-b:a", "128k",
        tmp.to_str().unwrap(),
    ])?;
    let hash = sha256_prefix(&fs::read(&tmp)?, 8);
    let final_name = format!("{name}-{hash}.mp4");
    let final_path = out_dir.join(&final_name);
    fs::rename(&tmp, &final_path)?;

    // 2. Poster frame (small seek + thumbnail dodges fade-in/black frames).
    let poster = format!("{name}-poster");
    run("ffmpeg", &[
        "-y", "-ss", "1", "-i", input.to_str().unwrap(),
        "-vf", "thumbnail", "-frames:v", "1", "-q:v", "2", "-update", "1",
        poster_dir.join(format!("{poster}.jpg")).to_str().unwrap(),
    ])?;

    // 3. Probe the ENCODED file so dims match what's actually served.
    let (width, height, duration) = probe(&final_path)?;

    // 4. Upload the encoded MP4 (immutable — content-addressed name)…
    println!("→ uploading {final_name}");
    run("aws", &[
        "s3", "cp", final_path.to_str().unwrap(),
        &format!("s3://{bucket}/media/{final_name}"),
        "--content-type", "video/mp4",
        "--cache-control", "public, max-age=31536000, immutable",
    ])?;
    // …and back up the untouched master (the only irreplaceable file in the chain).
    println!("→ backing up master {file}");
    run("aws", &[
        "s3", "cp", input.to_str().unwrap(),
        &format!("s3://{bucket}/masters/{file}"),
    ])?;

    // 5. Record in the committed manifest only after a successful upload.
    manifest.insert(
        name.clone(),
        serde_json::json!({
            "src": final_name,
            "width": width,
            "height": height,
            "duration": duration,
            "poster": poster,
            "sourceHash": source_hash,
        }),
    );
    write_json_map(&manifest_path, &manifest)?;
    println!("✓ {name}  ({width}×{height}, {duration}s) → {final_name}\n");
    Ok(())
}

/// ffprobe the encoded file for stream width/height and format duration.
fn probe(path: &Path) -> Res<(u64, u64, f64)> {
    let out = run_capture("ffprobe", &[
        "-v", "error", "-select_streams", "v:0",
        "-show_entries", "stream=width,height",
        "-show_entries", "format=duration",
        "-of", "json", path.to_str().unwrap(),
    ])?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    let stream = &v["streams"][0];
    let width = stream["width"].as_u64().ok_or("ffprobe: no width")?;
    let height = stream["height"].as_u64().ok_or("ffprobe: no height")?;
    let raw: f64 = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("ffprobe: no duration")?;
    Ok((width, height, (raw * 10.0).round() / 10.0))
}

// ===========================================================================
// tklon video --check  (pre-push parity guard)
// ===========================================================================

/// Fail if any source in site/videos/ is missing from videos.json or differs
/// from what was last encoded. Absence of the sources dir is fine (fresh clone).
pub fn check_videos(root: &Path) -> Res<()> {
    let site = root.join("site");
    let video_dir = site.join("videos");
    let manifest_path = site.join("data/videos.json");

    let mut sources: Vec<PathBuf> = match fs::read_dir(&video_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| has_ext(p, &VIDEO_EXTS))
            .collect(),
        Err(_) => return Ok(()),
    };
    sources.sort();
    if sources.is_empty() {
        return Ok(());
    }

    let manifest = read_json_map(&manifest_path);
    let manifest_mtime = fs::metadata(&manifest_path).and_then(|m| m.modified()).ok();

    let mut problems = Vec::new();
    for path in &sources {
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let name = strip_known_ext(file, &VIDEO_EXTS);
        let Some(entry) = manifest.get(&name) else {
            problems.push(format!("  {file} → no entry in data/videos.json"));
            continue;
        };
        match entry.get("sourceHash").and_then(|h| h.as_str()) {
            Some(recorded) => {
                if sha256_prefix(&fs::read(path)?, 16) != recorded {
                    problems.push(format!("  {file} → content differs from last encode (sourceHash mismatch)"));
                }
            }
            None => {
                // Legacy entry: fall back to mtime (weaker but still catches edits).
                if let (Ok(src_m), Some(man_m)) =
                    (fs::metadata(path).and_then(|m| m.modified()), manifest_mtime)
                {
                    if src_m > man_m {
                        problems.push(format!("  {file} → modified since last encode (source newer than manifest)"));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        println!("✓ video sources are in sync with data/videos.json");
        Ok(())
    } else {
        let mut msg = String::from("video sources are out of sync with data/videos.json:\n\n");
        msg.push_str(&problems.join("\n"));
        msg.push_str("\n\nRun `tklon video` to re-encode and upload, then commit the manifest.");
        Err(msg.into())
    }
}

// ===========================================================================
// tklon video --prune  (GC orphaned /media/ objects)
// ===========================================================================

/// Delete objects under `s3://…/media/` that no longer appear in videos.json
/// (left behind by re-encodes / renames). Requires `--yes` to actually delete.
pub fn prune(root: &Path, yes: bool) -> Res<()> {
    let bucket = config::load(root)?.bucket;
    let manifest = read_json_map(&root.join("site/data/videos.json"));
    let referenced: HashSet<&str> = manifest
        .values()
        .filter_map(|v| v.get("src").and_then(|s| s.as_str()))
        .collect();

    let listing = run_capture("aws", &[
        "s3api", "list-objects-v2", "--bucket", &bucket, "--prefix", "media/",
        "--query", "Contents[].Key", "--output", "text",
    ])?;
    let listing = listing.trim();
    let keys: Vec<&str> = if listing.is_empty() || listing == "None" {
        Vec::new()
    } else {
        listing.split_whitespace().collect()
    };

    let orphans: Vec<&str> = keys
        .iter()
        .map(|k| k.strip_prefix("media/").unwrap_or(k))
        .filter(|f| !f.is_empty() && !referenced.contains(f))
        .collect();

    if orphans.is_empty() {
        println!("no orphaned media objects — /media/ matches the manifest.");
        return Ok(());
    }
    println!("orphaned /media/ objects (not referenced by data/videos.json):");
    for o in &orphans {
        println!("  {o}");
    }
    if !yes {
        println!("\nre-run with `--prune --yes` to delete the {} object(s) above.", orphans.len());
        return Ok(());
    }
    for o in &orphans {
        run("aws", &["s3", "rm", &format!("s3://{bucket}/media/{o}")])?;
    }
    println!("deleted {} object(s).", orphans.len());
    Ok(())
}

// ===========================================================================
// helpers
// ===========================================================================

fn run(cmd: &str, args: &[&str]) -> Res<()> {
    let status = Command::new(cmd).args(args).status().map_err(|e| map_spawn_err(cmd, e))?;
    if !status.success() {
        return Err(format!("{cmd} exited with {status}").into());
    }
    Ok(())
}

fn run_capture(cmd: &str, args: &[&str]) -> Res<String> {
    let output = Command::new(cmd).args(args).output().map_err(|e| map_spawn_err(cmd, e))?;
    if !output.status.success() {
        return Err(format!(
            "{cmd} exited with {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn map_spawn_err(cmd: &str, e: io::Error) -> Box<dyn std::error::Error> {
    if e.kind() == io::ErrorKind::NotFound {
        format!("`{cmd}` not found — is it installed and on PATH?").into()
    } else {
        format!("failed to run {cmd}: {e}").into()
    }
}

fn read_json_map(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json_map(path: &Path, map: &serde_json::Map<String, serde_json::Value>) -> Res<()> {
    // serde_json's Map is sorted by default → deterministic, merge-friendly diffs.
    fs::write(path, serde_json::to_string_pretty(map)? + "\n")?;
    Ok(())
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| exts.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn stem(path: &Path) -> Res<String> {
    Ok(path.file_stem().and_then(|s| s.to_str()).ok_or("bad filename")?.to_string())
}

fn strip_known_ext(file: &str, exts: &[&str]) -> String {
    match Path::new(file).extension().and_then(|e| e.to_str()) {
        Some(e) if exts.contains(&e.to_ascii_lowercase().as_str()) => {
            file[..file.len() - e.len() - 1].to_string()
        }
        _ => file.to_string(),
    }
}

fn join_widths(widths: &[u32]) -> String {
    widths.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_prefix_known_vector() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(sha256_prefix(b"abc", 8), "ba7816bf");
    }

    #[test]
    fn sha256_prefix_various_lengths() {
        assert_eq!(sha256_prefix(b"abc", 8).len(), 8);
        assert_eq!(sha256_prefix(b"abc", 16).len(), 16);
        assert_eq!(sha256_prefix(b"abc", 1).len(), 1);
        assert_eq!(sha256_prefix(b"abc", 64).len(), 64);
    }

    #[test]
    fn sha256_prefix_16_and_full() {
        assert_eq!(sha256_prefix(b"abc", 16), "ba7816bf8f01cfea");
        assert_eq!(
            sha256_prefix(b"abc", 64),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_prefix_empty_input() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(sha256_prefix(b"", 8), "e3b0c442");
    }

    #[test]
    fn sha256_prefix_odd_length() {
        // A single byte pushes two hex chars, so odd n truncates mid-byte.
        assert_eq!(sha256_prefix(b"abc", 3), "ba7");
    }

    #[test]
    fn combine_camera_make_and_distinct_model() {
        assert_eq!(
            combine_camera(Some("Apple".into()), Some("iPhone 15".into())),
            Some("Apple iPhone 15".into())
        );
    }

    #[test]
    fn combine_camera_model_starts_with_make() {
        assert_eq!(
            combine_camera(Some("FUJIFILM".into()), Some("FUJIFILM X100V".into())),
            Some("FUJIFILM X100V".into())
        );
    }

    #[test]
    fn combine_camera_model_starts_with_make_case_insensitive() {
        assert_eq!(
            combine_camera(Some("Canon".into()), Some("canon EOS R5".into())),
            Some("canon EOS R5".into())
        );
    }

    #[test]
    fn combine_camera_model_only() {
        assert_eq!(combine_camera(None, Some("X100V".into())), Some("X100V".into()));
    }

    #[test]
    fn combine_camera_make_only() {
        assert_eq!(combine_camera(Some("Canon".into()), None), Some("Canon".into()));
    }

    #[test]
    fn combine_camera_neither() {
        assert_eq!(combine_camera(None, None), None);
    }
}
