import { create } from "zustand";
import type { SearchResult } from "@/domain/types";
import { searchMemories } from "@/services/search/searchMemories";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";

interface SearchStoreState {
  query: string;
  results: SearchResult[];
  selectedIndex: number;
  setQuery: (query: string) => void;
  moveSelection: (direction: 1 | -1) => void;
  refresh: () => void;
  reset: () => void;
}

const computeResults = (query: string) => {
  const memories = useMemoryStore.getState().memories;
  const projects = useProjectStore.getState().projects;
  return searchMemories(memories, projects, { text: query, limit: 12 });
};

export const useSearchStore = create<SearchStoreState>((set, get) => ({
  query: "",
  results: [],
  selectedIndex: 0,

  setQuery(query) {
    const results = computeResults(query);
    set({
      query,
      results,
      selectedIndex: results.length === 0 ? 0 : Math.min(get().selectedIndex, results.length - 1),
    });
  },

  moveSelection(direction) {
    const { results, selectedIndex } = get();
    if (results.length === 0) return;
    set({ selectedIndex: (selectedIndex + direction + results.length) % results.length });
  },

  refresh() {
    const { query } = get();
    const results = computeResults(query);
    set({
      results,
      selectedIndex: results.length === 0 ? 0 : Math.min(get().selectedIndex, results.length - 1),
    });
  },

  reset() {
    set({ query: "", results: [], selectedIndex: 0 });
  },
}));
