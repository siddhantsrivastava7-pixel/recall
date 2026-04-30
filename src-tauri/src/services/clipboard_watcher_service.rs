use std::collections::VecDeque;

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep, Duration};

use crate::{
    models::{MemoryInput, MemorySourceType},
    platform::contracts::ClipboardImage,
    services::{
        screenshot_store::SCREENSHOT_SOURCE_APP,
        spoken_transcript_service::{detect_transcription_app, looks_like_transcript_text},
    },
    state::app_state::AppState,
};

const CLIPBOARD_POLL_INTERVAL_MS: u64 = 900;
const RECENT_CAPTURE_SIGNATURE_LIMIT: usize = 64;
const RECENT_IMAGE_SIGNATURE_LIMIT: usize = 16;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstantCaptureSavedEvent {
    memory_id: String,
}

pub fn start_clipboard_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_seen_signature = read_clipboard_signature(&app).await;
        let mut recent_capture_signatures = VecDeque::<String>::new();
        // Image-side dedupe is independent of text dedupe — image
        // captures arrive less often but each one is much heavier, and
        // their "noise" comes from a different source (Win+PrintScreen
        // double-firing) that text dedupe can't see.
        let mut last_seen_image_hash: Option<u64> = None;
        let mut recent_image_hashes: VecDeque<u64> = VecDeque::new();

        loop {
            sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS)).await;

            // Gate everything on an active license once per tick — it
            // matches the existing text path and avoids running OCR /
            // disk writes for unactivated copies.
            let state = app.state::<AppState>();
            let license_active = state
                .license_service
                .get_state()
                .await
                .map(|license| license.is_activated)
                .unwrap_or(false);
            if !license_active {
                continue;
            }

            // ── 1. Image branch ─────────────────────────────────────
            //
            // Try the image clipboard *before* text. On Windows, when
            // you copy an image from a browser, the clipboard often
            // contains both the image bytes and an HTML/text fallback;
            // we want the screenshot memory, not the HTML.
            if let Some(image) = read_clipboard_image(&app).await {
                let hash = hash_rgba(&image);
                let is_new = last_seen_image_hash != Some(hash)
                    && !recent_image_hashes.contains(&hash);
                last_seen_image_hash = Some(hash);
                if is_new {
                    match save_clipboard_image_capture(&app, image).await {
                        Ok(memory_id) => {
                            remember_image_hash(&mut recent_image_hashes, hash);
                            let _ = app.emit(
                                "recall://instant-capture-saved",
                                InstantCaptureSavedEvent { memory_id },
                            );
                            // If the platform also exposes an HTML/text
                            // fallback alongside the image, skip text
                            // processing this tick so we don't double-
                            // capture the same paste.
                            continue;
                        }
                        Err(error) => {
                            if cfg!(debug_assertions) {
                                eprintln!(
                                    "[recall][clipboard-watch] image capture failed: {error}"
                                );
                            }
                            // Fall through to text — image failed but
                            // a text fallback might still be useful.
                        }
                    }
                }
            }

            // ── 2. Text branch ──────────────────────────────────────
            let Some(raw_content) = read_clipboard_text(&app).await else {
                continue;
            };
            let Some(signature) = clipboard_signature(&raw_content) else {
                continue;
            };

            if last_seen_signature.as_deref() == Some(signature.as_str()) {
                continue;
            }
            last_seen_signature = Some(signature.clone());

            if !is_meaningful_clipboard_content(&raw_content) {
                continue;
            }
            if recent_capture_signatures
                .iter()
                .any(|recent| recent == &signature)
            {
                continue;
            }

            match save_clipboard_capture(&app, raw_content).await {
                Ok(memory_id) => {
                    remember_signature(&mut recent_capture_signatures, signature);
                    let _ = app.emit(
                        "recall://instant-capture-saved",
                        InstantCaptureSavedEvent { memory_id },
                    );
                }
                Err(error) => {
                    if cfg!(debug_assertions) {
                        eprintln!("[recall][clipboard-watch] capture failed: {error}");
                    }
                }
            }
        }
    });
}

