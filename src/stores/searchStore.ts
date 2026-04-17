import { create } from "zustand";
import type { SearchResult, SearchSuggestion } from "@/domain/types";
import {
  getContextualSearchSuggestions,
  scoreMemoryForContext,
} from "@/services/context/ContextEngine";
import { searchMemories } from "@/services/search/searchMemories";
import { useContextStore } from "@/stores/contextStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";

interface SearchStoreState {
  query: string;
  results: SearchResult[];
  suggestions: SearchSuggestion[];
  selectedIndex: number;
  setQuery: (query: string) => void;
  moveSelection: (direction: 1 | -1) => void;
  refresh: () => void;
  reset: () => void;
}

const computeResults = (query: string) => {
  const memories = useMemoryStore.getState().memories;
  const projects = useProjectStore.getState().projects;
  const context = useContextStore.getState().getSessionContext();
  return searchMemories(memories, projects, { text: query, limit: 18 })
    .map((result) => ({
      ...result,
      score: result.score + Math.min(12, scoreMemoryForContext(result.memory, context).score / 10),
    }))
    .sort((left, right) => right.score - left.score)
    .slice(0, 12);
};

const computeSuggestions = (query: string) => {
  const memories = useMemoryStore.getState().memories;
  return getContextualSearchSuggestions(
    memories,
    query,
    useContextStore.getState().getSessionContext(),
  );
};

export const useSearchStore = create<SearchStoreState>((set, get) => ({
  query: "",
  results: [],
  suggestions: [],
  selectedIndex: 0,

  setQuery(query) {
    useContextStore.getState().recordQuery(query);
    const results = computeResults(query);
    const suggestions = computeSuggestions(query);
    set({
      query,
      results,
      suggestions,
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
    const suggestions = computeSuggestions(query);
    set({
      results,
      suggestions,
      selectedIndex: results.length === 0 ? 0 : Math.min(get().selectedIndex, results.length - 1),
    });
  },

  reset() {
    set({ query: "", results: [], suggestions: [], selectedIndex: 0 });
  },
}));
