use std::{path::Path, process::Command};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    errors::app_error::{AppError, AppResult},
    platform::contracts::ParsedBookmarkRecord,
};

pub async fn parse_safari_bookmarks(path: &Path) -> AppResult<Vec<ParsedBookmarkRecord>> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || parse_safari_bookmarks_blocking(&path))
        .await
        .map_err(|error| AppError::Invalid(format!("failed to load Safari bookmarks: {error}")))?
}

fn parse_safari_bookmarks_blocking(path: &Path) -> AppResult<Vec<ParsedBookmarkRecord>> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Invalid(format!(
            "Safari bookmark access failed. macOS may require permission to read {} ({stderr})",
            path.display()
        )));
    }

    let value = serde_json::from_slice::<Value>(&output.stdout)?;
    let mut parsed = Vec::new();

    if let Some(children) = value.get("Children").and_then(Value::as_array) {
        for child in children {
            collect_safari_bookmarks(child, &[], &mut parsed);
        }
    }

    Ok(parsed)
}

fn collect_safari_bookmarks(
    node: &Value,
    breadcrumbs: &[String],
    parsed: &mut Vec<ParsedBookmarkRecord>,
) {
    let node_type = node
        .get("WebBookmarkType")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if node_type == "WebBookmarkTypeLeaf" {
        let Some(url) = node.get("URLString").and_then(Value::as_str).map(str::trim) else {
            return;
        };

        if url.is_empty() {
            return;
        }

        let title = safari_leaf_title(node).unwrap_or(url).trim().to_string();
        parsed.push(ParsedBookmarkRecord {
            external_id: node
                .get("WebBookmarkUUID")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(url)
                .to_string(),
            title: if title.is_empty() {
                url.to_string()
            } else {
                title
            },
            url: url.to_string(),
            folder_path: if breadcrumbs.is_empty() {
                None
            } else {
                Some(breadcrumbs.join(" / "))
            },
            created_at: safari_created_at(node),
        });
        return;
    }

    let mut next_breadcrumbs = breadcrumbs.to_vec();
    if let Some(title) = safari_folder_title(node) {
        if next_breadcrumbs.last().is_none_or(|last| last != &title) {
            next_breadcrumbs.push(title);
        }
    }

    if let Some(children) = node.get("Children").and_then(Value::as_array) {
        for child in children {
            collect_safari_bookmarks(child, &next_breadcrumbs, parsed);
        }
    }
}

fn safari_leaf_title(node: &Value) -> Option<&str> {
    node.get("URIDictionary")
        .and_then(|dict| dict.get("title"))
        .and_then(Value::as_str)
        .or_else(|| node.get("Title").and_then(Value::as_str))
}

fn safari_folder_title(node: &Value) -> Option<String> {
    let raw = node.get("Title").and_then(Value::as_str)?.trim();
    if raw.is_empty() {
        return None;
    }

    let normalized = match raw {
        "BookmarksBar" => "Bookmarks Bar",
        "BookmarksMenu" => "Bookmarks Menu",
        "com.apple.ReadingList" => "Reading List",
        other => other,
    };

    Some(normalized.to_string())
}

fn safari_created_at(node: &Value) -> String {
    let candidates = [
        node.get("DateAdded"),
        node.get("ReadingList").and_then(|value| value.get("DateAdded")),
        node.get("ReadingList").and_then(|value| value.get("Date Added")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Some(raw) = candidate.as_str() {
            if let Ok(datetime) = DateTime::parse_from_rfc3339(raw) {
                return datetime.with_timezone(&Utc).to_rfc3339();
            }
        }
    }

    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::collect_safari_bookmarks;

    #[test]
    fn parses_safari_leaf_bookmarks_with_folder_labels() {
        let root = json!({
          "Title": "BookmarksBar",
          "WebBookmarkType": "WebBookmarkTypeList",
          "Children": [
            {
              "Title": "Research",
              "WebBookmarkType": "WebBookmarkTypeList",
              "Children": [
                {
                  "WebBookmarkType": "WebBookmarkTypeLeaf",
                  "WebBookmarkUUID": "uuid-1",
                  "URLString": "https://developer.apple.com",
                  "URIDictionary": { "title": "Apple Developer" },
                  "ReadingList": { "DateAdded": "2026-04-11T12:00:00Z" }
                }
              ]
            }
          ]
        });

        let mut parsed = Vec::new();
        collect_safari_bookmarks(&root, &[], &mut parsed);

        let first = parsed.first().expect("bookmark present");
        assert_eq!(first.external_id, "uuid-1");
        assert_eq!(first.title, "Apple Developer");
        assert_eq!(first.folder_path.as_deref(), Some("Bookmarks Bar / Research"));
        assert_eq!(first.url, "https://developer.apple.com");
        assert_eq!(first.created_at, "2026-04-11T12:00:00+00:00");
    }
}
