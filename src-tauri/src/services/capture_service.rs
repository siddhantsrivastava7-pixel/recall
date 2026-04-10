use chrono::Utc;
use sqlx::SqlitePool;

use crate::{
    db::{
        repositories::SharedMemoryRepository,
        system_projects::{ensure_default_inbox_project, DEFAULT_INBOX_PROJECT_ID},
    },
    errors::app_error::{AppError, AppResult},
    models::{BookmarkBrowser, Memory, MemoryInput, MemorySourceType},
    services::link_utils::detect_primary_url,
};

#[derive(Debug, Clone)]
pub struct BookmarkCaptureInput {
    pub browser: BookmarkBrowser,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub folder_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy)]
enum CaptureOrigin {
    Manual,
    BookmarkImport,
    Duplicate,
    Update,
}

#[derive(Debug, Clone)]
struct PreparedCaptureInput {
    input: MemoryInput,
    steps: Vec<&'static str>,
}

pub struct CaptureInputBuilder;

impl CaptureInputBuilder {
    fn from_manual_input(input: MemoryInput) -> AppResult<PreparedCaptureInput> {
        prepare_capture_input(input)
    }

    fn from_bookmark(input: BookmarkCaptureInput) -> AppResult<PreparedCaptureInput> {
        prepare_capture_input(MemoryInput {
            source_type: Some(MemorySourceType::Bookmark),
            title: Some(input.title),
            content: input.url.clone(),
            note: None,
            project_id: None,
            url: Some(input.url),
            external_id: Some(input.external_id),
            folder_path: input.folder_path,
            source_app: Some(input.browser.as_source_app().to_string()),
            source_window: None,
            created_at: Some(input.created_at.clone()),
            updated_at: Some(input.created_at),
        })
    }
}

pub struct CaptureService {
    pool: SqlitePool,
    repository: SharedMemoryRepository,
}

impl CaptureService {
    pub fn new(pool: SqlitePool, repository: SharedMemoryRepository) -> Self {
        Self { pool, repository }
    }

    pub async fn create(&self, input: MemoryInput) -> AppResult<Memory> {
        self.persist(CaptureOrigin::Manual, CaptureInputBuilder::from_manual_input(input)?)
            .await
    }

    pub async fn update(&self, id: &str, input: MemoryInput) -> AppResult<Memory> {
        let prepared = CaptureInputBuilder::from_manual_input(input)?;
        self.log_start(CaptureOrigin::Update, &prepared);

        let started_at = std::time::Instant::now();
        match self.repository.update(id, prepared.input).await {
            Ok(memory) => {
                self.log_success(
                    CaptureOrigin::Update,
                    &memory.id,
                    memory.source_type,
                    started_at.elapsed(),
                    &prepared.steps,
                );
                Ok(memory)
            }
            Err(error) => {
                self.log_failure(CaptureOrigin::Update, started_at.elapsed(), &prepared.steps, &error);
                Err(error)
            }
        }
    }

    pub async fn create_bookmark(&self, input: BookmarkCaptureInput) -> AppResult<Memory> {
        self.persist(CaptureOrigin::BookmarkImport, CaptureInputBuilder::from_bookmark(input)?)
            .await
    }

    pub async fn duplicate_from_memory(&self, original: Memory) -> AppResult<Memory> {
        let prepared = CaptureInputBuilder::from_manual_input(MemoryInput {
            title: original
                .title
                .map(|title| format!("{title} (Copy)"))
                .or(Some("Untitled memory (Copy)".into())),
            content: original.content,
            note: original.note,
            project_id: original.project_id,
            source_type: Some(original.source_type),
            url: original.url,
            external_id: None,
            folder_path: original.folder_path,
            source_app: original.source_app,
            source_window: original.source_window,
            created_at: None,
            updated_at: None,
        })?;

        self.persist(CaptureOrigin::Duplicate, prepared).await
    }

