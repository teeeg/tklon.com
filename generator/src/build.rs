//! `tklon build` — render the whole site into an output directory.

use crate::config::{self, Config};
use crate::markdown::{description_from, esc, render_body};
use crate::model::{
    load_images, load_videos, split_front_matter, Date, Images, Page, Post, Res, Videos,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};

const PER_PAGE: usize = 5;

pub fn build(root: &Path, out: &Path) -> Res<()> {
    let site = root.join("site");
    let source = site.join("source");

    let images = load_images(&site.join("data/images.json"))?;
    let videos = load_videos(&site.join("data/videos.json"))?;
    let cfg = config::load(root)?;
    let posts = load_posts(root, &source.join("posts"), &images, &videos, &cfg)?;
    let standalone_pages = load_pages(&source.join("pages"), &images, &videos)?;
    let mut all_tags: Vec<String> = posts.iter().flat_map(|p| p.tags.clone()).collect();
    all_tags.sort();
    all_tags.dedup();

    // Stage the whole build in a sibling directory and swap it in at the end, so
    // the dev server never serves from a half-written tree.
    let staging = out.with_file_name(format!(
        ".{}.staging",
        out.file_name().and_then(|n| n.to_str()).unwrap_or("build")
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let published = out;
    let out = staging.as_path();

    // ---- assets (content-hashed) ----
    let font_bytes = fs::read(source.join("fonts/hanken-grotesk.woff2"))?;
    let font_href = format!("/fonts/hanken-grotesk-{}.woff2", fnv(&font_bytes));
    write_out(out, &font_href, &font_bytes)?;

    // Inlined into every page rather than linked: the whole stylesheet is a few
    // KB, and an external <link> is render-blocking, so it costs a round trip
    // before first paint.
    let mut css = grass::from_path(
        source.join("stylesheets/all.css.scss"),
        &grass::Options::default().style(grass::OutputStyle::Compressed),
    )?
    .replace("/fonts/hanken-grotesk.woff2", &font_href);
    // Per-tag filter rules, driven by :root[data-tags] (set before paint by the
    // tags page's inline script) so a deep-linked tag filters with zero flicker.
    for tag in &all_tags {
        css.push_str(&format!(
            ":root[data-tags~=\"{t}\"] .Article:not(.{t}){{opacity:.15}}\
:root[data-tags~=\"{t}\"] .Tag[data-tag=\"{t}\"]{{background:var(--accent);color:var(--bg)}}",
            t = tag
        ));
    }
    let tera = make_tera(&site)?;

    let mut base = Context::new();
    base.insert("font_href", &font_href);
    base.insert("css", &css);

    // ---- posts ----
    for p in &posts {
        let mut ctx = base.clone();
        ctx.insert("display_title", &format!("{} — {}", p.title, cfg.name));
        ctx.insert("og_title", &p.title);
        ctx.insert("og_type", "article");
        ctx.insert("description", &p.description);
        ctx.insert("canonical", &format!("{}{}", cfg.base_url, p.url));
        ctx.insert(
            "page_classes",
            &format!("posts posts_{pl} posts_{pl}_index", pl = p.permalink),
        );
        ctx.insert("post", p);
        write_page(out, &p.url, &tera.render("post.html", &ctx)?)?;
    }

    // ---- paginated index ----
    let pages: Vec<&[Post]> = posts.chunks(PER_PAGE).collect();
    let total = pages.len();
    for (i, chunk) in pages.iter().enumerate() {
        let num = i + 1;
        let url = if num == 1 {
            "/".to_string()
        } else {
            format!("/page/{num}/")
        };
        let page_classes = if num == 1 {
            "index".to_string()
        } else {
            format!("page page_{num} page_{num}_index")
        };
        let mut ctx = base.clone();
        ctx.insert("display_title", &cfg.default_title);
        ctx.insert("og_title", &cfg.name);
        ctx.insert("og_type", "website");
        ctx.insert("description", &cfg.description);
        ctx.insert("canonical", &format!("{}{url}", cfg.base_url));
        ctx.insert("page_classes", &page_classes);
        ctx.insert("posts", chunk);
        if num == 2 {
            ctx.insert("prev_url", "/");
        } else if num > 2 {
            ctx.insert("prev_url", &format!("/page/{}/", num - 1));
        }
        if num < total {
            ctx.insert("next_url", &format!("/page/{}/", num + 1));
        }
        write_page(out, &url, &tera.render("index.html", &ctx)?)?;
    }

    // ---- tags filter page (dynamic; deep-linked tag filters via :root[data-tags]) ----
    {
        let mut ctx = base.clone();
        ctx.insert("display_title", &format!("posts — {}", cfg.name));
        ctx.insert("og_title", "posts");
        ctx.insert("og_type", "website");
        ctx.insert("description", &cfg.description);
        ctx.insert("canonical", &format!("{}/tags/", cfg.base_url));
        ctx.insert("page_classes", "tags tags_index");
        ctx.insert("posts", &posts);
        ctx.insert("all_tags", &all_tags);
        write_page(out, "/tags/", &tera.render("tags.html", &ctx)?)?;
    }

    // ---- standalone pages (About, …) ----
    for pg in &standalone_pages {
        let mut ctx = base.clone();
        ctx.insert("display_title", &format!("{} — {}", pg.title, cfg.name));
        ctx.insert("og_title", &pg.title);
        ctx.insert("og_type", "website");
        ctx.insert("description", &pg.description);
        ctx.insert("canonical", &format!("{}{}", cfg.base_url, pg.url));
        ctx.insert("page_classes", &format!("page-{}", pg.slug));
        ctx.insert("page", pg);
        write_page(out, &pg.url, &tera.render("page.html", &ctx)?)?;
    }

    // ---- 404 (no directory index) ----
    {
        let mut ctx = base.clone();
        ctx.insert("display_title", &cfg.default_title);
        ctx.insert("og_title", &cfg.name);
        ctx.insert("og_type", "website");
        ctx.insert("description", &cfg.description);
        ctx.insert("canonical", &format!("{}/404.html", cfg.base_url));
        ctx.insert("page_classes", "x404");
        fs::write(
            out.join("404.html"),
            minify(&tera.render("notfound.html", &ctx)?),
        )?;
    }

    // ---- feed + media ----
    fs::write(out.join("feed.xml"), render_feed(&posts, &cfg))?;
    copy_dir(&source.join("images"), &out.join("images"))?;

    if published.exists() {
        fs::remove_dir_all(published)?;
    }
    fs::rename(&staging, published)?;

    println!(
        "built {} posts + {} index pages → {}",
        posts.len(),
        total,
        published.display()
    );
    Ok(())
}

/// Normalize to `[a-z0-9-]+`. Slugs and tags end up in URLs, class names, CSS
/// attribute selectors and `view-transition-name` idents, so they are made safe
/// once here rather than escaped differently at each use site.
fn ident(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    // a CSS identifier may not start with a digit
    match out.chars().next() {
        Some(c) if c.is_ascii_digit() => format!("t-{out}"),
        _ => out.to_string(),
    }
}

// ---- post loading ---------------------------------------------------------

fn load_posts(
    root: &Path,
    dir: &Path,
    images: &Images,
    videos: &Videos,
    cfg: &Config,
) -> Res<Vec<Post>> {
    let mut files = Vec::new();
    collect_post_files(dir, &mut files)?;
    let mut posts = Vec::new();
    for path in files {
        posts.push(load_post(root, &path, images, videos, cfg)?);
    }
    // reverse-chronological; slug tiebreak keeps output deterministic
    posts.sort_by(|a, b| b.date_key.cmp(&a.date_key).then(a.slug.cmp(&b.slug)));
    Ok(posts)
}

/// Load standalone pages from site/source/pages/*.md (skips if the dir is absent).
fn load_pages(dir: &Path, images: &Images, videos: &Videos) -> Res<Vec<Page>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut pages = Vec::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    entries.sort();
    for path in entries {
        let raw = fs::read_to_string(&path)?;
        let (fm, body) = split_front_matter(&raw);
        let slug = path
            .file_stem()
            .and_then(|n| n.to_str())
            .ok_or("bad page filename")?
            .to_string();
        // A standalone page is alone in its document, so ids can't collide.
        let body_html = render_body(&body, images, videos, true)?;
        let description = description_from(&body_html);
        pages.push(Page {
            title: fm.get("title").cloned().unwrap_or_else(|| slug.clone()),
            url: format!("/{slug}/"),
            slug,
            body_html,
            description,
        });
    }
    Ok(pages)
}

fn collect_post_files(dir: &Path, acc: &mut Vec<PathBuf>) -> Res<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_post_files(&path, acc)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            acc.push(path);
        }
    }
    Ok(())
}

