# Recall

Recall is a private local-first desktop memory layer built with Tauri v2, React, TypeScript, Tailwind CSS, SQLite, and Zustand.

## Stack

- Tauri v2
- React 19
- TypeScript
- Tailwind CSS
- SQLite via native `sqlx`
- Zustand
- Official Tauri plugins:
  - `tauri-plugin-global-shortcut`
  - `tauri-plugin-dialog`
  - `tauri-plugin-clipboard-manager`

## Product surfaces

- Main window
- Floating widget window
- Search overlay window
- Quick save window
- License activation gate

## Run locally

```powershell
npm.cmd install
npm.cmd run tauri:dev
```

## Build frontend only

```powershell
npm.cmd run build
```

## Verify native side

```powershell
cd src-tauri
cargo check
```

## Architecture summary

### Frontend

- `src/app`
  - App entry and runtime bootstrap
- `src/components`
  - Reusable UI primitives and feature components
- `src/pages/main`
  - Main desktop pages: home, memories, projects, search, settings
- `src/pages/window`
  - Window-specific UI: widget, quick save, overlay, license gate
- `src/services`
  - Typed use cases and provider abstractions
- `src/platform`
  - Shared adapter contracts, Windows implementations, macOS placeholders, adapter factory
- `src/stores`
  - Zustand state containers

### Native

- `src-tauri/src/commands`
  - Tauri command surface for CRUD, windows, clipboard, license, import/export
- `src-tauri/src/db`
  - Migrations, seed data, repository traits, SQLite implementations
- `src-tauri/src/services`
  - Application services for memories, projects, settings, license
- `src-tauri/src/platform`
  - Platform contracts, Windows implementations, macOS stubs, factory
- `src-tauri/src/state`
  - Shared application state injected into Tauri commands

## Search design

V1 uses `SearchRuntime` plus `KeywordSearchProvider` on the frontend. UI never talks to ranking logic directly. Future semantic search can slot in through `SemanticSearchProvider`, `EmbeddingGenerator`, and `VectorIndex` contracts without rewriting the search surfaces.

## License design

The UI talks to a `LicenseService` boundary. Native-side `LocalLicenseVerifier` handles realistic mock validation and stores activation state in SQLite. A remote verifier can replace the local implementation later without changing the React screens.

## Local data

Database path is resolved from Tauri app data at runtime and displayed in Settings. Development seed data is auto-created when the database starts empty.

## Platform notes

- Windows is the primary implementation target in V1.
- Active app/window detection is implemented on Windows using Win32 APIs.
- macOS adapters are intentionally stubbed but isolated behind the same contracts.
- Shortcut defaults are registered globally via the official Tauri plugin:
  - `Alt+Space`
  - `Ctrl+Shift+S`
  - `Ctrl+Shift+O`

## Extension notes

- Search and platform extension guidance lives in `docs/architecture/search-and-platform.md`.
- macOS-specific window behavior, shortcut ergonomics, and accessibility-dependent app-context work should be completed in `src-tauri/src/platform/mac/mod.rs` and adjacent native modules, not in shared React code.
