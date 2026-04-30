// AI subsystem client (v0.2.0+).
//
// Thin wrapper over `tauri::invoke` for the five AI-related commands the
// Rust backend exposes. Kept separate from the main `tauri-client.ts` so
// adding Phase 2 commands (semantic search, embeddings, ask-recall) is a
// single-file diff that doesn't touch the rest of the surface area.
//
// All commands fail soft from the UI's perspective — every caller in the
// settings tab catches errors and surfaces them as inline notices, never
// as toasts that disrupt the rest of the app.

import { invoke } from "@tauri-apps/api/core";
import type { AiStatusPayload } from "@/domain/types";

export const aiClient = {
  /// Read-only snapshot for the AI Settings tab + status badges.
  status: () => invoke<AiStatusPayload>("ai_status"),

  /// Master toggle. Persists `aiEnabled` in `app_settings` and flips the
  /// scheduler atomic so workers wake/sleep accordingly.
  setEnabled: (enabled: boolean) =>
    invoke<AiStatusPayload>("ai_set_enabled", { enabled }),

  /// Mode picker — Phase 1 only honors `"off" | "on"`. Reserved surface
  /// area so the v0.3.0 Lite/Smart/Pro picker doesn't need a rename.
  setMode: (mode: "off" | "on" | "lite" | "smart" | "pro") =>
    invoke<AiStatusPayload>("ai_set_mode", { mode }),

  /// Manually enqueue OCR for a single memory. Used by a future
  /// "Re-run OCR" action on the memory detail; not surfaced in v0.2.0
  /// outside the rebuild flow.
  runOcrForMemory: (memoryId: string) =>
    invoke<boolean>("ocr_run_for_memory", { memoryId }),

  /// Bulk-enqueue OCR for every eligible memory the queue doesn't
  /// already cover. Returns the number of rows newly queued.
  rebuildIndex: () => invoke<number>("ocr_rebuild_index"),
};
