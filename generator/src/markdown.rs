//! Markdown rendering: native-Markdown responsive images, `{{< video >}}` /
//! `{{< embed >}}` shortcodes, CommonMark with smart punctuation, and
//! Kramdown-compatible heading ids.

use crate::model::{Images, Res, Videos};
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

/// HTML-escape text for element/attribute content (matches ERB `h()`).
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Render a post body: expand block shortcodes, then markdown → HTML (with
/// bare-name image links turned into responsive figures).
pub fn render_body(body: &str, images: &Images, videos: &Videos) -> Res<String> {
    let expanded = expand_block_shortcodes(body, images, videos)?;
    render_markdown(&expanded, images)
}

fn render_markdown(md: &str, images: &Images) -> Res<String> {
    let protected = protect_svg(md);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let events: Vec<Event> = Parser::new_ext(&protected, opts).collect();

    let events = transform_images(events, images)?;
    let events = inject_heading_ids(events);

    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    Ok(out)
}

// ---- responsive images (native `![alt](name "caption")`) ------------------

/// Replace paragraphs that consist solely of a bare-name image link with the
/// `<figure>` HTML the Middleman image partial produced.
fn transform_images<'a>(events: Vec<Event<'a>>, images: &Images) -> Res<Vec<Event<'a>>> {
    let mut out = Vec::with_capacity(events.len());
    let mut i = 0;
    while i < events.len() {
        if matches!(events[i], Event::Start(Tag::Paragraph)) {
            if let Some((html, next)) = try_image_paragraph(&events, i, images)? {
                out.push(Event::Html(CowStr::from(html)));
                i = next;
                continue;
            }
        }
        out.push(events[i].clone());
        i += 1;
    }
    Ok(out)
}

/// If the paragraph at `start` is exactly one bare-name image, return its
/// figure HTML and the index just past the paragraph.
fn try_image_paragraph(
    events: &[Event],
    start: usize,
    images: &Images,
) -> Res<Option<(String, usize)>> {
    let Some(Event::Start(Tag::Image { dest_url, title, .. })) = events.get(start + 1) else {
        return Ok(None);
    };
    // Only bare manifest names (no path, scheme, or extension) are ours.
    if dest_url.contains('/') || dest_url.contains(':') || dest_url.contains('.') {
        return Ok(None);
    }
    let mut k = start + 2;
    let mut alt = String::new();
    while k < events.len() {
        match &events[k] {
            Event::End(TagEnd::Image) => break,
            Event::Text(t) | Event::Code(t) => alt.push_str(t),
            _ => {}
        }
        k += 1;
    }
    // require the image to be the whole paragraph
    if !matches!(events.get(k + 1), Some(Event::End(TagEnd::Paragraph))) {
        return Ok(None);
    }
    let caption = if title.is_empty() {
        None
    } else {
        Some(title.as_ref())
    };
    let html = image_html(dest_url, &alt, caption, images)?;
    Ok(Some((html, k + 2)))
}

