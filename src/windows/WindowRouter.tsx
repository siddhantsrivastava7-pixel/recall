/**
 * WindowRouter
 *
 * Tauri runs 4 separate webview windows:
 *   "main"           → full dashboard shell
 *   "widget"         → floating pill (always-on-top, transparent)
 *   "search-overlay" → keyboard search overlay
 *   "quick-save"     → quick capture panel
 *
 * On startup each window calls bootstrap_app → returns RuntimeInfo
 * with currentWindowLabel so we know which UI to render.
 */

import { useEffect, useState } from "react";
import { useAppStore } from "@/stores/appStore";
import { MainWindow } from "./MainWindow";
import { WidgetWindow } from "./WidgetWindow";
import { SearchWindow } from "./SearchWindow";
import { QuickSaveWindow } from "./QuickSaveWindow";
import type { WindowLabel } from "@/domain/types";

export function WindowRouter() {
  const { bootstrap, isBootstrapping, initialized, error, runtime } = useAppStore();

  useEffect(() => {
    void bootstrap();
  }, []);

  if (isBootstrapping) {
    return <BootSplash />;
  }

  if (error) {
    return <ErrorScreen message={error} />;
  }

  const label = runtime?.currentWindowLabel ?? "main";

  switch (label as WindowLabel) {
    case "widget":         return <WidgetWindow />;
    case "search-overlay": return <SearchWindow />;
    case "quick-save":     return <QuickSaveWindow />;
    case "main":
    default:               return <MainWindow />;
  }
}

function BootSplash() {
  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        background: "linear-gradient(135deg, #0B0F1A 0%, #0E1424 60%, #0B1020 100%)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 16,
      }}
    >
      <div
        style={{
          width: 40,
          height: 40,
          borderRadius: "50%",
          background: "rgba(79,124,255,0.15)",
          border: "2px solid rgba(79,124,255,0.45)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          animation: "breathe 2.8s ease-in-out infinite",
        }}
      >
        <div style={{ width: 14, height: 14, borderRadius: "50%", background: "#4F7CFF" }} />
      </div>
      <span style={{ fontSize: 12, color: "rgba(255,255,255,0.28)", letterSpacing: "0.08em" }}>
        Starting Recall…
      </span>
    </div>
  );
}

function ErrorScreen({ message }: { message: string }) {
  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        background: "#0B0F1A",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 16,
        padding: 40,
      }}
    >
      <span style={{ fontSize: 13, fontWeight: 600, color: "var(--danger)" }}>
        Unable to start Recall
      </span>
      <span
        style={{
          fontSize: 12,
          color: "rgba(255,255,255,0.4)",
          textAlign: "center",
          maxWidth: 520,
          wordBreak: "break-word",
          fontFamily: "monospace",
          background: "rgba(255,255,255,0.05)",
          padding: "12px 16px",
          borderRadius: 8,
        }}
      >
        {message}
      </span>
    </div>
  );
}
