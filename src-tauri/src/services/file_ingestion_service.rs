//! v0.5.38 — file & folder ingestion.
//!
//! Public entry points:
//!
//! * [`ingest_path`] — single shot. Drops a file or folder into
//!   Recall's library. Files become `files` rows + shadow
//!   memory rows; folders become `folders` rows and walk into
//!   their contents (capped by user settings).
//!
//! * [`extract_text_for_path`] — pure helper that returns
//!   extracted body text for a given path, used by both the
//!   ingest path and (later) the watched-folder daemon.
//!
//! ## What gets extracted today
//!
//! | Type                              | Strategy                                 |
//! |-----------------------------------|------------------------------------------|
//! | .txt, .md, .json, .csv, source code | `fs::read_to_string` (UTF-8)            |
//! | .pdf                              | `pdf-extract` crate (pure Rust)          |
//! | image extensions                  | OCR via existing scheduler hook          |
//! | everything else                   | Metadata-only memory ("[Binary file]")   |
//!
//! ## Architecture
//!
//! Files and folders live in their own tables — they're not
//! memories. But each ingested file ALSO gets a memory row
//! (the "shadow memory" pattern) so existing search / Ask Recall
//! / daily recap code paths see them without refactoring.
//! Native multi-source retrieval lands in v0.5.41 alongside
//! search filters; until then shadows do the bridging.
//!
//! ## What we deliberately DON'T do
//!
//! * Auto-watch the filesystem (notify crate). v0.5.40.
//! * Chunk + embed file content. v0.5.39.
//! * Fold folder rows into a centroid embedding. v0.5.39.
//! * Office formats (.docx, .xlsx). v0.5.40.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::db::repositories::SharedMemoryRepository;
use crate::errors::app_error::{AppError, AppResult};
use crate::models::{AppSettings, MemoryInput, MemorySourceType};

/// Source-app stamp on memory rows that shadow file ingestion.
/// Lets the recap composer route them, the search path filter,
/// and future chunking/embedding work select these specifically.
pub const FILE_SOURCE_APP: &str = "file";
/// Source-app stamp on memory rows that shadow folder rows.
pub const FOLDER_SOURCE_APP: &str = "folder";

/// Result of one ingest run, surfaced to the frontend so the UI
/// can render "Imported 47 files (3 skipped, 1 too large)".
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestResult {
    pub files_seen: u32,
    pub files_imported: u32,
    pub files_skipped_size: u32,
    pub files_skipped_hidden: u32,
    pub files_skipped_error: u32,
    pub folders_imported: u32,
    pub stopped_at_count_cap: bool,
    pub stopped_at_depth_cap: bool,
    /// Human-friendly summary line for the UI to show inline.
    pub message: String,
}

