/**
 * v0.5.60 — source scope chips originally introduced on the All
 * Memories list (`MemoriesView.tsx`). v0.5.61 lifts them out of
 * MemoriesView so AskView can reuse the same taxonomy without it
 * drifting out of sync, and adds a wire-format mapping for the
 * backend `SourceAppFilter` Tauri command param.
 *
 * "memories" is the catch-all for everything that isn't one of
 * the other four buckets — clipboard captures, manual notes,
 * screenshots, browser bookmarks, etc. The set is
 * **negative-defined** rather than enumerated because new
 * source_app values get added more often than the chip set
 * changes; the catch-all naturally absorbs them.
 */

import type { Memory } from "./types";

/// The chip taxonomy. Adding a new chip is a four-step diff:
/// add the variant here, add to SOURCE_SCOPE_OPTIONS, extend
/// `memoryMatchesScope`, extend `sourceScopeToBackendFilter`.
/// TypeScript's exhaustiveness checking on the switches will
/// flag any spot you miss.
export type SourceScope = "all" | "memories" | "files" | "folders" | "twitter";

/// Source-of-truth list for chip rendering. Each surface (the
/// All Memories list, AskView, future search overlay) imports
/// this and renders its own icon binding — keeping icon imports
/// out of `domain/` so the domain layer stays free of UI deps.
export const SOURCE_SCOPE_OPTIONS: ReadonlyArray<{
  value: SourceScope;
  label: string;
}> = [
  { value: "all", label: "All" },
  { value: "memories", label: "Memories" },
  { value: "files", label: "Files" },
  { value: "folders", label: "Folders" },
  { value: "twitter", label: "Twitter" },
];

/// True when the memory belongs to the given scope. Lifted
/// verbatim from the v0.5.60 implementation in MemoriesView.tsx
/// so existing list-side semantics are preserved.
export function memoryMatchesScope(memory: Memory, scope: SourceScope): boolean {
  const app = memory.sourceApp;
  switch (scope) {
    case "all":
      return true;
    case "files":
      return app === "file";
    case "folders":
      return app === "folder";
    case "twitter":
      return app === "twitter";
    case "memories":
      return app !== "file" && app !== "folder" && app !== "twitter";
  }
}

/// v0.5.61: wire format the backend `ask_recall` command expects
/// for its optional `sourceAppFilter` param. Externally tagged
/// to match the Rust serde shape (`SourceAppFilter` enum with
/// `#[serde(rename_all = "snake_case")]`).
///
///   * `{ include: ["twitter"] }` → keep only memories whose
///     `source_app` is in the list (case-insensitive on the
///     backend).
///   * `{ exclude: [...] }` → keep everything except those
///     source_app values. Used by the negative-defined
///     "Memories" chip.
export type SourceAppFilter =
  | { include: string[] }
  | { exclude: string[] };

/// Map a chip selection to the backend wire filter. Returns
/// `undefined` for "all" so the IPC payload can omit the filter
/// altogether — the backend treats `None` as "no scoping",
/// preserving the v0.5.60 retrieval behavior bit-for-bit.
///
/// The "memories" mapping is negative-defined to match the
/// frontend's `memoryMatchesScope`. Keeping the two in lock-step
/// matters: a chip that filters one way locally and a different
/// way over the wire would surface a memory in the All Memories
/// list but hide it from Ask Recall (or vice versa) — confusing.
export function sourceScopeToBackendFilter(scope: SourceScope): SourceAppFilter | undefined {
  switch (scope) {
    case "all":
      return undefined;
    case "files":
      return { include: ["file"] };
    case "folders":
      return { include: ["folder"] };
    case "twitter":
      return { include: ["twitter"] };
    case "memories":
      return { exclude: ["file", "folder", "twitter"] };
  }
}
