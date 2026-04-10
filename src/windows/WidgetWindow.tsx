import { useEffect, useRef } from "react";
import { Save, Search, LayoutGrid } from "lucide-react";
import { getPlatformAdapters } from "@/app-runtime";
import { tauriClient } from "@/services/api/tauri-client";

export function WidgetWindow() {
  const { window: win } = getPlatformAdapters();
  const dragTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    // Widget window must be fully transparent — override body bg
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
  }, []);

  useEffect(() => {
    // Save position 800ms after drag ends
    const handleMouseUp = () => {
      if (dragTimeout.current) clearTimeout(dragTimeout.current);
      dragTimeout.current = setTimeout(() => {
        void tauriClient.saveWidgetPosition();
      }, 800);
    };
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mouseup", handleMouseUp);
      if (dragTimeout.current) clearTimeout(dragTimeout.current);
    };
  }, []);

  return (
    // Outer wrapper: full window, transparent, drag region so empty space drags too
    <div
      data-tauri-drag-region
      style={{
        width: "100vw",
        height: "100vh",
        background: "transparent",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        // No pointer-events on the wrapper itself so clicks fall through to pill
        WebkitAppRegion: "drag",
      } as React.CSSProperties}
    >
      {/* Pill: draggable background, but buttons are no-drag */}
      <div
        className="pill"
        data-tauri-drag-region
        style={{ position: "relative", WebkitAppRegion: "drag" } as React.CSSProperties}
      >
        {/* Glow */}
        <div
          style={{
            position: "absolute",
            inset: 0,
            borderRadius: "inherit",
            background:
              "radial-gradient(ellipse at 50% 50%, rgba(79,124,255,0.06) 0%, transparent 70%)",
            pointerEvents: "none",
          }}
        />

        {/* Quick Capture */}
        <button
          className="pill-btn"
          title="Quick Capture  Ctrl+Shift+S"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => win.openQuickSave()}
        >
          <Save size={15} strokeWidth={1.8} />
        </button>

        <div className="pill-sep" />

        {/* Search */}
        <button
          className="pill-btn"
          title="Search  Alt+Space"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => win.openSearchOverlay()}
        >
          <Search size={15} strokeWidth={1.8} />
        </button>

        <div className="pill-sep" />

        {/* Open main */}
        <button
          className="pill-btn"
          title="Open Recall  Ctrl+Shift+O"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => win.openMain()}
        >
          <LayoutGrid size={15} strokeWidth={1.8} />
        </button>
      </div>
    </div>
  );
}