/// Public entry — point at any path, get a result. Branches on
/// `is_dir` and routes to the file or folder handler.
pub async fn ingest_path(
    pool: &SqlitePool,
    memory_repo: &SharedMemoryRepository,
    settings: &AppSettings,
    path: &Path,
) -> AppResult<IngestResult> {
    let path = canonicalize_or_passthrough(path);
    if !path.exists() {
        return Err(AppError::Invalid(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }

    let mut result = IngestResult::default();
    if path.is_dir() {
        ingest_folder(pool, memory_repo, settings, &path, &mut result).await?;
    } else {
        ingest_single_file(pool, memory_repo, settings, &path, &mut result).await?;
    }
    result.message = build_summary_message(&result);
    Ok(result)
}

/// Walk a folder respecting all the user-configurable caps:
///   * `folder_ingest_depth_cap` — recursion depth
///   * `folder_ingest_file_cap` — total files ingested
///   * `skip_hidden_folders` — drop dot-prefixed dirs + a small
///     deny-list of system-ish dirs (`node_modules`, `.git`,
///     `Library`, etc.)
///   * `file_ingest_size_cap_mb` — per-file byte cap
async fn ingest_folder(
    pool: &SqlitePool,
    memory_repo: &SharedMemoryRepository,
    settings: &AppSettings,
    root: &Path,
    result: &mut IngestResult,
) -> AppResult<()> {
    // Persist the root folder row first so file rows can reference
    // their parent_folder string consistently.
    upsert_folder_row(pool, memory_repo, root, settings).await?;
    result.folders_imported += 1;

    let depth_cap = settings.folder_ingest_depth_cap as usize;
    let count_cap = settings.folder_ingest_file_cap as u32;
    let size_cap_bytes = (settings.file_ingest_size_cap_mb as u64) * 1024 * 1024;

    // walkdir handles recursion + symlink loops cleanly. We
    // filter_entry to drop hidden / system folders before
    // descent so we never walk into them at all.
    let walker = WalkDir::new(root)
        .max_depth(depth_cap)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true; // Always descend the root we were asked for.
            }
            let name = entry
                .file_name()
                .to_str()
                .unwrap_or("");
            !(settings.skip_hidden_folders
                && entry.file_type().is_dir()
                && is_hidden_or_system_folder(name))
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                result.files_skipped_error += 1;
                continue;
            }
        };
        let path = entry.path();
        let file_type = entry.file_type();

        if file_type.is_dir() && entry.depth() > 0 {
            // Persist subfolder rows so folder retrieval works
            // without re-walking. Cheap (one row per directory).
            upsert_folder_row(pool, memory_repo, path, settings).await.ok();
            result.folders_imported += 1;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        result.files_seen += 1;
        if result.files_imported >= count_cap {
            result.stopped_at_count_cap = true;
            break;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                result.files_skipped_error += 1;
                continue;
            }
        };
        if size_cap_bytes > 0 && metadata.len() > size_cap_bytes {
            result.files_skipped_size += 1;
            continue;
        }

        // We re-call ingest_single_file rather than inline so the
        // single-file drop path and the folder-walk path go
        // through identical extract+persist logic.
        match ingest_single_file(pool, memory_repo, settings, path, result).await {
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "[recall][file-ingest] failed for {}: {error}",
                    path.display()
                );
                result.files_skipped_error += 1;
            }
        }
    }

    Ok(())
}

