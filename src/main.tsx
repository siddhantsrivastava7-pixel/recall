/**
 * Recall — main.tsx
 *
 * Entry point for all Tauri windows.
 * WindowRouter reads runtime.currentWindowLabel and renders
 * the correct UI for each window type.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "@/styles/globals.css";
import { WindowRouter } from "@/windows/WindowRouter";
import { bootstrapTheme } from "@/hooks/useSystemTheme";

bootstrapTheme();

const currentWindowLabel = getCurrentWindow().label;

document.documentElement.dataset.recallWindow = currentWindowLabel;
document.body.dataset.recallWindow = currentWindowLabel;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <WindowRouter />
  </StrictMode>,
);