fn image_html(name: &str, alt: &str, caption: Option<&str>, images: &Images) -> Res<String> {
    if alt.is_empty() && caption.is_none() {
        return Err(format!("image '{name}' needs alt text or a caption").into());
    }
    let meta = images
        .get(name)
        .ok_or_else(|| format!("unknown image '{name}' in images.json"))?;
    let sizes = "(min-width: 750px) 750px, 100vw";
    let digest = &meta.digest;
    let max_w = *meta.widths.iter().max().unwrap_or(&750);
    let fallback = if meta.widths.contains(&750) { 750 } else { max_w };
    let srcset = |ext: &str| {
        meta.widths
            .iter()
            .map(|w| format!("/images/{name}-{w}-{digest}.{ext} {w}w"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let full = format!("/images/{name}-{max_w}-{digest}.webp");

    // Placeholder shown while the image loads: the ThumbHash's average colour.
    let placeholder = meta
        .thumbhash
        .as_deref()
        .and_then(crate::media::thumbhash_to_color)
        .map(|c| format!(" style=\"background-color:{c}\""))
        .unwrap_or_default();

    let mut cap = caption.map(esc).unwrap_or_default();
    if let Some(camera) = &meta.camera {
        let mut line = esc(camera);
        if let Some(s) = &meta.settings {
            line.push_str(" · ");
            line.push_str(&esc(s));
        }
        if !cap.is_empty() {
            cap.push_str("<br>");
        }
        cap.push_str(&format!("<span class=\"exif\">{line}</span>"));
    }
    let figcaption = if cap.is_empty() {
        String::new()
    } else {
        format!("<figcaption>{cap}</figcaption>")
    };

    Ok(format!(
        "<figure><a class=\"zoom\" href=\"{full}\" aria-label=\"View image full screen\">\
<picture>\
<source type=\"image/avif\" sizes=\"{sizes}\" srcset=\"{avif}\" />\
<source type=\"image/webp\" sizes=\"{sizes}\" srcset=\"{webp}\" />\
<img{placeholder} src=\"/images/{name}-{fallback}-{digest}.webp\" width=\"{w}\" height=\"{h}\" \
loading=\"lazy\" decoding=\"async\" alt=\"{alt}\" />\
</picture></a>{figcaption}</figure>",
        avif = srcset("avif"),
        webp = srcset("webp"),
        w = meta.width,
        h = meta.height,
        alt = esc(alt),
    ))
}

// ---- heading ids ----------------------------------------------------------

fn inject_heading_ids(mut events: Vec<Event>) -> Vec<Event> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let n = events.len();
    for i in 0..n {
        if !matches!(events[i], Event::Start(Tag::Heading { .. })) {
            continue;
        }
        let mut text = String::new();
        for ev in events.iter().take(n).skip(i + 1) {
            match ev {
                Event::End(TagEnd::Heading(_)) => break,
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                _ => {}
            }
        }
        let slug = unique_slug(&mut counts, &kramdown_slug(&text));
        if let Event::Start(Tag::Heading { level, classes, attrs, .. }) = events[i].clone() {
            events[i] = Event::Start(Tag::Heading {
                level,
                id: Some(CowStr::from(slug)),
                classes,
                attrs,
            });
        }
    }
    events
}

/// Merge author-written `<svg>…</svg>` blocks (which contain blank lines) into a
/// single CommonMark HTML block so the blank lines don't terminate it early.
fn protect_svg(md: &str) -> String {
    let mut out = String::new();
    let mut in_svg = false;
    for line in md.lines() {
        let t = line.trim_start();
        if !in_svg && t.starts_with("<svg") {
            in_svg = true;
            out.push_str(line);
            out.push('\n');
            if line.contains("</svg>") {
                in_svg = false;
            }
            continue;
        }
        if in_svg {
            if t.is_empty() {
                continue; // drop blank lines inside the svg
            }
            out.push_str(line);
            out.push('\n');
            if line.contains("</svg>") {
                in_svg = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Kramdown `generate_id`: strip leading non-letters, drop chars outside
/// [A-Za-z0-9 -], turn each space into `-` (no collapsing), downcase.
pub fn kramdown_slug(text: &str) -> String {
    let mut started = false;
    let mut s = String::new();
    for c in text.chars() {
        if !started {
            if c.is_ascii_alphabetic() {
                started = true;
            } else {
                continue;
            }
        }
        if c.is_ascii_alphanumeric() || c == '-' {
            s.push(c);
        } else if c == ' ' {
            s.push('-');
        }
    }
    let s = s.to_ascii_lowercase();
    if s.is_empty() {
        "section".to_string()
    } else {
        s
    }
}

fn unique_slug(counts: &mut HashMap<String, usize>, base: &str) -> String {
    let c = counts.entry(base.to_string()).or_insert(0);
    let slug = if *c == 0 {
        base.to_string()
    } else {
        format!("{}-{}", base, c)
    };
    *c += 1;
    slug
}

// ---- block shortcodes: {{< video … >}} / {{< embed … >}} ------------------

/// Expand Hugo-style `{{< name key="value" … >}}` block shortcodes.
pub fn expand_block_shortcodes(body: &str, images: &Images, videos: &Videos) -> Res<String> {
    let mut out = String::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{<") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let end = after.find(">}}").ok_or("unterminated {{< shortcode")?;
        let inner = after[..end].trim();
        out.push_str(&render_shortcode(inner, images, videos)?);
        rest = &after[end + 3..];
    }
    out.push_str(rest);
    Ok(out)
}

fn render_shortcode(inner: &str, images: &Images, videos: &Videos) -> Res<String> {
    let (name, argstr) = match inner.find(char::is_whitespace) {
        Some(p) => (&inner[..p], inner[p..].trim()),
        None => (inner, ""),
    };
    let attrs = parse_attrs(argstr);
    let get = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    match name {
        "video" => {
            let vname = get("name").ok_or("video shortcode needs name=")?;
            video_html(vname, get("caption"), images, videos)
        }
        "embed" => {
            let url = get("url").ok_or("embed shortcode needs url=")?;
            Ok(embed_html(url, get("title").unwrap_or("")))
        }
        other => Err(format!("unknown shortcode '{other}'").into()),
    }
}

fn video_html(name: &str, caption: Option<&str>, images: &Images, videos: &Videos) -> Res<String> {
    let meta = videos
        .get(name)
        .ok_or_else(|| format!("unknown video '{name}' in videos.json"))?;
    let poster = images.get(&meta.poster).ok_or_else(|| {
        format!("video '{name}' poster '{}' missing from images.json", meta.poster)
    })?;
    let figcaption = caption
        .map(|c| format!("<figcaption>{}</figcaption>", esc(c)))
        .unwrap_or_default();
    // Tint the video box with the poster's average colour (no black flash before
    // the poster paints).
    let color = poster
        .thumbhash
        .as_deref()
        .and_then(crate::media::thumbhash_to_color)
        .map(|c| format!(" style=\"background-color:{c}\""))
        .unwrap_or_default();
    Ok(format!(
        "<figure class=\"video\"><video controls playsinline preload=\"none\"{color} \
width=\"{w}\" height=\"{h}\" poster=\"/images/{poster}-750-{pd}.webp\">\
<source src=\"/media/{src}\" type=\"video/mp4\" /></video>{figcaption}</figure>",
        w = meta.width,
        h = meta.height,
        poster = meta.poster,
        pd = poster.digest,
        src = meta.src,
    ))
}

fn embed_html(url: &str, title: &str) -> String {
    format!(
        "<div class=\"video\"><iframe width=\"560\" height=\"315\" src=\"{url}\" \
title=\"{t}\" allowfullscreen></iframe><a href=\"{url}\">{t}</a></div>",
        url = esc(url),
        t = esc(title),
    )
}

/// Parse `key="value"` pairs (values may contain spaces).
fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut rest = s.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].split_whitespace().last().unwrap_or("").to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quoted) = after.strip_prefix('"') else {
            break;
        };
        let Some(end) = quoted.find('"') else { break };
        if !key.is_empty() {
            attrs.push((key, quoted[..end].to_string()));
        }
        rest = &quoted[end + 1..];
    }
    attrs
}

// ---- meta description -----------------------------------------------------

/// Replicate the layout's description logic: drop a leading heading, strip
/// tags, collapse whitespace, truncate to ~155 chars.
pub fn description_from(body_html: &str) -> String {
    let mut html = body_html.trim_start();
    if let Some(rest) = strip_leading_heading(html) {
        html = rest.trim_start();
    }
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 155 {
        let head: String = collapsed.chars().take(152).collect();
        format!("{}…", head.trim_end())
    } else {
        collapsed
    }
}

fn strip_leading_heading(html: &str) -> Option<&str> {
    let bytes = html.as_bytes();
    if bytes.first() != Some(&b'<') || bytes.get(1) != Some(&b'h') {
        return None;
    }
    let level = *bytes.get(2)?;
    if !(b'1'..=b'6').contains(&level) {
        return None;
    }
    let close = format!("</h{}>", level as char);
    let idx = html.find(&close)?;
    Some(&html[idx + close.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_no_dash_collapsing() {
        // " - " becomes three dashes: space, literal '-', space → "---".
        assert_eq!(kramdown_slug("Yaa Gyasi - Home Going"), "yaa-gyasi---home-going");
    }

    #[test]
    fn slug_drops_punctuation() {
        assert_eq!(
            kramdown_slug("Cixin Liu, The Three-Body Problem"),
            "cixin-liu-the-three-body-problem"
        );
    }

    #[test]
    fn slug_keeps_digits() {
        assert_eq!(kramdown_slug("Batch 1"), "batch-1");
    }

    #[test]
    fn slug_strips_leading_non_letters() {
        assert_eq!(
            kramdown_slug("1/26 - The Underground Railroad"),
            "the-underground-railroad"
        );
    }

    #[test]
    fn slug_simple_word() {
        assert_eq!(kramdown_slug("Impressions"), "impressions");
    }

    #[test]
    fn slug_empty_is_section() {
        assert_eq!(kramdown_slug(""), "section");
    }

    #[test]
    fn slug_all_non_letters_is_section() {
        assert_eq!(kramdown_slug("123 456"), "section");
    }

    #[test]
    fn slug_drops_unicode() {
        // Non-ASCII letters are outside [A-Za-z0-9 -] and are dropped.
        assert_eq!(kramdown_slug("Café Society"), "caf-society");
    }

    #[test]
    fn esc_all_special_chars() {
        assert_eq!(
            esc(r#"a & b < c > d " e ' f"#),
            "a &amp; b &lt; c &gt; d &quot; e &#39; f"
        );
    }

    #[test]
    fn esc_plain_text_unchanged() {
        assert_eq!(esc("nothing to escape"), "nothing to escape");
    }

    #[test]
    fn esc_empty() {
        assert_eq!(esc(""), "");
    }

    #[test]
    fn esc_preserves_unicode() {
        assert_eq!(esc("café — naïve"), "café — naïve");
    }

    #[test]
    fn description_drops_leading_heading() {
        assert_eq!(
            description_from("<h2>Title</h2><p>Hello world</p>"),
            "Hello world"
        );
    }

    #[test]
    fn description_short_text_unchanged() {
        let out = description_from("<p>Just a short paragraph.</p>");
        assert_eq!(out, "Just a short paragraph.");
        assert!(!out.ends_with('…'));
    }

    #[test]
    fn description_truncates_long_text() {
        let long: String = std::iter::repeat('a').take(200).collect(); // > 155 chars
        let out = description_from(&format!("<p>{long}</p>"));
        assert!(out.ends_with('…'));
        // 152 head chars + the ellipsis = 153 chars.
        assert_eq!(out.chars().count(), 153);
    }

    #[test]
    fn description_at_boundary_155_not_truncated() {
        let text: String = std::iter::repeat('a').take(155).collect();
        let out = description_from(&format!("<p>{text}</p>"));
        assert_eq!(out.chars().count(), 155);
        assert!(!out.ends_with('…'));
    }

    #[test]
    fn description_collapses_whitespace() {
        assert_eq!(
            description_from("<p>a   b\n\tc</p>"),
            "a b c"
        );
    }

    #[test]
    fn description_no_leading_heading_keeps_text() {
        // A heading that isn't at the very start is not stripped.
        assert_eq!(
            description_from("<p>lead</p><h2>later</h2>"),
            "lead later"
        );
    }

    #[test]
    fn embed_shortcode_exact_html() {
        let out = expand_block_shortcodes(
            r#"{{< embed url="https://x.com/e" title="T" >}}"#,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            out,
            r#"<div class="video"><iframe width="560" height="315" src="https://x.com/e" title="T" allowfullscreen></iframe><a href="https://x.com/e">T</a></div>"#
        );
    }

    #[test]
    fn embed_shortcode_escapes_attrs() {
        let out = expand_block_shortcodes(
            r#"{{< embed url="https://x.com/?a=1&b=2" title="A & B" >}}"#,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(out.contains("https://x.com/?a=1&amp;b=2"));
        assert!(out.contains(">A &amp; B</a>"));
    }

    #[test]
    fn embed_shortcode_missing_title_empty() {
        let out = expand_block_shortcodes(
            r#"{{< embed url="https://x.com/e" >}}"#,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(out.contains(r#"title="" allowfullscreen"#));
    }

    #[test]
    fn no_shortcode_passes_through() {
        let text = "just some **markdown** text with no shortcodes";
        let out = expand_block_shortcodes(text, &Default::default(), &Default::default()).unwrap();
        assert_eq!(out, text);
    }

    #[test]
    fn shortcode_surrounding_text_preserved() {
        let out = expand_block_shortcodes(
            r#"before {{< embed url="u" title="t" >}} after"#,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(out.starts_with("before <div"));
        assert!(out.ends_with("</div> after"));
    }

    #[test]
    fn unterminated_shortcode_is_err() {
        let out = expand_block_shortcodes(
            r#"{{< embed url="u""#,
            &Default::default(),
            &Default::default(),
        );
        assert!(out.is_err());
    }

    #[test]
    fn unknown_shortcode_is_err() {
        let out = expand_block_shortcodes(
            r#"{{< bogus x="y" >}}"#,
            &Default::default(),
            &Default::default(),
        );
        assert!(out.is_err());
    }
}