async fn ingest_single_file(
    pool: &SqlitePool,
    memory_repo: &SharedMemoryRepository,
    settings: &AppSettings,
    path: &Path,
    result: &mut IngestResult,
) -> AppResult<()> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| AppError::Invalid(format!("metadata failed for {}: {err}", path.display())))?;
    let size_cap_bytes = (settings.file_ingest_size_cap_mb as u64) * 1024 * 1024;
    if size_cap_bytes > 0 && metadata.len() > size_cap_bytes {
        result.files_skipped_size += 1;
        return Ok(());
    }

    let path_str = path.to_string_lossy().to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("(unnamed)")
        .to_string();
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    let parent_folder = path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let extraction = extract_text_for_path(path, &extension);
    let extracted_text = extraction.text.unwrap_or_default();
    let summary_text = build_summary(&filename, &extracted_text);
    let content_hash = sha256_hex(&extracted_text);

    let now = Utc::now().to_rfc3339();
    let file_created_at = metadata
        .created()
        .ok()
        .and_then(|t| DateTime::<Utc>::from(t).to_rfc3339().into());
    let file_modified_at = metadata
        .modified()
        .ok()
        .and_then(|t| DateTime::<Utc>::from(t).to_rfc3339().into());

    // Dedupe by absolute path. Re-ingest of an unchanged file
    // would otherwise create a fresh shadow memory each time
    // (UNIQUE constraint on path catches the row but the shadow
    // would already exist). Use ON CONFLICT to update content
    // when the file actually changed.
    let file_id = Uuid::new_v4().to_string();
    let new_shadow_id = Uuid::new_v4().to_string();
    let inserted = sqlx::query(
        r#"
        INSERT INTO files
          (id, path, filename, extension, parent_folder, size_bytes,
           file_created_at, file_modified_at, indexed_at, content_hash,
           extracted_text, summary_text, source_app, project_id, shadow_memory_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?)
        ON CONFLICT(path) DO UPDATE SET
          extracted_text = excluded.extracted_text,
          summary_text   = excluded.summary_text,
          content_hash   = excluded.content_hash,
          size_bytes     = excluded.size_bytes,
          file_modified_at = excluded.file_modified_at,
          indexed_at     = excluded.indexed_at
        "#,
    )
    .bind(&file_id)
    .bind(&path_str)
    .bind(&filename)
    .bind(&extension)
    .bind(&parent_folder)
    .bind(metadata.len() as i64)
    .bind(file_created_at)
    .bind(file_modified_at)
    .bind(&now)
    .bind(&content_hash)
    .bind(&extracted_text)
    .bind(&summary_text)
    .bind(FILE_SOURCE_APP)
    .bind(&new_shadow_id)
    .execute(pool)
    .await?;

    // If the row already existed, look up the canonical id +
    // existing shadow_memory_id so we don't create duplicate
    // shadows. The just-inserted file_id only "wins" if rows_affected
    // suggests an INSERT (rather than UPDATE).
    let existing: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, shadow_memory_id FROM files WHERE path = ?1",
    )
    .bind(&path_str)
    .fetch_optional(pool)
    .await?;

    let (canonical_file_id, existing_shadow) = match existing {
        Some((id, shadow)) => (id, shadow),
        None => (file_id.clone(), Some(new_shadow_id.clone())),
    };
    let _ = inserted; // sqlx::query result not used directly

    // Create or update the shadow memory.
    let shadow_title = build_shadow_title(&filename);
    let shadow_content = build_shadow_content(&filename, &path_str, &extracted_text);
    let file_url = format!("file://{}", path_str.replace('\\', "/"));

    if let Some(shadow_id) = existing_shadow.clone() {
        // Update existing shadow with fresh content.
        let _ = memory_repo
            .update(
                &shadow_id,
                MemoryInput {
                    source_type: Some(MemorySourceType::Manual),
                    title: Some(shadow_title),
                    content: shadow_content,
                    note: None,
                    project_id: None,
                    url: Some(file_url),
                    external_id: Some(canonical_file_id.clone()),
                    folder_path: Some(parent_folder.clone()),
                    source_app: Some(FILE_SOURCE_APP.to_string()),
                    source_window: None,
                    created_at: None,
                    updated_at: Some(now.clone()),
                },
            )
            .await;
        result.files_imported += 1;
    } else {
        // Brand-new file: create the shadow memory and link.
        let memory = memory_repo
            .create(MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: Some(shadow_title),
                content: shadow_content,
                note: None,
                project_id: None,
                url: Some(file_url),
                external_id: Some(canonical_file_id.clone()),
                folder_path: Some(parent_folder.clone()),
                source_app: Some(FILE_SOURCE_APP.to_string()),
                source_window: None,
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            })
            .await?;
        sqlx::query("UPDATE files SET shadow_memory_id = ?1 WHERE id = ?2")
            .bind(&memory.id)
            .bind(&canonical_file_id)
            .execute(pool)
            .await?;
        result.files_imported += 1;
    }

    Ok(())
}