async fn read_clipboard_signature(app: &AppHandle) -> Option<String> {
    let content = read_clipboard_text(app).await?;
    clipboard_signature(&content)
}

async fn read_clipboard_text(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    state.platform.clipboard.read_text(app).await.ok().flatten()
}

async fn read_clipboard_image(app: &AppHandle) -> Option<ClipboardImage> {
    let state = app.state::<AppState>();
    state.platform.clipboard.read_image(app).await.ok().flatten()
}

/// Cheap hash of a clipboard image for in-memory dedupe. We only need
/// to recognize "the same paste fired twice in a row", not be
/// collision-resistant — a 64-bit FNV-1a over a sparse sample of the
/// RGBA plane is plenty and avoids hashing 30 MB of pixels.
fn hash_rgba(image: &ClipboardImage) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in image.width.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for byte in image.height.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    // Sparse sample: every 64th byte. For a 4K screenshot that's ~520k
    // bytes mixed in, ~5ms of work, more than enough entropy.
    for byte in image.rgba.iter().step_by(64) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

async fn save_clipboard_image_capture(
    app: &AppHandle,
    image: ClipboardImage,
) -> crate::errors::app_error::AppResult<String> {
    let state = app.state::<AppState>();
    let store = state.screenshot_store().ok_or_else(|| {
        crate::errors::app_error::AppError::Invalid(
            "Screenshot store not initialized.".into(),
        )
    })?;
    let saved = store
        .save_rgba(image.rgba, image.width, image.height)
        .await?;

    let context = state
        .platform
        .app_context
        .detect_context()
        .await
        .unwrap_or(crate::models::AppContextSnapshot {
            source_app: None,
            source_window: None,
        });

    let now = Utc::now();
    let title = format!("Screenshot · {}", now.format("%b %-d, %Y · %-I:%M %p"));
    // Capture body holds a placeholder. The real searchable text lands
    // on `memory.ocr_text` once the AI scheduler finishes the OCR pass
    // (or never, if AI is disabled — that's fine, the screenshot still
    // shows up in the timeline).
    let placeholder_content = format!(
        "Screenshot from clipboard ({}×{}). OCR will fill in the text once it runs.",
        saved.width, saved.height
    );

    let memory = state
        .memory_service
        .create(MemoryInput {
            // We deliberately keep `source_type = manual` (existing
            // enum, no DB schema change) and tag the screenshot via
            // `source_app` — that's what the capture-service OCR hook
            // gates on, and what the frontend will use to render the
            // image inline.
            source_type: Some(MemorySourceType::Manual),
            title: Some(title),
            content: placeholder_content,
            note: None,
            project_id: None,
            url: Some(saved.file_url),
            external_id: None,
            folder_path: None,
            source_app: Some(SCREENSHOT_SOURCE_APP.to_string()),
            source_window: context.source_window,
            created_at: None,
            updated_at: None,
        })
        .await?;

    app.emit("recall://memory-saved", &memory)?;

    if cfg!(debug_assertions) {
        eprintln!(
            "[recall][clipboard-watch] saved screenshot memory_id={} dimensions={}x{} bytes={}",
            memory.id, saved.width, saved.height, saved.byte_size
        );
    }

    Ok(memory.id)
}

fn remember_image_hash(recent: &mut VecDeque<u64>, hash: u64) {
    recent.push_back(hash);
    while recent.len() > RECENT_IMAGE_SIGNATURE_LIMIT {
        let _ = recent.pop_front();
    }
}

