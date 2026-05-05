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

  /// v0.5.29: list the most recent failed AI work-queue rows so
  /// the activity-pill modal can render their actual error text.
  /// Backend default is 10 entries; the modal usually shows top
  /// 3-5 but a few extras lets the modal page without round-trip.
  recentAiFailures: () => invoke<AiFailedJob[]>("ai_recent_failures"),

  /// v0.5.30: drop every dead-lettered OCR row from the queue.
  /// Used by the "Clear failed jobs" action in the failure modal.
  /// Returns the count cleared so the UI can confirm visually.
  clearFailedOcr: () => invoke<number>("ai_clear_failed_ocr"),

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

  /// v0.4.0: read-only LLM model info for the user's hardware tier.
  llmStatus: () => invoke<LlmStatusPayload>("ai_llm_status"),

  /// v0.4.0: download the tier-aware Ask Recall model + tokenizer.
  /// Idempotent. Network-bound (1–4 GB depending on tier) on first
  /// call; instant if already on disk.
  downloadLlm: () => invoke<boolean>("ai_download_llm"),

  /// v0.4.0: drop loaded weights from RAM. Next generation will
  /// lazily reload from disk.
  unloadLlm: () => invoke<boolean>("ai_unload_llm"),

  /// v0.4.0a: smoke test. Runs a fixed prompt end-to-end so we can
  /// verify download + load + inference without building the full
  /// Ask Recall pipeline first.
  diagnoseLlm: () => invoke<LlmDiagnosticPayload>("ai_diagnose_llm"),

  /// v0.3.3: hybrid keyword + semantic search. Server-side embeds
  /// the query, runs cosine over active-model chunks (mean-centered),
  /// MMR-aggregates to memory level, then blends with a keyword score
  /// at 0.5 / 0.5. Returns empty list if AI / embeddings aren't ready
  /// — caller falls back to the TS-side keyword path.
  semanticSearch: (query: string, limit?: number) =>
    invoke<SemanticSearchHit[]>("semantic_search", { query, limit }),

  /// v0.4.3 / v0.5.12: Ask Recall — single-shot when sessionId is
  /// undefined (legacy behavior), multi-turn when sessionId is set.
  /// In multi-turn the backend looks up the session, injects past
  /// turns as chat-template messages in the prompt, and appends the
  /// new user/assistant pair to the session after generation.
  askRecall: (question: string, sessionId?: string) =>
    invoke<AskRecallResponse>("ask_recall", { question, sessionId }),

  /// v0.5.11: flip the cancel flag for the in-flight ask. The LLM
  /// generation loop polls every token and returns a partial
  /// response on cancel. Idempotent — calling when nothing is in
  /// flight returns false. Surfaced as the Cancel button in
  /// AskView's input row.
  cancelAskRecall: () => invoke<boolean>("ask_recall_cancel"),

  /// v0.5.15: create a new conversation session and return its id.
  /// Persisted to SQLite so the chat survives app restart. Title
  /// starts as "New chat" and gets replaced first by the user's
  /// first question (trimmed) and then by the LLM-generated
  /// summary (~1s after the first turn completes).
  newAskRecallSession: () => invoke<string>("ask_recall_new_session"),

  /// v0.5.15: fetch the full message list for a session, including
  /// stored citations + retrieved sources. Used by AskView to
  /// rehydrate the thread when the user clicks an entry in the
  /// sidebar.
  getAskRecallSession: (sessionId: string) =>
    invoke<AskRecallSessionFull | null>("ask_recall_get_session", { sessionId }),

  /// v0.5.15: list every session newest-first. Drives the
  /// RECENT CHATS sidebar. Lightweight shape — message bodies
  /// stay in the message table until a session is opened.
  listAskRecallSessions: () =>
    invoke<AskRecallSessionSummary[]>("ask_recall_list_sessions"),

  /// v0.5.15: drop a session and (cascade) all its messages.
  /// Idempotent.
  deleteAskRecallSession: (sessionId: string) =>
    invoke<void>("ask_recall_delete_session", { sessionId }),

  /// v0.5.15: rename a session. Updates the placeholder title;
  /// llm_title is left untouched.
  renameAskRecallSession: (sessionId: string, title: string) =>
    invoke<void>("ask_recall_rename_session", { sessionId, title }),

  /// v0.5.18: lazily generate (or regenerate) the LLM-backed
  /// summary for a Daily recap memory. The backend calls Qwen2.5
  /// with a constrained prompt and persists the result into the
  /// memory's `aiSummary` column. The memory detail view calls
  /// this when it opens a recap memory whose `aiSummary` is
  /// missing or staler than `updatedAt` (a new capture landed
  /// since last generation).
  generateDailyRecapSummary: (memoryId: string) =>
    invoke<DailyRecapSummaryPayload>("generate_daily_recap_summary", { memoryId }),

  /// v0.5.18: save a completed Ask Recall Q&A as a regular memory.
  /// Surfaced as a "Save as memory" button on every committed
  /// assistant bubble. The new memory is stamped with
  /// `sourceApp = "ask-recall"` so it's recognizable and gets
  /// routed to the "Saved notes" section of the Daily recap.
  saveQaAsMemory: (question: string, answer: string) =>
    invoke<SavedQaPayload>("save_qa_as_memory", { question, answer }),

  /// v0.5.23: get the active proactive surface card for Home, if
  /// any. Returns null when no surface qualifies right now (the
  /// slot stays hidden in that case). The engine is idempotent —
  /// repeat calls within the same day return the same surface for
  /// Forgotten Gold; Weekly recap returns the same row until the
  /// week rolls over.
  getProactiveSurface: () =>
    invoke<ActiveProactiveSurface | null>("proactive_surface_get_current"),

  /// v0.5.23: dismiss a surface card. Once dismissed, the row never
  /// renders again — Forgotten Gold won't pick a different memory
  /// the same day either. Tomorrow's once-per-day pick fires fresh.
  dismissProactiveSurface: (surfaceId: string) =>
    invoke<void>("proactive_surface_dismiss", { surfaceId }),

  /// v0.5.8: manual scrub trigger. Runs the v0.5.7 backfill (replace
  /// stale auto-tagger tags + flag self-captures + re-extract entities)
  /// regardless of the persisted "backfill done" flag, and returns a
  /// counts payload. Surfaced as a "Re-scrub" button in AI Settings —
  /// recovery path for users whose v0.5.7 auto-backfill silently
  /// failed (we have no way to log to file on Windows GUI builds, so
  /// silent failures are otherwise undetectable).
  forceScrub: () => invoke<ScrubResult>("ai_force_scrub"),
};