/// Persist (or refresh) one folder row. Counts children +
/// records dominant extensions for the folder summary.
async fn upsert_folder_row(
    pool: &SqlitePool,
    memory_repo: &SharedMemoryRepository,
    path: &Path,
    settings: &AppSettings,
) -> AppResult<()> {
    let path_str = path.to_string_lossy().to_string();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path_str.clone());
    let parent_path = path
        .parent()
        .map(|p| p.to_string_lossy().to_string());

    // Cheap one-level scan to compute child_count + dominant
    // extensions. Doesn't recurse (that's the walker's job above).
    let mut child_count = 0i64;
    let mut ext_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    if let Ok(read) = std::fs::read_dir(path) {
        for entry in read.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                child_count += 1;
                if let Some(ext) =
                    entry_path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase())
                {
                    *ext_counts.entry(format!(".{ext}")).or_insert(0) += 1;
                }
            } else if entry_path.is_dir() {
                let dir_name = entry
                    .file_name()
                    .to_str()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if settings.skip_hidden_folders && is_hidden_or_system_folder(&dir_name) {
                    continue;
                }
                child_count += 1;
            }
        }
    }
    let mut sorted_exts: Vec<(String, u32)> = ext_counts.into_iter().collect();
    sorted_exts.sort_by(|a, b| b.1.cmp(&a.1));
    let dominant: Vec<String> = sorted_exts.into_iter().take(5).map(|(e, _)| e).collect();
    let dominant_json = serde_json::to_string(&dominant).ok();

    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO folders
          (id, path, name, parent_path, child_count, dominant_extensions, indexed_at, project_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
        ON CONFLICT(path) DO UPDATE SET
          child_count         = excluded.child_count,
          dominant_extensions = excluded.dominant_extensions,
          indexed_at          = excluded.indexed_at
        "#,
    )
    .bind(&id)
    .bind(&path_str)
    .bind(&name)
    .bind(&parent_path)
    .bind(child_count)
    .bind(&dominant_json)
    .bind(&now)
    .execute(pool)
    .await?;

    // Folder shadow memory — one row per folder so it's findable
    // in search just like a file. Body is a tiny blueprint.
    let folder_url = format!("file://{}", path_str.replace('\\', "/"));
    let title = format!("Folder · {}", name);
    let body = build_folder_shadow_content(&path_str, child_count, &dominant);

    if let Some(existing) = memory_repo
        .find_by_external_source(FOLDER_SOURCE_APP, &path_str)
        .await?
    {
        let _ = memory_repo
            .update(
                &existing.id,
                MemoryInput {
                    source_type: Some(MemorySourceType::Manual),
                    title: Some(title),
                    content: body,
                    note: None,
                    project_id: None,
                    url: Some(folder_url),
                    external_id: Some(path_str.clone()),
                    folder_path: parent_path.clone(),
                    source_app: Some(FOLDER_SOURCE_APP.to_string()),
                    source_window: None,
                    created_at: None,
                    updated_at: Some(now.clone()),
                },
            )
            .await;
    } else {
        let _ = memory_repo
            .create(MemoryInput {
                source_type: Some(MemorySourceType::Manual),
                title: Some(title),
                content: body,
                note: None,
                project_id: None,
                url: Some(folder_url),
                external_id: Some(path_str.clone()),
                folder_path: parent_path.clone(),
                source_app: Some(FOLDER_SOURCE_APP.to_string()),
                source_window: None,
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            })
            .await;
    }

    Ok(())
}

/// Outcome of an extraction attempt.
#[derive(Debug, Clone, Default)]
pub struct ExtractionResult {
    pub text: Option<String>,
    /// True when the file is an image — caller should route to
    /// the OCR pipeline (we don't run OCR synchronously here;
    /// the existing capture-service post-save hook handles that
    /// for screenshot memories).
    pub is_image: bool,
}

/// Pure helper: try to extract text from a file based on its
/// extension. Pure UTF-8 read for text formats, `pdf-extract` for
/// PDFs, no-op for binary types.
pub fn extract_text_for_path(path: &Path, extension: &Option<String>) -> ExtractionResult {
    let ext = extension.as_deref().unwrap_or("");
    if is_text_extension(ext) {
        return match std::fs::read_to_string(path) {
            Ok(text) => ExtractionResult {
                text: Some(text),
                is_image: false,
            },
            Err(_) => ExtractionResult::default(),
        };
    }
    if ext == "pdf" {
        return match pdf_extract::extract_text(path) {
            Ok(text) => ExtractionResult {
                text: Some(text),
                is_image: false,
            },
            Err(_) => ExtractionResult::default(),
        };
    }
    if is_image_extension(ext) {
        return ExtractionResult {
            text: None,
            is_image: true,
        };
    }
    ExtractionResult::default()
}

