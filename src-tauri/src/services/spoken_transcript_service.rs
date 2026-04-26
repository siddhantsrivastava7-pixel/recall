use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use sysinfo::System;

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{AppContextSnapshot, Memory, MemoryInput, MemorySourceType},
};

const SPOKEN_SOURCE_APP: &str = "spoken";
const SPOKEN_EXTERNAL_ID_PREFIX: &str = "spoken-daily:";
const TRANSCRIPT_SECTION_MARKER: &str = "\n\nTranscript\n\n";

const TOPIC_STOPWORDS: &[&str] = &[
    "about", "after", "also", "and", "any", "app", "because", "been", "before", "being",
    "but", "could", "did", "does", "doing", "for", "from", "have", "into", "just", "know",
    "like", "look", "made", "make", "maybe", "need", "onto", "our", "out", "really", "said",
    "same", "should", "some", "something", "still", "than", "that", "their", "them", "then",
    "there", "these", "they", "this", "today", "transcript", "user", "using", "want", "what",
    "when", "which", "while", "with", "work", "would", "yeah", "your",
];

#[derive(Clone)]
pub struct SpokenTranscriptService {
    repository: SharedMemoryRepository,
}

impl SpokenTranscriptService {
    pub fn new(repository: SharedMemoryRepository) -> Self {
        Self { repository }
    }

    pub async fn capture_clipboard_snippet(
        &self,
        content: String,
        context: &AppContextSnapshot,
    ) -> AppResult<Memory> {
        let snippet = normalize_body_text(&content);
        if snippet.is_empty() {
            return Err(AppError::Invalid(
                "Spoken transcript snippet cannot be empty.".into(),
            ));
        }

        let now_utc = Utc::now();
        let now_local = now_utc.with_timezone(&Local);
        let day_key = now_local.format("%Y-%m-%d").to_string();
        let external_id = format!("{SPOKEN_EXTERNAL_ID_PREFIX}{day_key}");
        let title = format!("Spoken transcript · {}", now_local.format("%b %d"));
        let source_window = normalize_context_label(context.source_window.as_deref())
            .or_else(|| normalize_context_label(context.source_app.as_deref()))
            .unwrap_or_else(|| "Spoken".to_string());
        let entry = build_entry_block(&now_local, &source_window, &snippet);

        if let Some(existing) = self
            .repository
            .find_by_external_source(SPOKEN_SOURCE_APP, &external_id)
            .await?
        {
            let body = extract_transcript_body(&existing.content);
            let combined_body = append_entry_block(&body, &entry);
            let content = build_daily_document(
                &combined_body,
                &title,
                existing.created_at.as_str(),
                now_utc.to_rfc3339().as_str(),
            );

            return self
                .repository
                .update(
                    &existing.id,
                    MemoryInput {
                        source_type: Some(MemorySourceType::Manual),
                        title: Some(title),
                        content,
                        note: existing.note.clone(),
                        project_id: existing.project_id.clone(),
                        url: None,
                        external_id: Some(external_id),
                        folder_path: None,
                        source_app: Some(SPOKEN_SOURCE_APP.to_string()),
                        source_window: Some(source_window),
                        created_at: Some(existing.created_at),
                        updated_at: Some(now_utc.to_rfc3339()),
                    },
                )
                .await;
        }

        let content = build_daily_document(
            &entry,
            &title,
            now_utc.to_rfc3339().as_str(),
            now_utc.to_rfc3339().as_str(),
        );

        self.repository
            .create(MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: Some(title),
                content,
                note: None,
                project_id: None,
                url: None,
                external_id: Some(external_id),
                folder_path: None,
                source_app: Some(SPOKEN_SOURCE_APP.to_string()),
                source_window: Some(source_window),
                created_at: Some(now_utc.to_rfc3339()),
                updated_at: Some(now_utc.to_rfc3339()),
            })
            .await
    }
}

pub fn is_spoken_context(context: &AppContextSnapshot) -> bool {
    [context.source_app.as_deref(), context.source_window.as_deref()]
        .into_iter()
        .flatten()
        .map(|value| value.to_ascii_lowercase())
        .any(|value| value.contains("spoken"))
}

