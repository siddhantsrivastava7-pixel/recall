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

export interface ClipboardImageDiagnostic {
  ok: boolean;
  message: string;
  width?: number;
  height?: number;
  byteSize?: number;
}

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

  /// Diagnostic: synchronously call clipboard.read_image() and report
  /// what came back. Surfaced by the "Test clipboard image" button in
  /// AI Settings to debug why a copied screenshot might not turn into a
  /// memory.
  diagnoseClipboardImage: () =>
    invoke<ClipboardImageDiagnostic>("ai_diagnose_clipboard_image"),

  /// v0.3.0: trigger embedding model download. Idempotent — returns
  /// quickly when files are already on disk. The user clicks this
  /// once, opting in to the ~30 MB pull from Hugging Face; afterwards
  /// every embed call runs offline.
  downloadEmbeddingModel: () => invoke<boolean>("ai_download_embedding_model"),

  /// v0.3.2: chunk + enqueue embeds for every memory in the library.
  /// Idempotent — chunks whose content_hash already matches keep
  /// their existing vectors. Resets failed embed jobs as part of the
  /// pass so previously-stuck rows get a fresh shot.
  embedAllMemories: () => invoke<EmbedAllSummary>("embed_all_memories"),

  /// v0.3.0: return up to `limit` semantically-related memories for a
  /// source memory. Server-side runs chunk-level cosine + MMR
  /// diversity aggregation.
  findRelated: (memoryId: string, limit?: number) =>
    invoke<RelatedMemoryView[]>("find_related", { memoryId, limit }),
};

/// v0.3.2: backfill summary returned by `embed_all_memories`.
export interface EmbedAllSummary {
  memoriesScanned: number;
  memoriesChunked: number;
  chunksCreated: number;
  chunksEnqueued: number;
  failedJobsReset: number;
}

/// v0.3.0: result row from `find_related`. The chunk fields point at
/// the best-matching slice within the parent memory so the UI can
/// render an excerpt with offsets to highlight.
export interface RelatedMemoryView {
  memoryId: string;
  score: number;
  chunkText: string;
  chunkStart: number;
  chunkEnd: number;
}
