import type { BookmarkSyncSummary, Memory } from "@/domain/types";
import { useAppStore } from "@/stores/appStore";
import { useContextStore } from "@/stores/contextStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useSearchStore } from "@/stores/searchStore";

let lastAppliedBookmarkSyncToken: string | null = null;

export const applyCapturedMemoryToStores = (memory: Memory) => {
  useMemoryStore.getState().upsertMemory(memory);
  useContextStore.getState().recordCapture(memory);
  useSearchStore.getState().refresh();
};

export const applyBookmarkSyncToStores = async (
  summary?: Pick<BookmarkSyncSummary, "syncedAt"> | null,
) => {
  const token = summary?.syncedAt ?? null;
  if (token && token === lastAppliedBookmarkSyncToken) {
    return false;
  }

  if (token) {
    lastAppliedBookmarkSyncToken = token;
  }

  await useAppStore.getState().hydrateFromImport();
  useSearchStore.getState().refresh();
  return true;
};

export const resetCaptureSyncState = () => {
  lastAppliedBookmarkSyncToken = null;
};
