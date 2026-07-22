//! Site data model: posts, dates, and the image/video manifests.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A calendar date (time optional, defaults to midnight UTC).
#[derive(Clone, Copy)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
}

impl Date {
    /// Parse front-matter dates like `2015-10-26 05:36 UTC` or `2026-05-24`.
    pub fn parse(s: &str) -> Res<Date> {
        let s = s.trim();
        let mut parts = s.split_whitespace();
        let date = parts.next().ok_or("empty date")?;
        let mut d = date.split('-');
        let year: i32 = d.next().ok_or("no year")?.parse()?;
        let month: u32 = d.next().ok_or("no month")?.parse()?;
        let day: u32 = d.next().ok_or("no day")?.parse()?;
        let (mut hour, mut min) = (0, 0);
        if let Some(t) = parts.next() {
            let mut hm = t.split(':');
            hour = hm.next().unwrap_or("0").parse().unwrap_or(0);
            min = hm.next().unwrap_or("0").parse().unwrap_or(0);
        }
        Ok(Date { year, month, day, hour, min })
    }

    /// Sortable key (reverse-chronological ordering sorts on this descending).
    pub fn key(&self) -> (i32, u32, u32, u32, u32) {
        (self.year, self.month, self.day, self.hour, self.min)
    }

    fn mon(&self) -> &'static str {
        MONTHS[(self.month as usize - 1).min(11)]
    }

    /// strftime %F — 2015-10-26
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    /// strftime "%b %d %Y" — Oct 26 2015
    pub fn long(&self) -> String {
        format!("{} {:02} {}", self.mon(), self.day, self.year)
    }
    /// strftime %D — 10/26/15
    pub fn short(&self) -> String {
        format!("{:02}/{:02}/{:02}", self.month, self.day, self.year.rem_euclid(100))
    }
    /// strftime "%b %Y" — Oct 2015
    pub fn month_year(&self) -> String {
        format!("{} {}", self.mon(), self.year)
    }
    /// RFC 3339 for the Atom feed — 2015-10-26T05:36:00Z
    pub fn rfc3339(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:00Z",
            self.year, self.month, self.day, self.hour, self.min
        )
    }
    /// Blog permalink slug — `{slug}-{day}{month}{year}` (e.g. walkability-26102015).
    pub fn permalink(&self, slug: &str) -> String {
        format!("{slug}-{:02}{:02}{:04}", self.day, self.month, self.year)
    }
}

/// A rendered post. String fields are what templates consume directly.
#[derive(Serialize)]
pub struct Post {
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub url: String,
    pub tags: Vec<String>,
    pub body_html: String,
    pub description: String,
    pub edit_url: String,
    pub date_iso: String,
    pub date_long: String,
    pub date_short: String,
    pub date_month: String,
    pub date_rfc3339: String,
    #[serde(skip)]
    pub date_key: (i32, u32, u32, u32, u32),
}

/// A standalone page (e.g. About), rendered from site/source/pages/*.md.
#[derive(Serialize)]
pub struct Page {
    pub title: String,
    pub slug: String,
    pub url: String,
    pub body_html: String,
    pub description: String,
}

/// One entry in data/images.json.
#[derive(Serialize, Deserialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub widths: Vec<u32>,
    pub digest: String,
    /// Base64 ThumbHash for the blur-up placeholder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbhash: Option<String>,
    /// Camera name from EXIF, e.g. "Apple iPhone 15" (never GPS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    /// Capture settings, e.g. "26 mm · f/1.6 · 1/60 s · ISO 400".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<String>,
}

/// One entry in data/videos.json.
#[derive(Deserialize)]
pub struct VideoMeta {
    pub src: String,
    pub width: u32,
    pub height: u32,
    pub poster: String,
}

pub type Images = BTreeMap<String, ImageMeta>;
pub type Videos = BTreeMap<String, VideoMeta>;

pub fn load_images(path: &Path) -> Res<Images> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn load_videos(path: &Path) -> Res<Videos> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

