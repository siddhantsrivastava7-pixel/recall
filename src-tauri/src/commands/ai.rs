//! Tauri commands exposing the AI subsystem to the frontend.
//!
//! Phase 1 (v0.2.0) ships exactly five commands — anything more would
//! drift past the locked PRD scope:
//!
//!   * [`ai_status`] — read-only status for the AI Settings tab
//!   * [`ai_set_enabled`] — master toggle
//!   * [`ai_set_mode`] — currently a thin wrapper around the master toggle
//!     (kept in the surface area so Phase 2's Lite/Smart/Pro mode picker
//!     doesn't need a rename later)
//!   * [`ocr_run_for_memory`] — manual "OCR this one memory now"
//!   * [`ocr_rebuild_index`] — re-enqueue OCR for every eligible memory
//!
//! All commands are no-ops when AI is disabled — except the toggles
//! themselves and `ai_status`, which always reads.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{
    ai::hardware::HardwareInfo,
    ai::scheduler::SchedulerStatus,
    errors::app_error::{AppError, AppResult},
    state::app_state::AppState,
};

/// Aggregate snapshot the AI Settings tab renders. Cheap to recompute on
/// every tab open — the queue counts query is one indexed SQL aggregate.
/// Send-only (never deserialized from the frontend) — so we don't pull
/// `Deserialize` here and avoid touching the inner types' derives.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusPayload {
    /// Master enabled flag (mirrors the persisted setting + scheduler
    /// in-memory atomic; both are kept in lock-step).
    pub enabled: bool,
    /// Detected hardware tier + RAM/cores readout for the Settings tab.
    pub hardware: HardwareInfo,
    /// Stable engine label (e.g. `"apple-vision"`, `"windows-media-ocr"`,
    /// `"unsupported"`). Persisted on `memories.ocr_engine`.
    pub ocr_engine: &'static str,
    /// Whether a native OCR engine is available on this host. When
    /// `false`, the master toggle still works but no OCR jobs will run.
    pub ocr_available: bool,
    /// Live queue counts. `running` is informational; `queued` and
    /// `failed` (terminal failures, attempts maxed) drive the UI badges.
    pub queue: SchedulerStatus,
}

#[tauri::command]
pub async fn ai_status(state: State<'_, AppState>) -> AppResult<AiStatusPayload> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    let queue = scheduler.status_snapshot().await?;
    let hardware = scheduler.hardware().clone();
    let ocr_engine = scheduler.ocr_engine_label();

    Ok(AiStatusPayload {
        enabled: scheduler.is_enabled(),
        hardware,
        ocr_engine,
        ocr_available: ocr_engine != "unsupported",
        queue,
    })
}

#[tauri::command]
pub async fn ai_set_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<AiStatusPayload> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    // Persist the new flag on settings — single source of truth for
    // restart, with the in-memory atomic mirroring it for the worker
    // hot-path.
    let mut settings = state.settings_repository.get().await?;
    settings.ai_enabled = enabled;
    state.settings_repository.save(&settings).await?;

    scheduler.set_enabled(enabled);

    let queue = scheduler.status_snapshot().await?;
    Ok(AiStatusPayload {
        enabled,
        hardware: scheduler.hardware().clone(),
        ocr_engine: scheduler.ocr_engine_label(),
        ocr_available: scheduler.ocr_engine_label() != "unsupported",
        queue,
    })
}

/// AI mode is reserved for Phase 2's Lite/Smart/Pro picker. In Phase 1
/// we only ship `"off"` and `"on"` — anything else is rejected so we
/// don't accept values we have no intent to honor.
#[tauri::command]
pub async fn ai_set_mode(mode: String, state: State<'_, AppState>) -> AppResult<AiStatusPayload> {
    let normalized = mode.trim().to_ascii_lowercase();
    let enabled = match normalized.as_str() {
        "off" => false,
        "on" | "lite" | "smart" | "pro" => true,
        other => {
            return Err(AppError::Invalid(format!(
                "Unknown AI mode '{other}'. Allowed: off | on."
            )))
        }
    };
    ai_set_enabled(enabled, state).await
}

#[tauri::command]
pub async fn ocr_run_for_memory(
    memory_id: String,
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    if !scheduler.is_enabled() {
        return Err(AppError::Invalid(
            "Enable AI first to run OCR on individual memories.".into(),
        ));
    }
    scheduler.enqueue_ocr_for_memory(&memory_id).await
}

#[tauri::command]
pub async fn ocr_rebuild_index(state: State<'_, AppState>) -> AppResult<u64> {
    let scheduler = state
        .ai_scheduler()
        .ok_or_else(|| AppError::Invalid("AI scheduler is not initialized.".into()))?;

    if !scheduler.is_enabled() {
        return Err(AppError::Invalid(
            "Enable AI first to run an OCR rebuild.".into(),
        ));
    }
    scheduler.rebuild_ocr_index().await
}

/// Diagnostic snapshot of `clipboard.read_image()`. Used by the AI
/// Settings "Test clipboard image" button to surface, in one click, why
/// a copied screenshot might not be turning into a memory. Returns a
/// structured result so the UI can render the same shape regardless of
/// which branch hit (success / no image / error).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardImageDiagnostic {
    /// `true` when `read_image()` returned a usable image with
    /// non-zero dimensions and a buffer length matching width × height × 4.
    pub ok: bool,
    /// Human-readable summary: `"Got 1920×1080 image (8.3 MB)"` on
    /// success, or the failure reason on the negative path.
    pub message: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: Option<u64>,
}

#[tauri::command]
pub async fn ai_diagnose_clipboard_image(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ClipboardImageDiagnostic> {
    let result = state.platform.clipboard.read_image(&app).await;
    Ok(match result {
        Ok(Some(image)) => {
            let bytes = image.rgba.len() as u64;
            let mb = (bytes as f64) / (1024.0 * 1024.0);
            ClipboardImageDiagnostic {
                ok: true,
                message: format!(
                    "Got {}×{} image ({:.1} MB RGBA). Copy a screenshot, click again, and you should see a new memory appear.",
                    image.width, image.height, mb
                ),
                width: Some(image.width),
                height: Some(image.height),
                byte_size: Some(bytes),
            }
        }
        Ok(None) => ClipboardImageDiagnostic {
            ok: false,
            message: "No image on the clipboard, or the format isn't decodable. Copy an image (Win+Shift+S, Cmd+Shift+4, or right-click an image → Copy) and click again.".into(),
            width: None,
            height: None,
            byte_size: None,
        },
        Err(error) => ClipboardImageDiagnostic {
            ok: false,
            message: format!("Clipboard read failed: {error}"),
            width: None,
            height: None,
            byte_size: None,
        },
    })
}
