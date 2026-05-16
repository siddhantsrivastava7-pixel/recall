// v0.5.61 — Recall Pointer action dispatch.
//
// Every action here is a thin adapter onto an existing Recall
// pipeline command. No Pointer-specific retrieval or save logic
// exists — that's the whole point. The bridge is the feature;
// the machinery is reused.
//
//   Save        → useMemoryStore.create  (the same path Quick
//                  Save / Home capture use; link enrichment +
//                  chunk/embed fire downstream automatically)
//   Find related→ aiClient.semanticSearch  (selection as query)
//   Ask Recall  → aiClient.askRecall       (selection as anchor)
//   Probe (D)   → aiClient.semanticSearch limit 3, count only

import type { PointerSelection } from "@/domain/types";
import { aiClient } from "@/services/ai/AiClient";
import { useMemoryStore } from "@/stores/memoryStore";
import { usePointerStore } from "./pointerStore";

/// Background "Have I seen this before?" probe (Flow D). Runs
/// the moment a selection activates; cheap (limit 3) and never
/// blocks the panel from rendering. Result is just a count shown
/// as a header line — the full list is one click away via Find
/// related.
export async function probeRelatedCount(selection: PointerSelection) {
  const store = usePointerStore.getState();
  store.setBusy("probe");
  try {
    const hits = await aiClient.semanticSearch(selection.text, 3);
    // Only update if this is still the active selection — a fast
    // re-trigger shouldn't get clobbered by a stale probe.
    if (usePointerStore.getState().selection?.capturedAt === selection.capturedAt) {
      store.setRelatedCount(hits.length);
    }
  } catch {
    // Probe failure is silent — AI may be off / embeddings not
    // ready. The header line just won't render.
    store.setRelatedCount(null);
  } finally {
    if (usePointerStore.getState().busy === "probe") {
      store.setBusy(null);
    }
  }
}

/// Flow A — Save the selection as a memory. Reuses the standard
/// create path so the saved selection is a first-class memory
/// (chunked, embedded, trail-eligible) — not a separate silo.
export async function savePointerSelection(selection: PointerSelection) {
  const store = usePointerStore.getState();
  store.setBusy("save");
  store.setError(null);
  try {
    // Title: first non-empty line, capped. Source metadata
    // carried through so the memory remembers where it came
    // from (the Pointer card, the originating app).
    const firstLine =
      selection.text
        .split("\n")
        .map((l) => l.trim())
        .find((l) => l.length > 0) ?? selection.text;
    const title = firstLine.slice(0, 90);

    const result = await useMemoryStore.getState().create({
      sourceType: "manual",
      title,
      content: selection.text,
      note: null,
      projectId: null,
      url: null,
      externalId: null,
      folderPath: null,
      // Stamp the originating app so the memory's source label
      // reads e.g. "Safari" / "Code" instead of "manual". Falls
      // back to a generic Pointer tag when context was
      // unresolved.
      sourceApp: selection.sourceApp ?? "recall-pointer",
      sourceWindow: selection.sourceWindow ?? null,
    });
    if (result.ok) {
      // The create() result doesn't return the id directly;
      // newest memory in the store is the one we just made.
      const newest = useMemoryStore.getState().memories[0];
      store.setSaved(newest?.id ?? "saved");
    } else {
      store.setError(result.error ?? "Couldn't save selection.");
    }
  } catch (error) {
    store.setError(error instanceof Error ? error.message : String(error));
  } finally {
    if (usePointerStore.getState().busy === "save") {
      store.setBusy(null);
    }
  }
}

/// Flow B — Find related memories. Selection text becomes the
/// semantic query seed; results are the user's own saved
/// corpus, never anything external.
export async function findRelatedForSelection(selection: PointerSelection) {
  const store = usePointerStore.getState();
  store.setBusy("related");
  store.setError(null);
  store.setMode("related");
  try {
    const hits = await aiClient.semanticSearch(selection.text, 6);
    if (usePointerStore.getState().selection?.capturedAt === selection.capturedAt) {
      store.setRelated(hits);
      store.setRelatedCount(hits.length);
    }
  } catch (error) {
    store.setError(
      error instanceof Error ? error.message : "Couldn't search memories.",
    );
  } finally {
    if (usePointerStore.getState().busy === "related") {
      store.setBusy(null);
    }
  }
}

/// Flow C — Ask Recall about the selection. The selection is the
/// query anchor; the answer is grounded only in saved memories
/// with citations. The backend already enforces "say so when
/// there isn't enough evidence" — Pointer adds no new prompt.
export async function askRecallAboutSelection(selection: PointerSelection) {
  const store = usePointerStore.getState();
  store.setBusy("ask");
  store.setError(null);
  store.setMode("ask");
  try {
    const response = await aiClient.askRecall(selection.text);
    if (usePointerStore.getState().selection?.capturedAt === selection.capturedAt) {
      store.setAsk(response);
    }
  } catch (error) {
    store.setError(
      error instanceof Error ? error.message : "Couldn't reach Ask Recall.",
    );
  } finally {
    if (usePointerStore.getState().busy === "ask") {
      store.setBusy(null);
    }
  }
}
