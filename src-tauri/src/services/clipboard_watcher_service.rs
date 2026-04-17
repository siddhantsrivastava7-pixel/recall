use std::collections::VecDeque;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep, Duration};

use crate::{
    models::{MemoryInput, MemorySourceType},
    services::link_utils::detect_primary_url,
    state::app_state::AppState,
};

const CLIPBOARD_POLL_INTERVAL_MS: u64 = 900;
const MIN_MEANINGFUL_TEXT_CHARS: usize = 20;
const RECENT_CAPTURE_SIGNATURE_LIMIT: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstantCaptureSavedEvent {
    memory_id: String,
}

pub fn start_clipboard_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_seen_signature = read_clipboard_signature(&app).await;
        let mut recent_capture_signatures = VecDeque::<String>::new();

        loop {
            sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS)).await;

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

pub fn is_meaningful_clipboard_content(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    detect_primary_url(trimmed, None).is_some()
        || trimmed.chars().count() > MIN_MEANINGFUL_TEXT_CHARS
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
    fn meaningful_clipboard_content_rejects_noise() {
        assert!(!is_meaningful_clipboard_content(""));
        assert!(!is_meaningful_clipboard_content("short text"));
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
