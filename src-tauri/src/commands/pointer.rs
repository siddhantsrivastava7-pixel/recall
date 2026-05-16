//! v0.5.61 — Recall Pointer commands.
//!
//! Recall Pointer is the memory-aware context bridge: copy text
//! anywhere, hit the Pointer hotkey, and the overlay surfaces
//! what you already know about it from your saved corpus.
//!
//! This module is intentionally thin. The *capture* happens in
//! the global-shortcut handler (lib.rs) — it reads the clipboard,
//! resolves app context, and stashes a `PointerSelection` in
//! AppState. The frontend then **takes** that stash exactly once
//! via [`pointer_take_selection`]. Every downstream action (Save
//! / Find related / Ask Recall) reuses an existing command —
//! there is no Pointer-specific retrieval or save path. The
//! differentiator is the *bridge*, not new machinery.
//!
//! Take-once semantics matter: the search-overlay window is
//! shared between plain search and Pointer mode. If the stash
//! were read non-destructively, a Pointer selection from an hour
//! ago could resurface the next time the user opens the overlay
//! for an unrelated keyword search. Reading clears it.

use tauri::State;

use crate::{
    errors::app_error::AppResult,
    models::PointerSelection,
    state::app_state::AppState,
};

/// Take (read-and-clear) the pending Pointer selection. Returns
/// `None` when there's nothing stashed — which is the signal the
/// frontend uses to fall back to ordinary search-overlay
/// rendering. Idempotent after the first call until the next
/// hotkey trigger writes a fresh selection.
#[tauri::command]
pub async fn pointer_take_selection(
    state: State<'_, AppState>,
) -> AppResult<Option<PointerSelection>> {
    let mut slot = state.pointer_selection.lock().await;
    Ok(slot.take())
}