export interface ScrubResult {
  memoriesScanned: number;
  tagRowsUpdated: number;
  selfCapturesMarked: number;
  entitiesExtracted: number;
  errors: number;
  elapsedMs: number;
  /// v0.5.9: rows touched by the brute-force SQL purge.
  bulkPurgeRowsAffected: number;
  /// v0.5.9: per-managed-tag count BEFORE the scrub.
  beforeCounts: Record<string, number>;
  /// v0.5.9: per-managed-tag count AFTER the scrub.
  afterCounts: Record<string, number>;
}

/// v0.3.3: blended search result row. The strength label uses the
/// semantic score only; ranking uses the blended score.
export interface SemanticSearchHit {
  memoryId: string;
  /// Blended ranking score in [0, 1].
  score: number;
  /// Centered cosine, used to derive `strength`.
  semanticScore: number;
  /// Keyword contribution before blending, in [0, 1].
  keywordScore: number;
  strength: "strong" | "related" | "loose";
  chunkText: string;
  chunkStart: number;
  chunkEnd: number;
}

/// v0.4.0: read-only LLM model info for the current hardware tier.
export interface LlmStatusPayload {
  modelId: string;
  hfRepo: string;
  approxDownloadMb: number;
  approxInferenceRamMb: number;
  contextWindowTokens: number;
  ready: boolean;
}

/// v0.4.2: progress event payload for `recall://llm-download-progress`.
/// Emitted by the backend during the GGUF + tokenizer download so the
/// UI can render a real progress bar instead of a "Downloading…" string.
export interface LlmDownloadProgress {
  phase: "gguf" | "tokenizer" | "complete";
  bytesDownloaded: number;
  /// 0 when Content-Length is missing (UI falls back to indeterminate).
  bytesTotal: number;
  message: string;
}

/// v0.4.0a: smoke-test response.
export interface LlmDiagnosticPayload {
  ok: boolean;
  modelId: string;
  prompt: string;
  response: string;
  tokensGenerated: number;
  latencyMs: number;
  message: string;
}

/// v0.3.2: backfill summary returned by `embed_all_memories`.
export interface EmbedAllSummary {
  memoriesScanned: number;
  memoriesChunked: number;
  chunksCreated: number;
  chunksEnqueued: number;
  failedJobsReset: number;
}

/// v0.4.3: one memory cited in an Ask Recall answer. The backend
/// dedupes by memory_id and only returns chunks it actually fed to
/// the LLM, so every chip the UI renders points at a real, retrievable
/// source — no fabrications.
export interface AskRecallCitation {
  memoryId: string;
  title: string | null;
  chunkText: string;
  chunkStart: number;
  chunkEnd: number;
}

/// v0.5.12: a single message in an Ask Recall conversation. Backend
/// stores these in the session's `messages` array and ships them
/// over to the frontend on `getAskRecallSession`. The frontend
/// renders user messages as input bubbles and assistant messages
/// as answer bubbles + their own source-cards panel.
export type AskRecallMessage =
  | {
      role: "user";
      content: string;
      timestamp: string;
    }
  | {
      role: "assistant";
      content: string;
      retrievedSources: AskRecallCitation[];
      citations: AskRecallCitation[];
      tokensGenerated: number;
      latencyMs: number;
      tagIntent: string | null;
      timestamp: string;
    };