async fn save_clipboard_capture(
    app: &AppHandle,
    content: String,
) -> crate::errors::app_error::AppResult<String> {
    let state = app.state::<AppState>();
    let context = state
        .platform
        .app_context
        .detect_context()
        .await
        .unwrap_or(crate::models::AppContextSnapshot {
            source_app: None,
            source_window: None,
        });

    // Transcription-app routing: if any known transcription app (Spoken,
    // Whisper, Wispr Flow, MacWhisper, Otter, Granola, …) is running OR is in
    // the frontmost-app snapshot, AND the content reads as natural speech
    // (no URLs / code / tabular data), fold it into today's daily transcript
    // memory rather than creating a separate memory per line. URLs / code /
    // structured content still fall through to the normal capture path so
    // link enrichment + bookmark intelligence run on them.
    let detected_transcription_app = detect_transcription_app(&context);
    let route_to_transcript =
        detected_transcription_app.is_some() && looks_like_transcript_text(&content);
    if route_to_transcript {
        let transcript = state
            .spoken_transcript_service
            .capture_clipboard_snippet(content, &context, detected_transcription_app)
            .await?;

        app.emit("recall://memory-saved", &transcript)?;

        if cfg!(debug_assertions) {
            eprintln!(
                "[recall][clipboard-watch] appended spoken transcript memory_id={}",
                transcript.id
            );
        }

        return Ok(transcript.id);
    }

    let memory = state
        .memory_service
        .create(MemoryInput {
            source_type: Some(MemorySourceType::Manual),
            title: None,
            content,
            note: None,
            project_id: None,
            url: None,
            external_id: None,
            folder_path: None,
            source_app: context.source_app,
            source_window: context.source_window,
            created_at: None,
            updated_at: None,
        })
        .await?;

    app.emit("recall://memory-saved", &memory)?;
    state
        .link_enrichment_service
        .schedule_for_memory(app.clone(), memory.clone())
        .await;

    if cfg!(debug_assertions) {
        eprintln!(
            "[recall][clipboard-watch] saved memory_id={} source_type=manual",
            memory.id
        );
    }

    Ok(memory.id)
}

fn remember_signature(recent: &mut VecDeque<String>, signature: String) {
    recent.push_back(signature);
    while recent.len() > RECENT_CAPTURE_SIGNATURE_LIMIT {
        let _ = recent.pop_front();
    }
}

/// Accepts any non-empty trimmed content. Earlier versions enforced a 20-char
/// floor to filter out clipboard noise, but that swallowed deliberate short
/// captures (a phone number, a name, a transcript snippet, a quick fact). The
/// user explicitly initiated the copy, so trust them; signature dedupe + the
/// recent-capture bloom guard already prevent the worst flood scenarios.
pub fn is_meaningful_clipboard_content(content: &str) -> bool {
    !content.trim().is_empty()
}

pub fn clipboard_signature(content: &str) -> Option<String> {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::{clipboard_signature, is_meaningful_clipboard_content};

    #[test]
    fn meaningful_clipboard_content_accepts_urls() {
        assert!(is_meaningful_clipboard_content(
            "https://github.com/tauri-apps/tauri"
        ));
        assert!(is_meaningful_clipboard_content(
            "Keep this https://example.com/pricing for later"
        ));
    }

    #[test]
    fn meaningful_clipboard_content_accepts_long_text() {
        assert!(is_meaningful_clipboard_content(
            "This is a useful paragraph that should be captured."
        ));
    }

    #[test]
    fn meaningful_clipboard_content_accepts_short_intentional_captures() {
        // Short text is intentionally accepted — the user copied it on
        // purpose. Noise filtering is signature-dedupe's job, not length.
        assert!(is_meaningful_clipboard_content("short text"));
        assert!(is_meaningful_clipboard_content("ok"));
        assert!(is_meaningful_clipboard_content("Apr 26"));
    }

    #[test]
    fn meaningful_clipboard_content_rejects_only_whitespace() {
        assert!(!is_meaningful_clipboard_content(""));
        assert!(!is_meaningful_clipboard_content("   \n\t   "));
    }

    #[test]
    fn clipboard_signature_collapses_whitespace() {
        assert_eq!(
            clipboard_signature("  save   this\n\nlink  ").as_deref(),
            Some("save this link"),
        );
    }
}