fn load_post(
    root: &Path,
    path: &Path,
    images: &Images,
    videos: &Videos,
    cfg: &Config,
) -> Res<Post> {
    let raw = fs::read_to_string(path)?;
    let (fm, body) = split_front_matter(&raw);
    let title = fm.get("title").cloned().unwrap_or_default();
    let date = Date::parse(
        fm.get("date")
            .ok_or_else(|| format!("{}: missing date", path.display()))?,
    )?;
    let mut tags: Vec<String> = fm
        .get("tags")
        .map(|t| {
            t.split(',')
                .map(ident)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut seen = HashSet::new();
    tags.retain(|t| seen.insert(t.clone()));

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad post filename")?;
    let slug = ident(filename.trim_end_matches(".md"));
    let permalink = date.permalink(&slug);
    let url = format!("/posts/{permalink}/");

    let body_html = render_body(&body, images, videos, true)?;
    let body_html_index = render_body(&body, images, videos, false)?;
    let description = description_from(&body_html);

    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
    let edit_url = format!("{}/{rel}", cfg.edit_base);

    Ok(Post {
        title,
        slug,
        permalink,
        url,
        tags,
        body_html,
        body_html_index,
        description,
        edit_url,
        date_iso: date.iso(),
        date_long: date.long(),
        date_short: date.short(),
        date_month: date.month_year(),
        date_rfc3339: date.rfc3339(),
        date_key: date.key(),
    })
}

// ---- templates ------------------------------------------------------------

fn make_tera(site: &Path) -> Res<Tera> {
    let glob = site.join("templates/*.html");
    let mut tera = Tera::new(glob.to_str().ok_or("bad templates path")?)?;
    tera.autoescape_on(vec![]); // escape explicitly with `| escape`, matching ERB `h()`
    Ok(tera)
}

// ---- Atom feed ------------------------------------------------------------

fn render_feed(posts: &[Post], cfg: &Config) -> String {
    let updated = posts
        .first()
        .map(|p| p.date_rfc3339.clone())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string());
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    s.push_str(&format!("  <title>{}</title>\n", esc(&cfg.name)));
    s.push_str(&format!(
        "  <subtitle>{}</subtitle>\n",
        esc(&cfg.description)
    ));
    s.push_str(&format!(
        "  <link href=\"{}/feed.xml\" rel=\"self\"/>\n",
        cfg.base_url
    ));
    s.push_str(&format!("  <link href=\"{}/\"/>\n", cfg.base_url));
    s.push_str(&format!("  <updated>{updated}</updated>\n"));
    s.push_str(&format!("  <id>{}/</id>\n", cfg.base_url));
    s.push_str(&format!(
        "  <author><name>{}</name></author>\n",
        esc(&cfg.author)
    ));
    for p in posts {
        s.push_str("  <entry>\n");
        s.push_str(&format!("    <title>{}</title>\n", esc(&p.title)));
        s.push_str(&format!("    <link href=\"{}{}\"/>\n", cfg.base_url, p.url));
        s.push_str(&format!("    <id>{}{}</id>\n", cfg.base_url, p.url));
        s.push_str(&format!("    <published>{}</published>\n", p.date_rfc3339));
        s.push_str(&format!("    <updated>{}</updated>\n", p.date_rfc3339));
        for t in &p.tags {
            s.push_str(&format!("    <category term=\"{}\"/>\n", esc(t)));
        }
        s.push_str(&format!(
            "    <content type=\"html\">{}</content>\n",
            esc(&p.body_html)
        ));
        s.push_str("  </entry>\n");
    }
    s.push_str("</feed>\n");
    s
}

// ---- fs helpers -----------------------------------------------------------

/// Write a page whose url ends in `/` to `<out>/<url>/index.html`.
fn write_page(out: &Path, url: &str, html: &str) -> Res<()> {
    let rel = url.trim_start_matches('/');
    let dir = if rel.is_empty() {
        out.to_path_buf()
    } else {
        out.join(rel)
    };
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("index.html"), minify(html))?;
    Ok(())
}

