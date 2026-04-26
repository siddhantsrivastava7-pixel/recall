use chrono::{NaiveDate, Utc};
use serde::Deserialize;

use crate::{
    errors::app_error::{AppError, AppResult},
    platform::contracts::ParsedBookmarkRecord,
};

#[derive(Debug, Deserialize)]
struct BookmarkFile {
    roots: std::collections::HashMap<String, BookmarkNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct BookmarkNode {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    node_type: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    date_added: Option<String>,
    #[serde(default)]
    children: Vec<BookmarkNode>,
}

pub fn parse_chromium_bookmark_bytes(bytes: &[u8]) -> AppResult<Vec<ParsedBookmarkRecord>> {
    let bookmark_file = serde_json::from_slice::<BookmarkFile>(bytes)?;
    parse_chromium_bookmark_tree(&bookmark_file)
}

fn parse_chromium_bookmark_tree(bookmark_file: &BookmarkFile) -> AppResult<Vec<ParsedBookmarkRecord>> {
    let mut parsed = Vec::new();

    for (root_key, root_node) in &bookmark_file.roots {
        let root_label = root_label(root_key, &root_node.name);
        collect_bookmarks(root_node, &[root_label], &mut parsed)?;
    }

    Ok(parsed)
}

fn collect_bookmarks(
    node: &BookmarkNode,
    breadcrumbs: &[String],
    parsed: &mut Vec<ParsedBookmarkRecord>,
) -> AppResult<()> {
    if node.node_type == "url" {
        let url = node
            .url
            .clone()
            .ok_or_else(|| AppError::Invalid("Bookmark entry is missing a URL.".into()))?;

        parsed.push(ParsedBookmarkRecord {
            external_id: if node.id.trim().is_empty() {
                url.clone()
            } else {
                node.id.clone()
            },
            title: if node.name.trim().is_empty() {
                url.clone()
            } else {
                node.name.trim().to_string()
            },
            url,
            folder_path: if breadcrumbs.is_empty() {
                None
            } else {
                Some(breadcrumbs.join(" / "))
            },
            created_at: parse_chromium_bookmark_timestamp(node.date_added.as_deref()),
        });

        return Ok(());
    }

    let mut next_breadcrumbs = breadcrumbs.to_vec();
    if !node.name.trim().is_empty()
        && next_breadcrumbs
            .last()
            .is_none_or(|last| last != node.name.trim())
    {
        next_breadcrumbs.push(node.name.trim().to_string());
    }

    for child in &node.children {
        collect_bookmarks(child, &next_breadcrumbs, parsed)?;
    }

    Ok(())
}

fn root_label(root_key: &str, fallback_name: &str) -> String {
    if !fallback_name.trim().is_empty() {
        return fallback_name.trim().to_string();
    }

    match root_key {
        "bookmark_bar" => "Bookmarks Bar".into(),
        "other" => "Other Bookmarks".into(),
        "synced" => "Synced".into(),
        "mobile" => "Mobile Bookmarks".into(),
        _ => "Bookmarks".into(),
    }
}

pub fn parse_chromium_bookmark_timestamp(value: Option<&str>) -> String {
    let Some(raw) = value else {
        return Utc::now().to_rfc3339();
    };

    let Ok(microseconds) = raw.parse::<i64>() else {
        return Utc::now().to_rfc3339();
    };

    let Some(base) = NaiveDate::from_ymd_opt(1601, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0))
    else {
        return Utc::now().to_rfc3339();
    };

    let Some(datetime) = base.checked_add_signed(chrono::TimeDelta::microseconds(microseconds))
    else {
        return Utc::now().to_rfc3339();
    };

    chrono::DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc).to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::parse_chromium_bookmark_bytes;

    #[test]
    fn parses_chromium_bookmarks_with_folders() {
        let json = r#"{
          "roots": {
            "bookmark_bar": {
              "name": "",
              "type": "folder",
              "children": [
                {
                  "name": "Research",
                  "type": "folder",
                  "children": [
                    {
                      "id": "42",
                      "name": "OpenAI Pricing",
                      "type": "url",
                      "url": "https://openai.com/pricing",
                      "date_added": "13344473600000000"
                    }
                  ]
                }
              ]
            }
          }
        }"#;

        let parsed = parse_chromium_bookmark_bytes(json.as_bytes()).expect("should parse");
        let first = parsed.first().expect("bookmark present");

        assert_eq!(first.external_id, "42");
        assert_eq!(first.title, "OpenAI Pricing");
        assert_eq!(first.url, "https://openai.com/pricing");
        assert_eq!(first.folder_path.as_deref(), Some("Bookmarks Bar / Research"));
    }
}