/// Detects whether a Spoken process is currently running on this machine.
///
/// `is_spoken_context` only catches the case where Spoken happens to be the
/// frontmost app at the moment the clipboard fires — but Spoken pastes its
/// transcripts into the *destination* app (Notes, Chrome, Slack, …), so the
/// foreground app is never Spoken in practice. Checking process presence is
/// what actually fires reliably during a dictation session.
///
/// The result is cached for ~1.5s to avoid scanning the process table on every
/// clipboard tick (the clipboard watcher polls roughly every 900ms).
pub fn is_spoken_running() -> bool {
    static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    const TTL: Duration = Duration::from_millis(1500);

    if let Ok(guard) = CACHE.lock() {
        if let Some((stamped_at, value)) = *guard {
            if stamped_at.elapsed() < TTL {
                return value;
            }
        }
    }

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let running = system.processes().values().any(|process| {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        name.contains("spoken")
    });

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), running));
    }

    running
}

/// Classifier: does this clipboard content look like spoken-language text we
/// should fold into the daily transcript? Returns false for URLs, file paths,
/// code-shaped content, tabular data, very long content — those should remain
/// independent memories so the bookmark intelligence + link enrichment paths
/// still get a chance at them.
pub fn looks_like_spoken_text(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.len() > 4000 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();

    // URLs and protocol prefixes — these belong to link enrichment.
    if ["http://", "https://", "www.", "ftp://", "file://"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return false;
    }

    // File-path shapes (POSIX absolute, home-relative, multiple backslashes).
    if trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.matches('\\').count() >= 2
    {
        return false;
    }
    // Windows drive paths like `C:\…`.
    if trimmed.len() >= 3 {
        let bytes = trimmed.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
            return false;
        }
    }

    // Code fences or 3+ consecutive lines that look like code structure.
    if trimmed.contains("```") {
        return false;
    }
    let mut code_run = 0usize;
    for line in trimmed.lines() {
        let leading = line.trim_start();
        let starts_code_glyph = leading.starts_with('{')
            || leading.starts_with('}')
            || leading.starts_with('(')
            || leading.starts_with('<')
            || leading.starts_with("//")
            || leading.starts_with("/*")
            || leading.starts_with("import ")
            || leading.starts_with("function ")
            || leading.starts_with("const ")
            || leading.starts_with("def ");
        if starts_code_glyph {
            code_run += 1;
            if code_run >= 3 {
                return false;
            }
        } else {
            code_run = 0;
        }
    }

    // Tabular paste — multiple tabs strongly suggests spreadsheet content.
    if trimmed.matches('\t').count() > 1 {
        return false;
    }

    // Spoken text always has at least one space (rules out single-token IDs).
    if !trimmed.contains(' ') {
        return false;
    }

    // Letter-to-other-character ratio — natural speech is letter-heavy.
    let letters = trimmed.chars().filter(|c| c.is_alphabetic()).count();
    let other = trimmed
        .chars()
        .filter(|c| !c.is_alphabetic() && !c.is_whitespace())
        .count();
    if other > 0 && letters / other.max(1) < 4 {
        return false;
    }

    true
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn normalize_body_text(value: &str) -> String {
    let mut output = Vec::new();
    let mut blank_count = 0usize;

    for line in normalize_newlines(value).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 && !output.is_empty() {
                output.push(String::new());
            }
        } else {
            blank_count = 0;
            output.push(trimmed.to_string());
        }
    }

    output.join("\n").trim().to_string()
}

fn normalize_context_label(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn build_entry_block(timestamp: &DateTime<Local>, context_label: &str, snippet: &str) -> String {
    format!(
        "[{} · {}]\n{}",
        timestamp.format("%-I:%M %p"),
        context_label,
        snippet
    )
}

fn append_entry_block(existing_body: &str, next_entry: &str) -> String {
    let trimmed = existing_body.trim();
    if trimmed.is_empty() {
        next_entry.to_string()
    } else {
        format!("{trimmed}\n\n{next_entry}")
    }
}

fn extract_transcript_body(content: &str) -> String {
    content
        .split_once(TRANSCRIPT_SECTION_MARKER)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|| content.to_string())
        .trim()
        .to_string()
}

fn build_daily_document(
    body: &str,
    title: &str,
    first_captured_at: &str,
    last_captured_at: &str,
) -> String {
    let summary = summarize_transcript_body(body, first_captured_at, last_captured_at);

    format!("{title}\n\nSummary\n{summary}\n\nTranscript\n\n{}", body.trim())
}