/// Collapse whitespace + drop comments, but keep tag structure (conservative).
fn minify(html: &str) -> Vec<u8> {
    let cfg = minify_html::Cfg {
        keep_closing_tags: true,
        keep_html_and_head_opening_tags: true,
        ..Default::default()
    };
    minify_html::minify(html.as_bytes(), &cfg)
}

/// Write raw bytes to `<out>/<url>` (url is an absolute site path).
fn write_out(out: &Path, url: &str, bytes: &[u8]) -> Res<()> {
    let path = out.join(url.trim_start_matches('/'));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Res<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// FNV-1a, low 32 bits as 8 hex — a zero-dependency content hash for assets.
fn fnv(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `ident` feeds permalinks and URLs, so a change here silently rewrites
    // every post's address. These pin the existing slugs in particular.
    #[test]
    fn ident_leaves_wellformed_slugs_untouched() {
        for s in [
            "sous-vide-yogurt",
            "may-reads",
            "tio-mike",
            "speedbox-bosch",
            "cooking",
        ] {
            assert_eq!(ident(s), s, "{s} must survive unchanged or its URL moves");
        }
    }

    #[test]
    fn ident_lowercases_and_joins_words() {
        assert_eq!(ident("Home Cooking"), "home-cooking");
        assert_eq!(ident("Sous  Vide"), "sous-vide");
    }

    #[test]
    fn ident_strips_css_and_html_metacharacters() {
        // The cases that made the raw-interpolation bugs reachable: a quote
        // broke out of an attribute, a `;` injected a CSS declaration.
        assert_eq!(ident("a; background:red"), "a-background-red");
        assert_eq!(
            ident("q\" onmouseover=alert(1) x=\""),
            "q-onmouseover-alert-1-x"
        );
        assert_eq!(ident("book/film"), "book-film");
    }

    #[test]
    fn ident_never_starts_with_a_digit() {
        // A CSS identifier may not begin with a digit, so these get a prefix.
        assert_eq!(ident("2026 reads"), "t-2026-reads");
        assert_eq!(ident("35mm"), "t-35mm");
    }

    #[test]
    fn ident_trims_separators_at_both_ends() {
        assert_eq!(ident("  spaced  "), "spaced");
        assert_eq!(ident("--dashes--"), "dashes");
        assert_eq!(ident("!!!"), "");
    }
}
