//! Site identity, loaded from `site/config.yml`.
//! Flat `key: value` lines only — the same YAML subset as post front matter.

use crate::model::Res;
use std::collections::BTreeMap;
use std::path::Path;

pub struct Config {
    pub name: String,
    pub default_title: String,
    pub description: String,
    pub author: String,
    pub base_url: String,
    pub edit_base: String,
    pub bucket: String,
}

pub fn load(root: &Path) -> Res<Config> {
    let path = root.join("site/config.yml");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut fields = parse(&raw);
    let mut get = |k: &str| -> Res<String> {
        fields
            .remove(k)
            .ok_or_else(|| format!("{}: missing `{k}`", path.display()).into())
    };
    Ok(Config {
        name: get("name")?,
        default_title: get("default_title")?,
        description: get("description")?,
        author: get("author")?,
        base_url: get("base_url")?,
        edit_base: get("edit_base")?,
        bucket: get("bucket")?,
    })
}

fn parse(raw: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            fields.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_blanks() {
        let f = parse("# site\n\nname: x\n");
        assert_eq!(f.get("name").map(String::as_str), Some("x"));
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn parse_unquotes_values_with_colons() {
        let f = parse("description: \"a: b, c\"\n");
        assert_eq!(f.get("description").map(String::as_str), Some("a: b, c"));
    }
}