fn summarize_transcript_body(
    body: &str,
    first_captured_at: &str,
    last_captured_at: &str,
) -> String {
    let entry_count = body
        .lines()
        .filter(|line| line.starts_with('[') && line.ends_with(']'))
        .count();
    let first_local = parse_rfc3339_to_local(first_captured_at);
    let last_local = parse_rfc3339_to_local(last_captured_at);
    let active_span = match (first_local, last_local) {
        (Some(first), Some(last)) => format!(
            "- Active from {} to {}.",
            first.format("%-I:%M %p"),
            last.format("%-I:%M %p")
        ),
        _ => "- Active throughout the day.".to_string(),
    };
    let app_summary = extract_context_labels(body);
    let topics = extract_top_topics(body, 4);

    let mut lines = vec![
        format!("- {entry_count} spoken snippet{} captured today.", if entry_count == 1 { "" } else { "s" }),
        active_span,
    ];

    if !app_summary.is_empty() {
        lines.push(format!("- Captured in {}.", app_summary.join(", ")));
    }

    if !topics.is_empty() {
        lines.push(format!("- Mentioned often: {}.", topics.join(", ")));
    }

    lines.join("\n")
}

fn parse_rfc3339_to_local(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.with_timezone(&Local))
}

fn extract_context_labels(body: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();

    for line in body.lines() {
        if !line.starts_with('[') || !line.ends_with(']') {
            continue;
        }

        let inner = line.trim_matches(['[', ']']);
        let mut parts = inner.split(" · ");
        let _ = parts.next();
        if let Some(label) = parts.next() {
            let label = label.trim();
            if !label.is_empty() {
                seen.insert(label.to_string());
            }
        }
    }

    seen.into_iter().take(4).collect()
}

fn extract_top_topics(body: &str, limit: usize) -> Vec<String> {
    let stopwords = TOPIC_STOPWORDS.iter().copied().collect::<HashSet<_>>();
    let mut scores = HashMap::<String, usize>::new();

    for line in body.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            continue;
        }

        for raw_token in line
            .split(|character: char| !character.is_alphanumeric() && character != '-' && character != '_')
            .map(|token| token.trim_matches(['-', '_']))
            .filter(|token| token.len() >= 4)
        {
            let normalized = raw_token.to_ascii_lowercase();
            if stopwords.contains(normalized.as_str())
                || normalized.chars().all(|character| character.is_numeric())
            {
                continue;
            }

            *scores.entry(normalized).or_insert(0) += 1;
        }
    }

    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    ranked
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .take(limit)
        .map(|(token, _)| token)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

    use crate::{
        db::{migrations::run_migrations, sqlite_memory_repository::SqliteMemoryRepository},
        models::AppContextSnapshot,
    };

    use super::{build_daily_document, extract_transcript_body, is_spoken_context, SpokenTranscriptService};

    async fn make_service() -> SpokenTranscriptService {
        let options = SqliteConnectOptions::from_str(":memory:").expect("in-memory options");
        let pool = SqlitePool::connect_with(options).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        SpokenTranscriptService::new(Arc::new(SqliteMemoryRepository::new(pool)))
    }

    #[test]
    fn spoken_context_detects_spoken_in_app_or_window() {
        assert!(is_spoken_context(&AppContextSnapshot {
            source_app: Some("Spoken".into()),
            source_window: None,
        }));
        assert!(is_spoken_context(&AppContextSnapshot {
            source_app: Some("Chrome".into()),
            source_window: Some("Spoken Overlay".into()),
        }));
        assert!(!is_spoken_context(&AppContextSnapshot {
            source_app: Some("Chrome".into()),
            source_window: Some("ChatGPT".into()),
        }));
    }

    #[tokio::test]
    async fn spoken_snippets_roll_into_one_daily_memory() {
        let service = make_service().await;
        let context = AppContextSnapshot {
            source_app: Some("Spoken".into()),
            source_window: Some("Spoken Overlay".into()),
        };

        let first = service
            .capture_clipboard_snippet("We should tighten the pricing page copy.".into(), &context)
            .await
            .expect("first transcript save");
        let second = service
            .capture_clipboard_snippet("Let's also revisit onboarding tomorrow.".into(), &context)
            .await
            .expect("second transcript save");

        assert_eq!(first.id, second.id);
        assert_eq!(second.source_app.as_deref(), Some("spoken"));
        assert!(second.content.contains("Summary"));
        assert!(second.content.contains("Transcript"));
        let body = extract_transcript_body(&second.content);
        assert!(body.contains("pricing page copy"));
        assert!(body.contains("revisit onboarding tomorrow"));
    }

    #[test]
    fn daily_document_contains_summary_and_transcript_sections() {
        let content = build_daily_document(
            "[9:12 AM · Spoken]\nTest snippet",
            "Spoken transcript · Apr 25",
            "2026-04-25T03:42:00Z",
            "2026-04-25T04:12:00Z",
        );

        assert!(content.contains("Summary"));
        assert!(content.contains("Transcript"));
        assert!(content.contains("spoken snippet"));
    }
}
