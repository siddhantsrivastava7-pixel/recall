/**
 * Recall — main.tsx
 *
 * Entry point for all Tauri windows.
 * WindowRouter reads runtime.currentWindowLabel and renders
 * the correct UI for each window type.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@/styles/globals.css";
import { WindowRouter } from "@/windows/WindowRouter";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <WindowRouter />
  </StrictMode>,
);
