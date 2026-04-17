import { create } from "zustand";
import type { Memory, MemoryFilters, MemoryInput } from "@/domain/types";
import { deleteMemory, duplicateMemory, updateMemory } from "@/services/memories";
import { saveCapturedMemory } from "@/services/capture/saveCapturedMemory";
import { tauriClient } from "@/services/api/tauri-client";
import {
  evaluateSearchVisibilityForMemory,
  markCaptureSuccess,
  markSearchVisibleComplete,
  markStoreUpdateComplete,
  type CaptureTraceOrigin,
} from "@/services/capture/captureTelemetry";
import { useProjectStore } from "@/stores/projectStore";

const sortMemoriesByUpdatedAt = (memories: Memory[]) =>
  memories
    .slice()
    .sort(
      (left, right) =>
        new Date(right.updatedAt || right.createdAt).getTime() -
        new Date(left.updatedAt || left.createdAt).getTime(),
    );

interface MemoryStoreState {
  memories: Memory[];
  filters: MemoryFilters;
  selectedMemoryId: string | null;
  operationMessage: string | null;
  hydrate: (memories: Memory[]) => void;
  setFilters: (filters: Partial<MemoryFilters>) => void;
  selectMemory: (memoryId: string | null) => void;
  create: (
    input: MemoryInput,
    options?: { origin?: CaptureTraceOrigin },
  ) => Promise<{ ok: boolean; error?: string; traceId?: string }>;
  update: (id: string, input: MemoryInput) => Promise<{ ok: boolean; error?: string }>;
  remove: (id: string) => Promise<{ ok: boolean; error?: string }>;
  duplicate: (id: string) => Promise<{ ok: boolean; error?: string }>;
  markOpened: (id: string) => Promise<void>;
  upsertMemory: (memory: Memory) => void;
  replaceMemory: (memory: Memory) => void;
  clearOperationMessage: () => void;
}

export const useMemoryStore = create<MemoryStoreState>((set, get) => ({
  memories: [],
  filters: { projectId: "all", sortOrder: "newest", text: "" },
  selectedMemoryId: null,
  operationMessage: null,

  hydrate(memories) {
    set({ memories: sortMemoriesByUpdatedAt(memories) });
  },

  setFilters(filters) {
    set(state => ({ filters: { ...state.filters, ...filters } }));
  },

  selectMemory(selectedMemoryId) {
    set({ selectedMemoryId });
  },

  async create(input, options) {
    const result = await saveCapturedMemory(input, {
      origin: options?.origin ?? "manual",
    });
    if (result.ok) {
      set(state => ({
        memories: sortMemoriesByUpdatedAt([
          result.memory,
          ...state.memories.filter(existing => existing.id !== result.memory.id),
        ]),
        operationMessage: "Memory saved.",
        selectedMemoryId: result.memory.id,
      }));
      markStoreUpdateComplete(result.traceId);
      const visibility = evaluateSearchVisibilityForMemory(result.memory, {
        memories: get().memories,
        projects: useProjectStore.getState().projects,
      });
      markSearchVisibleComplete(result.traceId, visibility);
      markCaptureSuccess(result.traceId);
      return { ok: true, traceId: result.traceId };
    }
    return { ok: false, error: result.error ?? "Failed to save.", traceId: result.traceId };
  },

  async update(id, input) {
    if (!input.content.trim()) return { ok: false, error: "Content is required." };
    const result = await updateMemory(id, input);
    if (result.ok && result.data) {
      const memory = result.data;
      set(state => ({
        memories: sortMemoriesByUpdatedAt(
          state.memories.map(m => m.id === id ? memory : m),
        ),
        operationMessage: "Changes saved.",
      }));
      return { ok: true };
    }
    return { ok: false, error: result.error ?? "Failed to update." };
  },

  async remove(id) {
    const result = await deleteMemory(id);
    if (result.ok) {
      set(state => ({
        memories: state.memories.filter(m => m.id !== id),
        selectedMemoryId: state.selectedMemoryId === id ? null : state.selectedMemoryId,
        operationMessage: "Memory deleted.",
      }));
      return { ok: true };
    }
    return { ok: false, error: result.error ?? "Failed to delete." };
  },

  async duplicate(id) {
    const result = await duplicateMemory(id);
    if (result.ok && result.data) {
      const memory = result.data;
      set(state => ({
        memories: sortMemoriesByUpdatedAt([
          memory,
          ...state.memories.filter(existing => existing.id !== memory.id),
        ]),
        operationMessage: "Memory duplicated.",
        selectedMemoryId: memory.id,
      }));
      return { ok: true };
    }
    return { ok: false, error: result.error ?? "Failed to duplicate." };
  },

  async markOpened(id) {
    try {
      const memory = await tauriClient.markMemoryOpened(id);
      if (!memory) return;
      set(state => ({
        memories: sortMemoriesByUpdatedAt(
          state.memories.map(m => m.id === id ? memory : m),
        ),
      }));
    } catch (error) {
      console.warn("[recall] Unable to mark memory opened", error);
    }
  },

  upsertMemory(memory) {
    set(state => ({
      memories: sortMemoriesByUpdatedAt([
        memory,
        ...state.memories.filter(existing => existing.id !== memory.id),
      ]),
    }));
  },

  replaceMemory(memory) {
    set(state => ({
      memories: sortMemoriesByUpdatedAt(
        state.memories.map(m => m.id === memory.id ? memory : m),
      ),
    }));
  },

  clearOperationMessage() {
    if (get().operationMessage) set({ operationMessage: null });
  },
}));
