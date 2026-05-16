// v0.5.61 — Recall Pointer store.
//
// Holds the transient state of one Pointer session: the captured
// selection, which mode the compact panel is in, and the results
// of the memory-aware actions. Everything resets when the overlay
// closes — Pointer is a momentary bridge, not a persistent view.
//
// The store deliberately owns NO retrieval logic. Save / Find
// related / Ask Recall all delegate to the existing pipeline
// commands via pointerActions.ts. The differentiator is the
// bridge, not new machinery.

import { create } from "zustand";
import type { PointerSelection } from "@/domain/types";
import type {
  SemanticSearchHit,
  AskRecallResponse,
} from "@/services/ai/AiClient";

/// `actions` is the resting state: selection preview + the three
/// buttons. `related` / `ask` are the expanded result states.
/// There is no separate "have I seen this before" mode — that
/// signal is computed in the background the moment a selection
/// activates and shown as a header line above the actions.
export type PointerMode = "actions" | "related" | "ask";

interface PointerStoreState {
  selection: PointerSelection | null;
  mode: PointerMode;

  /// Flow D — background probe count. null = not yet computed,
  /// 0 = computed and nothing matched, N = N related memories.
  relatedCount: number | null;

  relatedResults: SemanticSearchHit[];
  ask: AskRecallResponse | null;

  /// Which action is in flight (disables the others + shows a
  /// spinner on that one). null = idle.
  busy: "save" | "related" | "ask" | "probe" | null;

  /// Set after a successful Save so the panel can confirm
  /// ("Saved ✓") and offer "Open in Recall".
  savedMemoryId: string | null;

  /// Non-fatal action error surfaced inline in the panel.
  errorMessage: string | null;

  activate: (selection: PointerSelection) => void;
  setMode: (mode: PointerMode) => void;
  setRelatedCount: (count: number | null) => void;
  setRelated: (results: SemanticSearchHit[]) => void;
  setAsk: (response: AskRecallResponse | null) => void;
  setBusy: (busy: PointerStoreState["busy"]) => void;
  setSaved: (memoryId: string | null) => void;
  setError: (message: string | null) => void;
  reset: () => void;
}

const EMPTY = {
  selection: null,
  mode: "actions" as PointerMode,
  relatedCount: null,
  relatedResults: [],
  ask: null,
  busy: null,
  savedMemoryId: null,
  errorMessage: null,
};

export const usePointerStore = create<PointerStoreState>((set) => ({
  ...EMPTY,

  activate(selection) {
    // Fresh session — clear any prior results so a re-trigger
    // never shows stale matches from an earlier selection.
    set({ ...EMPTY, selection, mode: "actions" });
  },
  setMode(mode) {
    set({ mode });
  },
  setRelatedCount(relatedCount) {
    set({ relatedCount });
  },
  setRelated(relatedResults) {
    set({ relatedResults });
  },
  setAsk(ask) {
    set({ ask });
  },
  setBusy(busy) {
    set({ busy });
  },
  setSaved(savedMemoryId) {
    set({ savedMemoryId });
  },
  setError(errorMessage) {
    set({ errorMessage });
  },
  reset() {
    set({ ...EMPTY });
  },
}));
