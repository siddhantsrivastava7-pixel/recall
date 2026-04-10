# Recall — UI Integration Notes

## What this folder is

This is the complete Tauri frontend (`src/` + `index.html`) that plugs
directly into the `recall-core-no-ui` architecture.

The architecture zip already contains:
- `src-tauri/` — Rust backend, SQLite, migrations, native commands
- `src/domain/`, `src/platform/`, `src/services/`, `src/stores/`, `src/hooks/`

This adds the missing pieces:
- `src/main.tsx` — new entry point
- `src/styles/globals.css` — all design tokens
- `src/windows/` — one component per Tauri window label
- `src/components/` — all UI components

---

## Window map

| Tauri window label | Component file          | What it renders                        |
|--------------------|-------------------------|----------------------------------------|
| `main`             | `windows/MainWindow.tsx`| Full app shell: sidebar + content      |
| `widget`           | `windows/WidgetWindow.tsx` | Floating pill — no decorations, transparent |
| `search-overlay`   | `windows/SearchWindow.tsx` | Keyboard search overlay, glass surface |
| `quick-save`       | `windows/QuickSaveWindow.tsx` | Quick capture panel, glass surface |

`WindowRouter` bootstraps the app, then reads `runtime.currentWindowLabel`
from `bootstrap_app` to decide which window to render.

---

## How the bootstrap flow works

```
WindowRouter mounts
  → useAppStore.bootstrap()
    → tauriClient.bootstrap()  [invoke("bootstrap_app")]
      → configureRuntimePlatform(payload.runtime.platform)
      → useMemoryStore.hydrate(payload.memories)
      → useProjectStore.hydrate(payload.projects)
      → useSettingsStore.hydrate(payload.settings, payload.shortcuts, payload.license)
      → set({ runtime, initialized: true })
  → WindowRouter reads runtime.currentWindowLabel
  → renders correct window
```

---

## Component → Architecture mapping

| UI Component              | Architecture store / service                       |
|---------------------------|----------------------------------------------------|
| `WidgetWindow`            | `platform.window.openQuickSave/openSearchOverlay/openMain` |
| `SearchWindow`            | `useSearchStore` → `searchMemories` (keyword provider) |
| `QuickSaveWindow`         | `tauriClient.readClipboardText`, `tauriClient.detectAppContext`, `useMemoryStore.create` |
| `Dashboard`               | `useMemoryStore`, `useProjectStore`, `tauriClient.openSearchOverlay` |
| `MemoriesView`            | `useMemoryStore` (filter, sort, select)            |
| `MemoryCard`              | `useMemoryStore.remove`, `duplicate`               |
| `MemoryDetail`            | `useMemoryStore.update`, `remove`                  |
| `ProjectsView`            | `useProjectStore` (CRUD)                           |
| `SettingsView/GeneralTab` | `useSettingsStore.updateSettings`, `tauriClient.exportData/importData/clearAllData` |
| `SettingsView/BookmarksTab` | `tauriClient.syncBookmarksNow`                   |
| `SettingsView/LicenseTab` | `useSettingsStore.activateLicense/deactivateLicense` |

---

## Tauri window config needed

Add these to `src-tauri/tauri.conf.json` → `app.windows`:

```json
{
  "label": "widget",
  "title": "Recall Widget",
  "width": 268,
  "height": 60,
  "transparent": true,
  "decorations": false,
  "alwaysOnTop": true,
  "resizable": false,
  "skipTaskbar": true,
  "visible": false
},
{
  "label": "search-overlay",
  "title": "Recall Search",
  "width": 680,
  "height": 560,
  "transparent": true,
  "decorations": false,
  "center": true,
  "resizable": false,
  "skipTaskbar": true,
  "visible": false
},
{
  "label": "quick-save",
  "title": "Recall Quick Save",
  "width": 560,
  "height": 380,
  "transparent": true,
  "decorations": false,
  "center": true,
  "resizable": false,
  "skipTaskbar": true,
  "visible": false
}
```

---

## Design rules strictly followed

- **No glass on memory cards** — `background: #111827` solid always
- **Glass only on overlays** — search + quick-save windows
- **Floating pill** — transparent window, no rectangular wrapper
- **64px icon sidebar** — always-on sidebar in main window
- **Accent: #4F7CFF** — used for active states, accent lines, primary buttons
- **Base bg gradient** — `135deg, #0B0F1A → #0E1424 → #0B1020`
- **Card hover** — `translateY(-3px)` with `border-color` lift
- **Resurfaced cards** — 2px top gradient border in accent color

---

## Missing pieces / notes

1. **Window event listeners** — `MainWindow` has a stub where you'd subscribe
   to Tauri window events (e.g., `open-memory-in-main`) to deep-link from
   search results directly to a memory detail.

2. **Global shortcuts** — shortcuts are already registered in the Rust layer
   (`src-tauri/src/platform/`). The frontend just responds to window
   opens/closes — no JS shortcut registration needed.

3. **Drag region** — `data-tauri-drag-region` is set on the pill wrapper.
   For the main window, add a custom titlebar if you want drag behavior.

4. **Semantic search** — `SearchWindow` uses `KeywordSearchProvider` from
   the architecture. `SemanticSearchProvider` + `EmbeddingGenerator` are
   in the architecture and can be connected once an embedding model is ready.

5. **`src/features/` and `src/infrastructure/`** — these exist in the
   architecture but had permission issues in the zip. Check them for any
   additional hooks that should be integrated.