    async fn persist(
        &self,
        origin: CaptureOrigin,
        prepared: PreparedCaptureInput,
    ) -> AppResult<Memory> {
        self.log_start(origin, &prepared);

        if prepared.input.project_id.as_deref() == Some(DEFAULT_INBOX_PROJECT_ID) {
            ensure_default_inbox_project(&self.pool).await?;
        }

        let started_at = std::time::Instant::now();
        match self.repository.create(prepared.input).await {
            Ok(memory) => {
                self.log_success(
                    origin,
                    &memory.id,
                    memory.source_type,
                    started_at.elapsed(),
                    &prepared.steps,
                );
                Ok(memory)
            }
            Err(error) => {
                self.log_failure(origin, started_at.elapsed(), &prepared.steps, &error);
                Err(error)
            }
        }
    }

    fn log_start(&self, origin: CaptureOrigin, prepared: &PreparedCaptureInput) {
        debug_capture_log(format!(
            "start origin={} source_type={} steps={}",
            origin_label(origin),
            source_type_label(prepared.input.source_type.unwrap_or(MemorySourceType::Manual)),
            prepared.steps.join(","),
        ));
    }

    fn log_success(
        &self,
        origin: CaptureOrigin,
        capture_id: &str,
        source_type: MemorySourceType,
        duration: std::time::Duration,
        steps: &[&'static str],
    ) {
        debug_capture_log(format!(
            "success origin={} capture_id={} source_type={} duration_ms={} steps={}",
            origin_label(origin),
            capture_id,
            source_type_label(source_type),
            duration.as_millis(),
            steps.join(","),
        ));
    }

    fn log_failure(
        &self,
        origin: CaptureOrigin,
        duration: std::time::Duration,
        steps: &[&'static str],
        error: &AppError,
    ) {
        debug_capture_log(format!(
            "failure origin={} duration_ms={} steps={} error={}",
            origin_label(origin),
            duration.as_millis(),
            steps.join(","),
            error,
        ));
    }
}

fn debug_capture_log(message: String) {
    if cfg!(debug_assertions) {
        eprintln!("[recall][capture] {message}");
    }
}

fn origin_label(origin: CaptureOrigin) -> &'static str {
    match origin {
        CaptureOrigin::Manual => "manual",
        CaptureOrigin::BookmarkImport => "bookmark-import",
        CaptureOrigin::Duplicate => "duplicate",
        CaptureOrigin::Update => "update",
    }
}

fn source_type_label(source_type: MemorySourceType) -> &'static str {
    match source_type {
        MemorySourceType::Manual => "manual",
        MemorySourceType::Bookmark => "bookmark",
    }
}

fn prepare_capture_input(input: MemoryInput) -> AppResult<PreparedCaptureInput> {
    let mut steps = Vec::new();
    let had_source_type = input.source_type.is_some();
    let had_project_id = input.project_id.is_some();
    let original_note = input.note.clone();
    let original_url = input.url.clone();
    let original_source_app = input.source_app.clone();
    let original_source_window = input.source_window.clone();

    let normalized_content = normalize_body_text(&input.content);
    if normalized_content != input.content {
        steps.push("normalized-content");
    }
    if normalized_content.is_empty() {
        return Err(AppError::Invalid("Memory content is required.".into()));
    }

    let source_type = input.source_type.unwrap_or(MemorySourceType::Manual);
    if !had_source_type {
        steps.push("defaulted-source-type");
    }

    let title = normalize_optional_single_line(input.title);
    let note = normalize_optional_body_text(input.note);
    let url = detect_primary_url(&normalized_content, input.url.as_deref());
    let external_id = normalize_optional_single_line(input.external_id);
    let folder_path = normalize_optional_single_line(input.folder_path);
    let source_app = normalize_optional_single_line(input.source_app);
    let source_window = normalize_optional_single_line(input.source_window);
    let project_id = normalize_optional_single_line(input.project_id)
        .or_else(|| Some(DEFAULT_INBOX_PROJECT_ID.to_string()));

    if project_id.as_deref() == Some(DEFAULT_INBOX_PROJECT_ID) && !had_project_id {
        steps.push("assigned-inbox");
    }

    let inferred_title = if title.is_none() {
        steps.push("derived-title");
        Some(derive_title(&normalized_content))
    } else {
        title
    };

    if url.is_some() && original_url != url {
        steps.push(if original_url.is_some() {
            "normalized-url"
        } else {
            "detected-url"
        });
    }
    if note.is_some() != original_note.is_some() || note != original_note {
        steps.push("normalized-note");
    }
    if source_app != original_source_app || source_window != original_source_window {
        steps.push("normalized-source");
    }

    let created_at = input.created_at.or_else(|| Some(Utc::now().to_rfc3339()));
    let updated_at = input.updated_at.or_else(|| created_at.clone());
    let normalized_capture_content = if source_type == MemorySourceType::Bookmark {
        url.clone().unwrap_or(normalized_content)
    } else {
        normalized_content
    };

    Ok(PreparedCaptureInput {
        input: MemoryInput {
            source_type: Some(source_type),
            title: inferred_title,
            content: normalized_capture_content,
            note,
            project_id,
            url,
            external_id,
            folder_path,
            source_app,
            source_window,
            created_at,
            updated_at,
        },
        steps,
    })
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn collapse_empty_lines(value: &str) -> String {
    let mut output = Vec::new();
    let mut blank_count = 0usize;

    for line in value.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                output.push(String::new());
            }
        } else {
            blank_count = 0;
            output.push(trimmed_end.to_string());
        }
    }

    output.join("\n").trim().to_string()
}

