import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

import type { BookmarkSyncSummary, Memory } from "@/domain/types";
import { applyBookmarkSyncToStores, applyCapturedMemoryToStores } from "@/services/capture/captureSync";

export function useRecallDataSyncEvents() {
  useEffect(() => {
    const disposeMemorySaved = listen<Memory>("recall://memory-saved", (event) => {
      applyCapturedMemoryToStores(event.payload);
    });

    const disposeBookmarksSynced = listen<BookmarkSyncSummary>("recall://bookmarks-synced", (event) => {
      void applyBookmarkSyncToStores(event.payload);
    });

    return () => {
      void disposeMemorySaved.then((dispose) => dispose());
      void disposeBookmarksSynced.then((dispose) => dispose());
    };
  }, []);
}
