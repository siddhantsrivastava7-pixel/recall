# Recall Core Handoff

This folder contains the working Recall architecture without the current UI layer.

## Included

- `src-tauri/`
  - Tauri app setup
  - native window management
  - global shortcuts
  - SQLite repositories, migrations, seed data
  - bookmark ingestion and sync
  - platform adapters and commands
- `src/domain/`
  - shared types and domain helpers
- `src/platform/`
  - frontend platform adapter contracts and implementations
- `src/services/`
  - app-facing service layer, search abstractions, Tauri client
- `src/stores/`
  - Zustand state and flows
- `src/hooks/`
  - boot logic
- `src/utils/`, `src/lib/`, `src/infrastructure/`, `src/features/`
  - supporting shared logic used by the architecture
- root config files
  - `package.json`
  - `vite.config.ts`
  - `tailwind.config.ts`
  - `tsconfig*.json`

## Intentionally Removed

- `src/components/`
- `src/layouts/`
- `src/pages/`
- `src/styles/`
- current `src/main.tsx`
- current `src/app/App.tsx`

Those files were the active UI layer that you said you want to replace.

## Main Integration Points For Your UI

- bootstrap app state:
  - `src/hooks/useBootApp.ts`
  - `src/stores/appStore.ts`
- open windows / shortcuts / native actions:
  - `src/platform/contracts/WindowAdapter.ts`
  - `src/platform/windows/WindowsWindowAdapter.ts`
  - `src/services/api/tauri-client.ts`
- memories:
  - `src/stores/memoryStore.ts`
- projects:
  - `src/stores/projectStore.ts`
- settings + license:
  - `src/stores/settingsStore.ts`
- search:
  - `src/stores/searchStore.ts`
  - `src/services/search/`

## What You Need To Recreate

- a new `src/main.tsx`
- a new top-level app router / window switcher
- your replacement UI components for:
  - widget / floating pill
  - search overlay
  - quick capture
  - main app shell

## Note

This handoff folder is meant as a clean architecture base, not a ready-to-run build by itself until you add your replacement UI entry files.
