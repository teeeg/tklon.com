//! `tklon ingest` — wire attached photos into a post's `@` placeholders.
//!
//! A post written on mobile marks image slots with `![alt](@)`; the author
//! attaches the photos to the PR in the same order. This fills the Nth `@` with
//! a `{slug}-{n}` name, copies the Nth media file into site/images/ (a master),
//! and rewrites the post. `tklon images` then encodes + backs up as usual.

use crate::model::Res;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Fill the post's `@` image placeholders with `media` (in order).
pub fn ingest(root: &Path, post: &Path, media: &[PathBuf]) -> Res<()> {
    let slug = post
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("bad post filename")?;
    let content = fs::read_to_string(post)?;
    let slots = placeholders(&content);
    if slots.len() != media.len() {
        return Err(format!(
            "post has {} `@` placeholder(s) but {} media file(s) were provided",
            slots.len(),
            media.len()
        )
        .into());
    }

    let images_dir = root.join("site/images");
    fs::create_dir_all(&images_dir)?;

    // Rewrite back-to-front so earlier byte offsets stay valid.
    let mut out = content.clone();
    let mut names = vec![String::new(); slots.len()];
    for i in (0..slots.len()).rev() {
        let ext = sniff_ext(&read_head(&media[i])?)?;
        let name = format!("{slug}-{}", i + 1);
        fs::copy(&media[i], images_dir.join(format!("{name}.{ext}")))?;
        out.replace_range(slots[i].clone(), &name);
        names[i] = name;
    }
    fs::write(post, out)?;

    // Emit the assigned names in order for the workflow's preview.
    for name in &names {
        println!("{name}");
    }
    Ok(())
}

/// Byte ranges of each `@` that is an image link target (`![alt](@)` or
/// `![alt](@ "caption")`).
fn placeholders(s: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = s.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("](@") {
        let at = from + rel + 2; // index of '@'
        match bytes.get(at + 1) {
            Some(b')') | Some(b' ') => ranges.push(at..at + 1),
            _ => {}
        }
        from = at + 1;
    }
    ranges
}

fn read_head(path: &Path) -> Res<Vec<u8>> {
    let mut buf = vec![0u8; 16];
    let n = fs::File::open(path)?.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

/// Detect a supported image format from its leading bytes.
fn sniff_ext(head: &[u8]) -> Res<&'static str> {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok("jpg")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        Ok("png")
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" {
        Err("HEIC/HEIF isn't supported — attach the photo as JPEG (iOS usually \
converts to JPEG on upload)"
            .into())
    } else {
        Err("unsupported image format (expected JPEG or PNG)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(s: &str, ranges: &[std::ops::Range<usize>]) -> Vec<String> {
        ranges.iter().map(|r| s[r.clone()].to_string()).collect()
    }

    #[test]
    fn finds_plain_and_captioned_placeholders() {
        let s = "a\n\n![the climb](@)\n\nb\n\n![view](@ \"worth it\")\n";
        let p = placeholders(s);
        assert_eq!(p.len(), 2);
        assert_eq!(fill(s, &p), vec!["@", "@"]);
    }

    #[test]
    fn ignores_at_that_is_not_a_link_target() {
        // email-ish and inline @ are not `](@`
        let s = "reach me @handle or foo](bar) and ![x](name)";
        assert_eq!(placeholders(s).len(), 0);
    }

    #[test]
    fn ignores_at_inside_a_real_url_target() {
        let s = "![x](https://a.com/@u)"; // `](@` never occurs
        assert_eq!(placeholders(s).len(), 0);
    }

    #[test]
    fn replacing_the_range_keeps_alt_and_caption() {
        let s = "![the climb](@ \"cap\")";
        let p = placeholders(s);
        let mut out = s.to_string();
        out.replace_range(p[0].clone(), "morning-ride-1");
        assert_eq!(out, "![the climb](morning-ride-1 \"cap\")");
    }

    #[test]
    fn sniff_jpeg_png_and_rejects_heic() {
        assert_eq!(sniff_ext(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap(), "jpg");
        assert_eq!(sniff_ext(b"\x89PNG\r\n\x1a\n").unwrap(), "png");
        let heic = b"\0\0\0\x18ftypheic\0\0\0\0";
        assert!(sniff_ext(heic).is_err());
        assert!(sniff_ext(b"GIF89a").is_err());
    }
}
