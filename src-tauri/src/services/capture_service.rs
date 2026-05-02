use std::sync::OnceLock;

use chrono::Utc;
use sqlx::SqlitePool;

use crate::{
    ai::{
        embeddings::{auto_tagger, chunker},
        scheduler::AiScheduler,
    },
    db::{
        repositories::{ChunkUpsert, SharedMemoryRepository},
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
    /// AI scheduler for the post-save OCR hook. Empty until installed at
    /// app boot via [`Self::install_ai_scheduler`]. Held as `OnceLock`
    /// rather than `Option` so we can write through `&self` (the service
    /// is wrapped in `Arc` and shared across handlers).
    ai_scheduler: OnceLock<AiScheduler>,
}

impl CaptureService {
    pub fn new(pool: SqlitePool, repository: SharedMemoryRepository) -> Self {
        Self {
            pool,
            repository,
            ai_scheduler: OnceLock::new(),
        }
    }

    /// Install the AI scheduler. Idempotent — only the first call wins.
    /// The capture service will enqueue OCR jobs from this moment on for
    /// any new screenshot / imported-image memory it commits.
    pub fn install_ai_scheduler(&self, scheduler: AiScheduler) {
        let _ = self.ai_scheduler.set(scheduler);
    }

    /// Chunk + enqueue-embed hook. Called after `repository.create` and
    /// `repository.update` returns so the save path is never blocked
    /// by chunking or queue I/O. Hash-aware: an unchanged chunk keeps
    /// its existing embedding, novel chunks become embed jobs.
    fn maybe_chunk_and_embed(&self, memory: &Memory) {
        let Some(scheduler) = self.ai_scheduler.get() else {
            return;
        };
        if !scheduler.is_enabled() {
            return;
        }

        // Skip the (currently) placeholder body for fresh screenshot
        // memories — the OCR worker promotes content + re-runs this
        // hook via a follow-up update once the real text lands. No
        // sense embedding "Screenshot from clipboard (1010×455)..."
        // verbatim; the related-memory results would be useless.
        if memory.source_app.as_deref() == Some("screenshot")
            && memory.content.starts_with("Screenshot from clipboard")
            && memory.content.contains("OCR will fill in")
        {
            return;
        }

        let memory_id = memory.id.clone();
        let title = memory.title.clone();
        let content = memory.content.clone();
        let scheduler = scheduler.clone();
        let repository = self.repository.clone();
        tauri::async_runtime::spawn(async move {
            // 1. Auto-tag opaque-token content (license keys, URLs,
            // emails, etc.) so dense retrieval can bridge the gap
            // when raw tokens carry no natural-language signal.
            // Merged into existing `topic_labels` (preserves any
            // tags already set by link enrichment / classifier
            // passes).
            let detected_tags = auto_tagger::detect_tags(&content);
            let tags = match repository
                .merge_topic_labels(&memory_id, &detected_tags)
                .await
            {
                Ok(merged) => merged,
                Err(error) => {
                    eprintln!(
                        "[recall][capture] tag merge failed for {memory_id}: {error}"
                    );
                    Vec::new()
                }
            };

            // v0.5.6: extract structured entities at capture time
            // alongside auto-tagging. Empty projects slice — v0.5.7
            // will plumb the project repo through. Soft-fail
            // because entity extraction is best-effort enrichment,
            // never a path that should block the embedding work.
            let _ = crate::ai::entities::extract_and_persist(
                &repository,
                &memory_id,
                &content,
                &[],
            )
            .await;

            // 2. Run the chunker. Empty content → no chunks → nothing
            // to do (the row stays unembedded; resurface logic skips
            // it cleanly).
            let mut chunks = chunker::chunk_text(&content);
            if chunks.is_empty() {
                return;
            }

            // 3. Recompute each chunk's content_hash to reflect the
            // *enriched* embedding text (title + tags + chunk text).
            // Why: the embedding worker constructs the same enriched
            // text at embed time, so a title or tag change should
            // invalidate the cached vector even if chunk_text is
            // unchanged. Hash semantics align with embedding semantics
            // — neither a stale vector nor an unnecessary re-embed.
            for chunk in &mut chunks {
                let enriched = auto_tagger::enriched_embedding_text(
                    title.as_deref(),
                    &tags,
                    &chunk.text,
                );
                chunk.content_hash = chunker::fnv1a_64_hex(&enriched);
            }

            // 4. Hash-aware replace. Returns the IDs of chunks that
            // need a fresh embedding — anything reused via hash match
            // keeps its existing vector and is excluded from this list.
            let upserts: Vec<ChunkUpsert<'_>> = chunks
                .iter()
                .enumerate()
                .map(|(idx, c)| ChunkUpsert {
                    chunk_index: idx,
                    text: &c.text,
                    start_offset: c.start_offset,
                    end_offset: c.end_offset,
                    byte_size: c.byte_size(),
                    token_estimate: c.token_estimate(),
                    content_hash: &c.content_hash,
                })
                .collect();

            // Capture path doesn't have a direct handle on the
            // scheduler's adapter id without an extra hop, but we
            // know the embedding adapter is wired alongside the
            // scheduler we already hold. Re-use the scheduler's
            // model label as the active model — same value the
            // worker checks, same value embed_all uses.
            let active_model = Some(scheduler.embedding_model_label());
            let needs_embedding = match repository
                .replace_chunks_hash_aware(&memory_id, &upserts, active_model)
                .await
            {
                Ok(ids) => ids,
                Err(error) => {
                    eprintln!(
                        "[recall][capture] chunk replace failed for {memory_id}: {error}"
                    );
                    return;
                }
            };

            // 3. Enqueue an embed job per novel chunk.
            for chunk_id in needs_embedding {
                if let Err(error) = scheduler
                    .enqueue_embed_chunk(&chunk_id, &memory_id)
                    .await
                {
                    eprintln!(
                        "[recall][capture] embed enqueue failed for chunk {chunk_id}: {error}"
                    );
                }
            }
        });
    }

    /// Post-save OCR hook. Called after `repository.create` returns so
    /// the save path is never blocked by adapter probing or queue I/O.
    /// All three PRD conditions are checked here:
    ///   1. `source_type IN ('screenshot', 'imported_image')`
    ///   2. `ocr_status IS NULL OR ocr_status IN ('failed', 'pending')`
    ///   3. `dedupe_key UNIQUE` already prevents duplicate enqueueing —
    ///      `enqueue_ocr_for_memory` returns `Ok(false)` on conflict.
    /// Any error here is logged-and-swallowed: we never let the AI
    /// subsystem's hiccups bubble up into the capture path.
    fn maybe_enqueue_ocr(&self, memory: &Memory) {
        let Some(scheduler) = self.ai_scheduler.get() else {
            return; // scheduler not yet installed (pre-boot) — nothing to do
        };
        if !scheduler.is_enabled() {
            return;
        }

        // Phase 1: source_type is still an enum of `manual | bookmark`,
        // so the database column never matches `screenshot` or
        // `imported_image` yet. The check is wired against the raw
        // `source_app` string (set by capture pipelines when image
        // sources land in v0.2.x) so the hook is forward-compatible.
        let qualifies = matches!(
            memory.source_app.as_deref(),
            Some("screenshot") | Some("imported_image")
        );
        if !qualifies {
            return;
        }

        match memory.ocr_status.as_deref() {
            None | Some("pending") | Some("failed") => {}
            // 'running' or 'done' — already covered.
            _ => return,
        }

        let memory_id = memory.id.clone();
        let scheduler = scheduler.clone();
        // Fire-and-forget: the enqueue is a single SQL INSERT OR IGNORE,
        // but we still don't want to extend the capture future's lifetime
        // by awaiting it. Spawn into the tauri runtime.
        tauri::async_runtime::spawn(async move {
            if let Err(error) = scheduler.enqueue_ocr_for_memory(&memory_id).await {
                eprintln!(
                    "[recall][capture] OCR enqueue failed for {memory_id}: {error}"
                );
            }
        });
    }

    pub async fn create(&self, input: MemoryInput) -> AppResult<Memory> {
        self.persist(
            CaptureOrigin::Manual,
            CaptureInputBuilder::from_manual_input(input)?,
        )
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
                // v0.3.0: re-chunk + re-embed on edit. The hash-aware
                // upsert keeps embeddings for chunks whose text didn't
                // actually change, so a one-word edit triggers at most
                // one re-embed.
                self.maybe_chunk_and_embed(&memory);
                Ok(memory)
            }
            Err(error) => {
                self.log_failure(
                    CaptureOrigin::Update,
                    started_at.elapsed(),
                    &prepared.steps,
                    &error,
                );
                Err(error)
            }
        }
    }

    pub async fn create_bookmark(&self, input: BookmarkCaptureInput) -> AppResult<Memory> {
        self.persist(
            CaptureOrigin::BookmarkImport,
            CaptureInputBuilder::from_bookmark(input)?,
        )
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
                // v0.2.0: AI subsystem post-save hook. Save has already
                // committed; enqueueing OCR is best-effort and async so
                // any latency here can't bleed into the user's capture.
                self.maybe_enqueue_ocr(&memory);
                // v0.3.0: chunk + enqueue-embed hook.
                self.maybe_chunk_and_embed(&memory);
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
            source_type_label(
                prepared
                    .input
                    .source_type
                    .unwrap_or(MemorySourceType::Manual)
            ),
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
    // `file://` URLs come from the v0.2.1 clipboard image branch and
    // point at on-disk screenshots. They have no host, so the regular
    // URL normalizer (`detect_primary_url` → `Url::parse(...).host_str()?`)
    // silently strips them — which would orphan the screenshot from
    // its bytes and break OCR. Pass them through verbatim instead;
    // they're not user-facing links and don't need normalization.
    let url = if input.url.as_deref().is_some_and(|u| u.starts_with("file://")) {
        input.url.clone()
    } else {
        detect_primary_url(&normalized_content, input.url.as_deref())
    };
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
            migrations::run_migrations, sqlite_memory_repository::SqliteMemoryRepository,
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
        assert_eq!(
            memory.summary_text.as_deref(),
            Some("Pricing strategy Keep this for later."),
        );
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
        assert_eq!(
            memory.enrichment_status,
            Some(LinkEnrichmentStatus::Pending)
        );
        assert_eq!(memory.content, "https://platform.openai.com/docs/pricing");
        assert_eq!(
            memory.summary_text.as_deref(),
            Some("OpenAI pricing docs"),
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

        assert_eq!(
            memory.title.as_deref(),
            Some("https://example.com/docs/render")
        );
        assert_eq!(
            memory.url.as_deref(),
            Some("https://example.com/docs/render")
        );
        assert_eq!(memory.domain.as_deref(), Some("example.com"));
        assert_eq!(
            memory.summary_text.as_deref(),
            Some("Saved link from example.com. Open the source to view the saved page."),
        );
        assert_eq!(
            memory.enrichment_status,
            Some(LinkEnrichmentStatus::Pending)
        );
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
                content: "Remember this launch note https://example.com/posts/pricing-strategy"
                    .into(),
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
        assert_eq!(
            memory.enrichment_status,
            Some(LinkEnrichmentStatus::Pending)
        );
    }
}
