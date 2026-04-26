import { create } from "zustand";

import type { Memory } from "@/domain/types";
import {
  buildSessionContext,
  type SessionContext,
} from "@/services/context/ContextEngine";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";

const MAX_RECENT_QUERIES = 12;
const MAX_RECENT_MEMORIES = 24;

interface ContextStoreState {
  recentQueries: string[];
  recentlyOpenedMemoryIds: string[];
  recentCaptureIds: string[];
  recordQuery: (query: string) => void;
  recordMemoryOpened: (memory: Memory) => void;
  recordCapture: (memory: Memory) => void;
  resetSession: () => void;
  getSessionContext: () => SessionContext;
}

const prependUnique = (values: string[], value: string, limit: number) => [
  value,
  ...values.filter((candidate) => candidate !== value),
].slice(0, limit);

export const useContextStore = create<ContextStoreState>((set, get) => ({
  recentQueries: [],
  recentlyOpenedMemoryIds: [],
  recentCaptureIds: [],

  recordQuery(query) {
    const normalized = query.trim().replace(/\s+/g, " ");
    if (normalized.length < 3) return;
    set((state) => ({
      recentQueries: prependUnique(state.recentQueries, normalized, MAX_RECENT_QUERIES),
    }));
  },

  recordMemoryOpened(memory) {
    set((state) => ({
      recentlyOpenedMemoryIds: prependUnique(
        state.recentlyOpenedMemoryIds,
        memory.id,
        MAX_RECENT_MEMORIES,
      ),
    }));
  },

  recordCapture(memory) {
    set((state) => ({
      recentCaptureIds: prependUnique(state.recentCaptureIds, memory.id, MAX_RECENT_MEMORIES),
    }));
  },

  resetSession() {
    set({
      recentQueries: [],
      recentlyOpenedMemoryIds: [],
      recentCaptureIds: [],
    });
  },

  getSessionContext() {
    return buildSessionContext(useMemoryStore.getState().memories, {
      recentQueries: get().recentQueries,
      recentlyOpenedMemoryIds: get().recentlyOpenedMemoryIds,
      recentCaptureIds: get().recentCaptureIds,
      activeProjectId: useProjectStore.getState().activeProjectId,
    });
  },
}));