/// Split a `---`-delimited YAML-ish front matter block from the body.
/// Returns (fields, body). Only simple `key: value` lines are supported.
pub fn split_front_matter(raw: &str) -> (BTreeMap<String, String>, String) {
    let mut fields = BTreeMap::new();
    let stripped = raw.strip_prefix("---\n").or_else(|| raw.strip_prefix("---\r\n"));
    let Some(rest) = stripped else {
        return (fields, raw.to_string());
    };
    // scan line by line tracking byte offset until the closing `---`
    let mut body_start = None;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed == "---" {
            body_start = Some(offset + line.len());
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            fields.insert(k.trim().to_string(), v.trim().to_string());
        }
        offset += line.len();
    }
    let body = match body_start {
        Some(b) => rest[b..].trim_start_matches(['\n', '\r']).to_string(),
        None => rest.to_string(),
    };
    (fields, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_datetime() {
        let d = Date::parse("2015-10-26 05:36 UTC").unwrap();
        assert_eq!((d.year, d.month, d.day, d.hour, d.min), (2015, 10, 26, 5, 36));
    }

    #[test]
    fn parse_date_only_defaults_midnight() {
        let d = Date::parse("2026-05-24").unwrap();
        assert_eq!((d.year, d.month, d.day, d.hour, d.min), (2026, 5, 24, 0, 0));
    }

    #[test]
    fn parse_trims_surrounding_whitespace() {
        let d = Date::parse("  2015-10-26 05:36 UTC  ").unwrap();
        assert_eq!((d.year, d.month, d.day, d.hour, d.min), (2015, 10, 26, 5, 36));
    }

    #[test]
    fn parse_empty_is_err() {
        assert!(Date::parse("").is_err());
    }

    #[test]
    fn parse_whitespace_only_is_err() {
        assert!(Date::parse("   ").is_err());
    }

    #[test]
    fn parse_non_numeric_year_is_err() {
        assert!(Date::parse("abcd-01-02").is_err());
    }

    #[test]
    fn parse_missing_month_is_err() {
        assert!(Date::parse("2015").is_err());
    }

    #[test]
    fn format_methods_full() {
        let d = Date::parse("2015-10-26 05:36 UTC").unwrap();
        assert_eq!(d.iso(), "2015-10-26");
        assert_eq!(d.long(), "Oct 26 2015");
        assert_eq!(d.short(), "10/26/15");
        assert_eq!(d.month_year(), "Oct 2015");
        assert_eq!(d.rfc3339(), "2015-10-26T05:36:00Z");
        assert_eq!(d.permalink("walkability"), "walkability-26102015");
    }

    #[test]
    fn format_methods_date_only() {
        let d = Date::parse("2026-05-24").unwrap();
        assert_eq!(d.iso(), "2026-05-24");
        assert_eq!(d.long(), "May 24 2026");
        assert_eq!(d.short(), "05/24/26");
        assert_eq!(d.month_year(), "May 2026");
        assert_eq!(d.rfc3339(), "2026-05-24T00:00:00Z");
        assert_eq!(d.permalink("sous-vide-yogurt"), "sous-vide-yogurt-24052026");
    }

    #[test]
    fn short_uses_last_two_digits_of_year() {
        // rem_euclid(100) keeps the year in 00..=99 for the %D format.
        let d = Date::parse("2007-01-09").unwrap();
        assert_eq!(d.short(), "01/09/07");
    }

    #[test]
    fn key_orders_chronologically() {
        let a = Date::parse("2015-10-26 05:36 UTC").unwrap();
        let b = Date::parse("2015-10-26 05:37 UTC").unwrap();
        assert!(a.key() < b.key());
    }

    #[test]
    fn front_matter_parsed() {
        let raw = "---\ntitle: Hello\ndate: 2020-01-02\ntags: a, b\n---\n\nbody text\n";
        let (fields, body) = split_front_matter(raw);
        assert_eq!(fields.get("title").map(String::as_str), Some("Hello"));
        assert_eq!(fields.get("date").map(String::as_str), Some("2020-01-02"));
        assert_eq!(fields.get("tags").map(String::as_str), Some("a, b"));
        assert!(body.starts_with("body text"));
    }

    #[test]
    fn front_matter_absent_returns_whole_body() {
        let (fields, body) = split_front_matter("just body");
        assert!(fields.is_empty());
        assert_eq!(body, "just body");
    }

    #[test]
    fn front_matter_values_are_trimmed() {
        let raw = "---\ntitle:    Spaced Out   \n---\nbody\n";
        let (fields, _) = split_front_matter(raw);
        assert_eq!(fields.get("title").map(String::as_str), Some("Spaced Out"));
    }

    #[test]
    fn front_matter_crlf_delimiters() {
        let raw = "---\r\ntitle: Hi\r\n---\r\nbody\r\n";
        let (fields, body) = split_front_matter(raw);
        assert_eq!(fields.get("title").map(String::as_str), Some("Hi"));
        assert!(body.starts_with("body"));
    }

    #[test]
    fn front_matter_value_with_colon_keeps_remainder() {
        // split_once(':') means only the first colon delimits key/value.
        let raw = "---\nurl: https://example.com\n---\nbody\n";
        let (fields, _) = split_front_matter(raw);
        assert_eq!(fields.get("url").map(String::as_str), Some("https://example.com"));
    }
}
