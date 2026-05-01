import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

import type { BookmarkSyncSummary, Memory, PairingInfo } from "@/domain/types";
import { applyBookmarkSyncToStores, applyCapturedMemoryToStores } from "@/services/capture/captureSync";
import { usePairingStore } from "@/features/pairing/pairingStore";
import { tauriClient } from "@/services/api/tauri-client";
import { useMemoryStore } from "@/stores/memoryStore";
import { useSearchStore } from "@/stores/searchStore";

export function useRecallDataSyncEvents() {
  useEffect(() => {
    const disposeMemorySaved = listen<Memory>("recall://memory-saved", (event) => {
      applyCapturedMemoryToStores(event.payload);
    });

    // v0.2.0+: AI scheduler emits this when an OCR pass finishes (success
    // or failure). Refetch the row so `ocr_text` / `ocr_status` flow into
    // the store and any open detail pane re-renders the status pill.
    const disposeOcrUpdated = listen<{ memoryId: string }>(
      "recall://memory-ocr-updated",
      async (event) => {
        const id = event.payload?.memoryId;
        if (!id) return;
        try {
          const fresh = await tauriClient.getMemory(id);
          if (fresh) {
            useMemoryStore.getState().upsertMemory(fresh);
            useSearchStore.getState().refresh();
          }
        } catch {
          // Best-effort UI sync — search keeps working off the cached
          // copy until the next list refresh, so swallowing this error
          // is safe.
        }
      },
    );

    // v0.3.0+: scheduler emits this when a chunk's embedding lands.
    // Refetch the parent memory so `embedding_generated_at` updates,
    // which is the dependency the RelatedMemories component uses to
    // trigger a fresh `find_related` call.
    const disposeEmbeddingUpdated = listen<{ memoryId: string }>(
      "recall://memory-embedding-updated",
      async (event) => {
        const id = event.payload?.memoryId;
        if (!id) return;
        try {
          const fresh = await tauriClient.getMemory(id);
          if (fresh) {
            useMemoryStore.getState().upsertMemory(fresh);
          }
        } catch {
          // Best-effort.
        }
      },
    );

    const disposeBookmarksSynced = listen<BookmarkSyncSummary>("recall://bookmarks-synced", (event) => {
      void applyBookmarkSyncToStores(event.payload);
    });

    const disposePairingUpdated = listen<PairingInfo>("recall://pairing-updated", (event) => {
      usePairingStore.getState().applyPairingInfo(event.payload);
    });

    return () => {
      void disposeMemorySaved.then((dispose) => dispose());
      void disposeOcrUpdated.then((dispose) => dispose());
      void disposeEmbeddingUpdated.then((dispose) => dispose());
      void disposeBookmarksSynced.then((dispose) => dispose());
      void disposePairingUpdated.then((dispose) => dispose());
    };
  }, []);
}
