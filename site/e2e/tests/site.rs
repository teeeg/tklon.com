//! Link checks for the built site.
//!
//! They run against whatever is in build/, so build first:
//!
//!   cargo run -- build && cargo test -p e2e
//!
//! or `make e2e`, which does both.

use e2e::*;
use std::collections::BTreeSet;

#[test]
fn internal_links_all_resolve() {
    let broken = crawl().broken;
    assert!(
        broken.is_empty(),
        "internal links with no file behind them in build/: {broken:?}"
    );
}

#[test]
fn external_links_are_reachable() {
    let external = crawl().external;
    assert!(
        !external.is_empty(),
        "expected at least one external link on the site"
    );

    let client = http_client();
    let mut dead = Vec::new();
    let mut inconclusive = Vec::new();
    for url in &external {
        match client.get(url).call() {
            Ok(r) if r.status().as_u16() < 400 => {}
            Ok(r) if matches!(r.status().as_u16(), 404 | 410) => {
                dead.push((url.clone(), r.status().as_u16()))
            }
            Ok(r) => inconclusive.push((url.clone(), r.status().as_u16())),
            Err(e) => {
                eprintln!("{url}: {e}");
                inconclusive.push((url.clone(), 0));
            }
        }
    }
    // Bot walls, rate limits and timeouts are not evidence a link is dead;
    // only an unambiguous 404/410 fails the test.
    if !inconclusive.is_empty() {
        eprintln!("inconclusive (not treated as failures): {inconclusive:?}");
    }
    assert!(dead.is_empty(), "external links returned 404/410: {dead:?}");
}

/// The tag filter is wired across three artifacts that only meet in the
/// browser: chip and article markup on /tags/, per-tag dimming rules generated
/// into the stylesheet, and the inline script that syncs the URL to
/// `<html data-tags>`. Nothing checks their agreement at build time, so assert
/// here that all three name exactly the same set of tags.
#[test]
fn tag_filter_contract_holds() {
    let page = std::fs::read_to_string(build_dir().join("tags/index.html")).unwrap();

    // A chip without a matching article class is a filter that dims
    // everything; an article tag without a chip is unfilterable.
    let chips: BTreeSet<String> = attr_values(&page, "data-tag").into_iter().collect();
    assert!(!chips.is_empty(), "no tag chips on /tags/");
    let article_tags: BTreeSet<String> = attr_values(&page, "class")
        .iter()
        .map(|v| v.split_whitespace().collect::<Vec<_>>())
        .filter(|tokens| tokens.contains(&"Article"))
        .flat_map(|tokens| {
            tokens
                .into_iter()
                .filter(|t| *t != "Article")
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        chips, article_tags,
        "tag chips and article tag classes disagree"
    );

    // A tag missing from the CSS has no dimming or highlight rule, so
    // selecting it would visibly do nothing.
    let css: String = attr_values(&page, "href")
        .into_iter()
        .filter(|h| h.ends_with(".css"))
        .map(|h| {
            std::fs::read_to_string(build_dir().join(h.trim_start_matches('/')))
                .expect("stylesheet linked from /tags/ is missing from the build")
        })
        .collect();
    let css_tags: BTreeSet<String> = css
        .split("data-tags~=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next().map(str::to_string))
        .collect();
    assert_eq!(chips, css_tags, "tag chips and per-tag CSS rules disagree");

    // The script's two hooks into the page: the attribute the CSS keys on and
    // the live region it announces counts into.
    assert!(
        page.contains("dataset.tags"),
        "tag filter script is missing or no longer syncs data-tags"
    );
    assert!(
        attr_values(&page, "id").iter().any(|id| id == "tagStatus"),
        "no #tagStatus live region for the script to announce into"
    );
}