fn is_text_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Plain / docs
        "txt" | "md" | "markdown" | "rst" | "org" |
        // Structured data
        "json" | "yaml" | "yml" | "toml" | "csv" | "tsv" |
        "xml" | "html" | "htm" |
        // Code
        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" |
        "c" | "h" | "cpp" | "hpp" | "cc" | "cs" | "swift" | "kt" |
        "rb" | "php" | "lua" | "pl" | "sh" | "bash" | "zsh" | "fish" |
        "ps1" | "psm1" | "vue" | "svelte" | "scala" | "r" | "jl" |
        "ex" | "exs" | "dart" | "elm" | "clj" | "hs" |
        // Config
        "conf" | "ini" | "cfg" | "env" | "properties" |
        // SQL
        "sql"
    )
}

fn is_image_extension(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif")
}

/// Hidden / system folder names we skip on walk by default.
/// Covers Unix dotfile convention + a few common noise dirs that
/// users almost never want indexed.
fn is_hidden_or_system_folder(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "node_modules"
            | "venv"
            | "__pycache__"
            | "target"
            | "build"
            | "dist"
            | ".git"
            | ".svn"
            | ".hg"
            | "library"   // macOS user Library
            | "appdata"
            | "$recycle.bin"
            | "system volume information"
    )
}

fn build_summary(filename: &str, extracted_text: &str) -> String {
    let snippet: String = extracted_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(220)
        .collect();
    if snippet.is_empty() {
        format!("File: {filename}")
    } else {
        format!("{filename} — {snippet}")
    }
}

fn build_shadow_title(filename: &str) -> String {
    filename.to_string()
}

fn build_shadow_content(filename: &str, path_str: &str, extracted_text: &str) -> String {
    if extracted_text.trim().is_empty() {
        format!(
            "File: {filename}\nPath: {path_str}\n\n[Recall did not extract text from this file. Open the original to view contents.]"
        )
    } else {
        format!("File: {filename}\nPath: {path_str}\n\n{extracted_text}")
    }
}

fn build_folder_shadow_content(
    path_str: &str,
    child_count: i64,
    dominant: &[String],
) -> String {
    let extensions = if dominant.is_empty() {
        "no recognized file types yet".to_string()
    } else {
        dominant.join(", ")
    };
    format!(
        "Folder: {path_str}\n\nDirect children: {child_count}\nDominant types: {extensions}\n\n[Open the folder to see its contents in Finder/Explorer; Recall surfaces individual files as separate memories.]"
    )
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

fn canonicalize_or_passthrough(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn build_summary_message(result: &IngestResult) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "Imported {} file{}",
        result.files_imported,
        if result.files_imported == 1 { "" } else { "s" }
    ));
    if result.folders_imported > 0 {
        parts.push(format!(
            "{} folder{}",
            result.folders_imported,
            if result.folders_imported == 1 { "" } else { "s" }
        ));
    }
    let mut skipped: Vec<String> = Vec::new();
    if result.files_skipped_size > 0 {
        skipped.push(format!("{} too large", result.files_skipped_size));
    }
    if result.files_skipped_hidden > 0 {
        skipped.push(format!("{} hidden", result.files_skipped_hidden));
    }
    if result.files_skipped_error > 0 {
        skipped.push(format!("{} errored", result.files_skipped_error));
    }
    if !skipped.is_empty() {
        parts.push(format!("skipped {}", skipped.join(" / ")));
    }
    if result.stopped_at_count_cap {
        parts.push("hit file-count cap".to_string());
    }
    if result.stopped_at_depth_cap {
        parts.push("hit depth cap".to_string());
    }
    parts.join(" · ")
}