/// v0.5.12: full session payload returned by `getAskRecallSession`.
export interface AskRecallSession {
  sessionId: string;
  messages: AskRecallMessage[];
  createdAt: string;
}

/// v0.5.15: persisted session payload for the new SQLite-backed
/// store. Distinct from the v0.5.12 `AskRecallSession` because
/// it adds title fields + last_used_at for sidebar rendering.
export interface AskRecallSessionFull {
  sessionId: string;
  title: string;
  llmTitle: string | null;
  createdAt: string;
  lastUsedAt: string;
  messages: AskRecallMessage[];
}

/// v0.5.15: lightweight summary for the RECENT CHATS sidebar list.
/// Backend's `ask_recall_list_sessions` returns these newest-first.
/// `displayTitle()` (helper) prefers `llmTitle` when present.
export interface AskRecallSessionSummary {
  sessionId: string;
  title: string;
  llmTitle: string | null;
  createdAt: string;
  lastUsedAt: string;
  messageCount: number;
}

/// Helper: pick the LLM-generated title when available, otherwise
/// fall back to the placeholder title (first user message,
/// trimmed). Centralized so every surface renders the same way.
export function chatDisplayTitle(s: { title: string; llmTitle: string | null }): string {
  return s.llmTitle && s.llmTitle.trim().length > 0 ? s.llmTitle : s.title;
}

/// v0.5.18: response shape from `generate_daily_recap_summary`.
/// The frontend writes the summary into the rendered "Summary"
/// block of a Daily recap memory in place of the rule-based one.
export interface DailyRecapSummaryPayload {
  memoryId: string;
  summary: string;
  generatedAt: string;
  tokensGenerated: number;
  latencyMs: number;
}

/// v0.5.18: response shape from `save_qa_as_memory`.
export interface SavedQaPayload {
  memoryId: string;
  title: string;
}

/// v0.5.23: one row of the proactive_surfaces table — a card the
/// surface engine has decided to show at the top of Home. The
/// frontend keys rendering off `kind`; the backend ensures only
/// one ActiveProactiveSurface is returned per call (the highest
/// priority one for the current state).
export interface ProactiveSurfaceRow {
  id: string;
  /// v0.5.23 ships with `'forgotten_gold'` and `'weekly_recap'`.
  /// v0.5.24+ may add `'project_briefing'`, `'researched_before'`.
  kind: string;
  memoryId: string;
  /// 0..=1, comparable within a kind only.
  score: number;
  /// User-facing one-liner rendered as the card subtitle.
  reason: string | null;
  surfacedAt: string;
  dismissedAt: string | null;
  expiresAt: string | null;
}

/// v0.5.23: payload returned by `getProactiveSurface`. Holds the
/// surface row + the underlying memory hydrated so the card can
/// render without a separate fetch round-trip.
export interface ActiveProactiveSurface {
  surface: ProactiveSurfaceRow;
  memory: import("@/domain/types").Memory;
}

/// v0.5.29: one failed AI work-queue row, surfaced to the
/// activity-pill modal so the user can see why something failed.
export interface AiFailedJob {
  id: string;
  /// `'ocr'` or `'embed_chunk'`.
  kind: string;
  /// `dedupe_key` carries the underlying memory_id for OCR jobs
  /// (`ocr:<memory_id>:<engine>`).
  dedupeKey: string;
  attempts: number;
  lastError: string | null;
  finishedAt: string | null;
}

/// v0.4.3: response shape from `ask_recall`. The `text` is the full
/// generated answer, including in-line `[memory:<uuid>]` markers the
/// frontend rewrites into clickable citation chips.
///
/// v0.5.5: `retrievedSources` lists every memory we fed the LLM as
/// context — distinct from `citations` which is what the LLM
/// actually emitted citation markers for. For tag-pivot enumeration
/// queries the LLM may hedge and cite only one of N matches; the
/// UI renders all retrievedSources as cards so the user sees every
/// match. `tagIntent` carries the auto-tag class that triggered
/// pivot retrieval (null for general queries).
export interface AskRecallResponse {
  question: string;
  text: string;
  citations: AskRecallCitation[];
  retrievedSources: AskRecallCitation[];
  tokensGenerated: number;
  latencyMs: number;
  /// Number of chunks used as context. 0 means retrieval found
  /// nothing above the Strong-tier threshold; the LLM is instructed
  /// to say so explicitly in that case.
  contextChunks: number;
  /// v0.5.5: e.g. "license-key", "url", "phone-number". `null`
  /// when no tag-intent was detected.
  tagIntent: string | null;
}

/// v0.3.0: result row from `find_related`. The chunk fields point at
/// the best-matching slice within the parent memory so the UI can
/// render an excerpt with offsets to highlight.
///
/// v0.3.3: `score` is now the centered cosine (post-mean-subtraction)
/// rather than raw cosine. Use `strength` for human display — the raw
/// number is too misleading to render as a percent.
export interface RelatedMemoryView {
  memoryId: string;
  score: number;
  strength: "strong" | "related" | "loose";
  chunkText: string;
  chunkStart: number;
  chunkEnd: number;
}
