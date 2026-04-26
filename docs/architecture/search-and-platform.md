# Search And Platform Extension Notes

This document describes the intentional extension seams for future semantic search and better macOS support.

## Current search behavior

Current product behavior is unchanged.

- UI calls `searchMemories(...)`
- `searchMemories(...)` delegates to `recallSearchRuntime`
- `recallSearchRuntime` currently uses only `KeywordSearchProvider`
- Ranking still uses the same keyword scoring behavior as before

Relevant files:

- `src/services/search/searchMemories.ts`
- `src/services/search/SearchRuntime.ts`
- `src/services/search/KeywordSearchProvider.ts`

## Future semantic search seam

The app now has explicit local-first extension points for a semantic stack:

- `src/services/search/SemanticSearchProvider.ts`
  - contract for semantic retrieval
- `src/services/search/EmbeddingGenerator.ts`
  - contract for local embedding generation
- `src/services/search/VectorIndex.ts`
  - contract for local vector storage and retrieval
- `src/services/search/SearchIndexCoordinator.ts`
  - coordination point for reindexing and deletion flows

Recommended future flow:

1. Implement a local embedding generator.
2. Implement a local vector index.
3. Add a concrete `SemanticSearchProvider`.
4. Register that provider in `SearchRuntime`.
5. Keep the UI unchanged by preserving `searchMemories(...)` as the app-facing entry point.

## Why this keeps the app local-first

- No cloud dependency is assumed by any of the new interfaces.
- Embeddings can be generated locally.
- Vector retrieval can remain entirely on-device.
- Search UI does not need to know whether retrieval is keyword, semantic, or hybrid.

## macOS platform seam

All OS-specific window, shortcut, and active-app behavior should remain inside:

- `src-tauri/src/platform/contracts.rs`
- `src-tauri/src/platform/factory.rs`
- `src-tauri/src/platform/mac/mod.rs`
- `src-tauri/src/platform/windows/mod.rs`

Shared UI should continue to call shared services and adapter facades only.

## macOS follow-up checklist

Complete these in `src-tauri/src/platform/mac/mod.rs` and related native modules later:

1. Window behavior
   - Use macOS-appropriate presentation for the floating widget and search overlay.
   - Revisit focus, hide/show, activation policy, and window levels.

2. Shortcuts
   - Confirm default shortcut ergonomics and modifier conventions on macOS.
   - Validate platform-specific behavior for global shortcut registration and conflicts.

3. App context detection
   - Add native frontmost-app detection.
   - Add active window-title detection where allowed.
   - Handle Accessibility permission requirements explicitly.

4. Permissions and packaging
   - Document any required accessibility prompts.
   - Revisit notarization, hardened runtime, and entitlements if distribution moves further on macOS.

## Guardrail

Do not move platform checks into React components. If macOS needs different behavior later, extend the native adapter implementations and keep shared product logic untouched.
