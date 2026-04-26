import { useEffect, useState } from "react";

export type ThemeMode = "light" | "dark" | "auto";

const STORAGE_KEY = "recall-theme";
const THEME_QUERY = "(prefers-color-scheme: dark)";

const readStoredMode = (): ThemeMode => {
  if (typeof window === "undefined") return "auto";
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "auto") {
    return stored;
  }
  return "auto";
};

const applyMode = (mode: ThemeMode) => {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (mode === "auto") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = mode;
  }
};

/** Theme controller. Persists user choice (light/dark/auto) and writes
 *  `data-theme` on <html>. Auto removes the attribute so the
 *  `prefers-color-scheme` media query in globals.css drives the palette. */
export const useThemeMode = () => {
  const [mode, setModeState] = useState<ThemeMode>(() => readStoredMode());

  useEffect(() => {
    applyMode(mode);
    if (typeof window !== "undefined") {
      window.localStorage.setItem(STORAGE_KEY, mode);
    }
  }, [mode]);

  return { mode, setMode: setModeState };
};

/** Apply stored mode synchronously on app boot before React renders, and
 *  install cross-window listeners (storage events from other Tauri windows
 *  + OS prefers-color-scheme changes when in auto mode) so the theme stays
 *  in sync across the main, widget, search-overlay, and quick-save windows. */
export const bootstrapTheme = () => {
  applyMode(readStoredMode());

  if (typeof window === "undefined") return;

  window.addEventListener("storage", (event) => {
    if (event.key !== STORAGE_KEY) return;
    const next = event.newValue;
    if (next === "light" || next === "dark" || next === "auto") {
      applyMode(next);
    } else if (next === null) {
      applyMode("auto");
    }
  });

  if (typeof window.matchMedia === "function") {
    const media = window.matchMedia(THEME_QUERY);
    const handler = () => {
      if (readStoredMode() === "auto") applyMode("auto");
    };
    if (typeof media.addEventListener === "function") {
      media.addEventListener("change", handler);
    } else {
      media.addListener(handler);
    }
  }
};
