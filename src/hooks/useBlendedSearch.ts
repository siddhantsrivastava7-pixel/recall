// Shared blended search hook (v0.3.8+).
//
// Until v0.3.8, each search surface in the app rolled its own filter:
// the floating bar used `useSearchStore` (full keyword + semantic
// blend), but the All Memories filter and the Dashboard quick-search
// each implemented weaker variants — naive substring on the All
// Memories side, sync-keyword-only on the Dashboard side. Users
// reasonably expected every "type-and-find" surface to be as smart
// as the strongest one.
//
// This hook is the unified entry point. Same behavior as the floating
// bar's pipeline, packaged so each surface can hold its own local
// query state without fighting `useSearchStore`'s global state
// (which carries selection cursor + suggestions specific to the
// overlay UX).
//
// Behavior:
//   * Empty query → returns empty results immediately (caller renders
//     its browse view).
//   * Non-empty query → runs the synchronous keyword ranker, returns
//     those results immediately, then fires `semantic_search` in the
//     background. When the blended response lands and still matches
//     the current query, the keyword results are replaced.
//   * Query changes mid-flight cancel the previous semantic call via
//     a version counter so a fast typist's stale response can't
//     overwrite the freshest keyword result.

import { useEffect, useRef, useState } from "react";

import type { Memory, Project, SearchResult } from "@/domain/types";
import { aiClient, type SemanticSearchHit } from "@/services/ai/AiClient";
import { searchMemories } from "@/services/search/searchMemories";

export interface UseBlendedSearchResult {
  results: SearchResult[];
  /// True between query change and the keyword pass landing
  /// (essentially never observable — keyword is sync), or while
  /// the semantic pass is in flight.
  isSemanticPending: boolean;
}

interface Options {
  limit?: number;
  /// Skip the semantic call entirely. Used by surfaces that need
  /// determinism in tests or that explicitly want keyword-only.
  keywordOnly?: boolean;
}

const DEFAULT_LIMIT = 12;

const buildHighlightFromHit = (hit: SemanticSearchHit): string[] => {
  const collapsed = hit.chunkText.replace(/\s+/g, " ").trim();
  if (collapsed.length === 0) return [];
  return [collapsed.length > 220 ? `${collapsed.slice(0, 219)}…` : collapsed];
};

const mapBlendedHitsToResults = (
  hits: SemanticSearchHit[],
  memories: Memory[],
): SearchResult[] => {
  const byId = new Map<string, Memory>();
  for (const m of memories) byId.set(m.id, m);
  const out: SearchResult[] = [];
  for (const hit of hits) {
    const memory = byId.get(hit.memoryId);
    if (!memory) continue;
    out.push({
      memory,
      // Map blended [0, 1] to keyword's ~0–100 magnitude band so the
      // score is comparable in mixed-result rendering.
      score: hit.score * 100,
      highlights: buildHighlightFromHit(hit),
      strategy: "semantic",
      providerId: "blended",
    });
  }
  return out;
};

/// Hook entry. `query` is the current search input value. The hook
/// owns its own query-version counter so rapid typing produces
/// last-write-wins semantics on the semantic side.
export function useBlendedSearch(
  query: string,
  memories: Memory[],
  projects: Project[],
  options: Options = {},
): UseBlendedSearchResult {
  const limit = options.limit ?? DEFAULT_LIMIT;
  const keywordOnly = options.keywordOnly ?? false;

  const [results, setResults] = useState<SearchResult[]>([]);
  const [isSemanticPending, setSemanticPending] = useState(false);
  const versionRef = useRef(0);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setSemanticPending(false);
      // Bump version so any in-flight semantic call from a previous
      // non-empty query lands as stale.
      versionRef.current += 1;
      return;
    }

    // Synchronous keyword pass — instant feedback.
    const keyword = searchMemories(memories, projects, {
      text: trimmed,
      limit,
    });
    setResults(keyword);

    if (keywordOnly) {
      setSemanticPending(false);
      return;
    }

    // Async semantic pass. Replace results if the query is still
    // current when the response lands and the response is non-empty.
    const myVersion = ++versionRef.current;
    setSemanticPending(true);
    aiClient
      .semanticSearch(trimmed, limit)
      .then((hits) => {
        if (myVersion !== versionRef.current) return;
        if (!hits || hits.length === 0) {
          // Adapter not ready, no embedded matches, etc. Keep keyword
          // results.
          return;
        }
        const blended = mapBlendedHitsToResults(hits, memories);
        if (blended.length === 0) return;
        setResults(blended);
      })
      .catch(() => {
        // Network glitch, adapter not ready, etc. — keyword results
        // are the safety net. Stay silent on the surface.
      })
      .finally(() => {
        if (myVersion === versionRef.current) {
          setSemanticPending(false);
        }
      });
    // We intentionally don't include `memories` and `projects` in the
    // re-run dependency list directly here — they refresh on every
    // capture and would re-fire the semantic call unnecessarily. The
    // query itself is the user's intent; memories list is a supporting
    // catalog. If a new memory lands mid-search, the next user
    // keystroke (or store-driven `refresh()`) picks it up.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, limit, keywordOnly]);

  return { results, isSemanticPending };
}