fn normalize_body_text(value: &str) -> String {
    collapse_empty_lines(&normalize_newlines(value))
}

fn normalize_optional_body_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let normalized = normalize_body_text(&value);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn normalize_optional_single_line(value: Option<String>) -> Option<String> {
    normalize_optional_body_text(value).and_then(|value| {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn truncate_title(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn first_sentence(value: &str) -> Option<String> {
    for (index, character) in value.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            let sentence = value[..=index].trim();
            if !sentence.is_empty() {
                return Some(sentence.to_string());
            }
        }
    }
    None
}

fn derive_title(content: &str) -> String {
    let first_line = content.lines().map(str::trim).find(|line| !line.is_empty());

    if let Some(line) = first_line {
        if line.chars().count() <= 96 {
            return line.to_string();
        }
    }

    if let Some(sentence) = first_sentence(content) {
        return truncate_title(&sentence, 96);
    }

    if let Some(line) = first_line {
        return truncate_title(line, 96);
    }

    "Untitled Memory".into()
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

    use crate::{
        db::{
            migrations::run_migrations,
            sqlite_memory_repository::SqliteMemoryRepository,
            system_projects::DEFAULT_INBOX_PROJECT_ID,
        },
        models::{BookmarkBrowser, LinkEnrichmentStatus, MemorySourceType},
    };

    use super::{BookmarkCaptureInput, CaptureInputBuilder, CaptureService};

    async fn make_capture_service() -> CaptureService {
        let options = SqliteConnectOptions::from_str(":memory:").expect("in-memory options");
        let pool = SqlitePool::connect_with(options).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        CaptureService::new(pool.clone(), Arc::new(SqliteMemoryRepository::new(pool)))
    }

    #[tokio::test]
    async fn quick_capture_is_normalized_and_enriched() {
        let service = make_capture_service().await;

        let memory = service
            .create(crate::models::MemoryInput {
                source_type: None,
                title: Some("   ".into()),
                content: "  Pricing strategy\r\n\r\n\r\nKeep this for later.  ".into(),
                note: Some("  Why this matters.\r\n\r\n  ".into()),
                project_id: None,
                url: None,
                external_id: None,
                folder_path: None,
                source_app: Some("  Chrome  ".into()),
                source_window: Some("  Pricing Doc  ".into()),
                created_at: None,
                updated_at: None,
            })
            .await
            .expect("capture should succeed");

        assert_eq!(memory.source_type, MemorySourceType::Manual);
        assert_eq!(memory.title.as_deref(), Some("Pricing strategy"));
        assert_eq!(memory.content, "Pricing strategy\n\nKeep this for later.");
        assert_eq!(memory.note.as_deref(), Some("Why this matters."));
        assert_eq!(memory.project_id.as_deref(), Some(DEFAULT_INBOX_PROJECT_ID));
        assert_eq!(memory.source_app.as_deref(), Some("Chrome"));
        assert_eq!(memory.source_window.as_deref(), Some("Pricing Doc"));
    }

    #[tokio::test]
    async fn bookmark_capture_is_saved_consistently() {
        let service = make_capture_service().await;

        let memory = service
            .create_bookmark(BookmarkCaptureInput {
                browser: BookmarkBrowser::Chrome,
                external_id: "   bookmark-1   ".into(),
                title: "  OpenAI pricing docs ".into(),
                url: " HTTPS://Platform.OpenAI.com/docs/pricing ".into(),
                folder_path: Some(" Research / API ".into()),
                created_at: "2026-04-01T09:00:00.000Z".into(),
            })
            .await
            .expect("bookmark capture should succeed");

        assert_eq!(memory.source_type, MemorySourceType::Bookmark);
        assert_eq!(
            memory.url.as_deref(),
            Some("https://platform.openai.com/docs/pricing")
        );
        assert_eq!(memory.domain.as_deref(), Some("platform.openai.com"));
        assert_eq!(memory.enrichment_status, Some(LinkEnrichmentStatus::Pending));
        assert_eq!(
            memory.content,
            "https://platform.openai.com/docs/pricing"
        );
        assert_eq!(memory.external_id.as_deref(), Some("bookmark-1"));
        assert_eq!(memory.source_app.as_deref(), Some("chrome"));
        assert_eq!(memory.folder_path.as_deref(), Some("Research / API"));
        assert_eq!(memory.project_id.as_deref(), Some(DEFAULT_INBOX_PROJECT_ID));
    }

    #[tokio::test]
    async fn empty_capture_is_rejected() {
        let prepared = CaptureInputBuilder::from_manual_input(crate::models::MemoryInput {
            source_type: None,
            title: None,
            content: " \n\t  ".into(),
            note: None,
            project_id: None,
            url: None,
            external_id: None,
            folder_path: None,
            source_app: None,
            source_window: None,
            created_at: None,
            updated_at: None,
        });

        assert!(prepared.is_err());
    }

    #[tokio::test]
    async fn clipboard_like_capture_keeps_context_and_infers_title() {
        let service = make_capture_service().await;

        let memory = service
            .create(crate::models::MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: None,
                content: "   https://example.com/docs/render   ".into(),
                note: None,
                project_id: Some("".into()),
                url: Some(" HTTPS://Example.com/docs/render ".into()),
                external_id: None,
                folder_path: None,
                source_app: Some(" Brave ".into()),
                source_window: Some(" Render Docs ".into()),
                created_at: None,
                updated_at: None,
            })
            .await
            .expect("clipboard capture should succeed");

        assert_eq!(memory.title.as_deref(), Some("https://example.com/docs/render"));
        assert_eq!(memory.url.as_deref(), Some("https://example.com/docs/render"));
        assert_eq!(memory.domain.as_deref(), Some("example.com"));
        assert_eq!(memory.enrichment_status, Some(LinkEnrichmentStatus::Pending));
        assert_eq!(memory.source_app.as_deref(), Some("Brave"));
        assert_eq!(memory.source_window.as_deref(), Some("Render Docs"));
    }

    #[tokio::test]
    async fn embedded_url_is_detected_without_explicit_url_field() {
        let service = make_capture_service().await;

        let memory = service
            .create(crate::models::MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: None,
                content: "Remember this launch note https://example.com/posts/pricing-strategy".into(),
                note: Some("Useful later".into()),
                project_id: None,
                url: None,
                external_id: None,
                folder_path: None,
                source_app: None,
                source_window: None,
                created_at: None,
                updated_at: None,
            })
            .await
            .expect("embedded url capture should succeed");

        assert_eq!(
            memory.url.as_deref(),
            Some("https://example.com/posts/pricing-strategy"),
        );
        assert_eq!(memory.domain.as_deref(), Some("example.com"));
        assert_eq!(memory.enrichment_status, Some(LinkEnrichmentStatus::Pending));
    }
}
