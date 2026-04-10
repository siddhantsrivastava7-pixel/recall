import type {
  BookmarkBrowser,
  BookmarkSourceStatus,
  BookmarkSyncSummary,
  Memory,
  ServiceResult,
} from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";
import { applyBookmarkSyncToStores } from "@/services/capture/captureSync";
import {
  evaluateSearchVisibilityForMemories,
  markCaptureFailure,
  markCaptureSuccess,
  markDbWriteComplete,
  markSearchVisibleComplete,
  markStoreUpdateComplete,
  startCaptureTrace,
  type CaptureLatencyThresholds,
  type CaptureRankThresholds,
} from "@/services/capture/captureTelemetry";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";

export const listBookmarkSources = async () => {
  try {
    const sources = await tauriClient.listBookmarkSources();
    return { ok: true, data: sources } satisfies ServiceResult<BookmarkSourceStatus[]>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to inspect bookmark sources.",
    } satisfies ServiceResult<BookmarkSourceStatus[]>;
  }
};

interface BookmarkSyncOptions {
  latencyThresholds?: Partial<CaptureLatencyThresholds>;
  rankThresholds?: Partial<CaptureRankThresholds>;
}

interface BookmarkSyncServiceResult extends ServiceResult<BookmarkSyncSummary> {
  traceId?: string;
  importedMemories?: Memory[];
}

const completeBookmarkSync = async (
  request: () => Promise<BookmarkSyncSummary>,
  options: BookmarkSyncOptions = {},
): Promise<BookmarkSyncServiceResult> => {
  const traceId = startCaptureTrace({
    origin: "bookmark-import",
    sourceType: "bookmark",
    latencyThresholds: options.latencyThresholds,
  });
  const existingIds = new Set(useMemoryStore.getState().memories.map((memory) => memory.id));

  try {
    const summary = await request();
    markDbWriteComplete(traceId, summary.syncedAt ?? traceId);
    await applyBookmarkSyncToStores(summary);
    markStoreUpdateComplete(traceId);

    const importedMemories = useMemoryStore
      .getState()
      .memories.filter(
        (memory) => memory.sourceType === "bookmark" && !existingIds.has(memory.id),
      );

    markSearchVisibleComplete(
      traceId,
      evaluateSearchVisibilityForMemories(importedMemories, {
        memories: useMemoryStore.getState().memories,
        projects: useProjectStore.getState().projects,
        rankThresholds: options.rankThresholds,
      }),
    );
    markCaptureSuccess(traceId);

    return { ok: true, data: summary, traceId, importedMemories };
  } catch (error) {
    markCaptureFailure(
      traceId,
      error instanceof Error ? error.message : "Unable to sync bookmarks.",
    );
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to import bookmarks.",
      traceId,
    };
  }
};

export const importBookmarks = async (
  browsers: BookmarkBrowser[],
  options?: BookmarkSyncOptions,
) => completeBookmarkSync(() => tauriClient.importBookmarks(browsers), options);

export const syncBookmarksNow = async (options?: BookmarkSyncOptions) =>
  completeBookmarkSync(() => tauriClient.syncBookmarksNow(), options);
