import { create } from "zustand";
import type { Memory, SearchResult, SearchSuggestion } from "@/domain/types";
import {
  getContextualSearchSuggestions,
  scoreMemoryForContext,
} from "@/services/context/ContextEngine";
import { searchMemories } from "@/services/search/searchMemories";
import { aiClient, type SemanticSearchHit } from "@/services/ai/AiClient";
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

// v0.3.3: track the in-flight semantic call so a fast typist's stale
// response doesn't clobber the freshest keyword result. Each setQuery
// increments the version; only the matching version's response is
// allowed to replace the results.
let activeQueryVersion = 0;

const buildHighlightFromHit = (hit: SemanticSearchHit): string[] => {
  // Trim the matched chunk to a single-line excerpt. The detail view
  // uses chunkStart/chunkEnd for precise highlighting; this is just
  // the search-result preview text.
  const collapsed = hit.chunkText.replace(/\s+/g, " ").trim();
  if (collapsed.length === 0) return [];
  return [collapsed.length > 220 ? `${collapsed.slice(0, 219)}…` : collapsed];
};

const blendedHitsToResults = (hits: SemanticSearchHit[]): SearchResult[] => {
  const memories = useMemoryStore.getState().memories;
  const byId = new Map<string, Memory>();
  for (const m of memories) byId.set(m.id, m);
  const out: SearchResult[] = [];
  for (const hit of hits) {
    const memory = byId.get(hit.memoryId);
    if (!memory) continue;
    out.push({
      memory,
      // Map the [0, 1] blended score to the same magnitude band the
      // keyword path uses (~0–100) so result mixing across other
      // surfaces stays sane.
      score: hit.score * 100,
      highlights: buildHighlightFromHit(hit),
      strategy: "semantic",
      providerId: "blended",
    });
  }
  return out;
};

/// Trigger the Rust-side blended search and, if it returns results
/// that still match the current query version, replace the store's
/// results with them. Empty / errored returns silently leave the
/// keyword results in place so the user sees something either way.
const tryBlendedSearch = (query: string, version: number) => {
  if (query.trim().length < 2) return;
  void aiClient
    .semanticSearch(query, 12)
    .then((hits) => {
      if (version !== activeQueryVersion) return;
      if (!hits || hits.length === 0) return;
      const results = blendedHitsToResults(hits);
      if (results.length === 0) return;
      useSearchStore.setState((state) => ({
        results,
        selectedIndex:
          results.length === 0 ? 0 : Math.min(state.selectedIndex, results.length - 1),
      }));
    })
    .catch(() => {
      // Adapter not ready, network glitch, etc. — keyword results stay.
    });
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
    // Kick off the blended path. If it returns useful results, the
    // store updates again; if it doesn't, the keyword results stay.
    const version = ++activeQueryVersion;
    tryBlendedSearch(query, version);
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
    const version = ++activeQueryVersion;
    tryBlendedSearch(query, version);
  },

  reset() {
    set({ query: "", results: [], suggestions: [], selectedIndex: 0 });
  },
}));
