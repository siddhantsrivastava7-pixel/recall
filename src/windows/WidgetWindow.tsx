import { useEffect, useRef, useState } from "react";
import type { DragEvent, MouseEvent } from "react";
import { Save, Search, LayoutGrid } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getPlatformAdapters } from "@/app-runtime";
import { tauriClient } from "@/services/api/tauri-client";
import { buildQuickCaptureInput } from "@/services/capture/CaptureInputBuilder";
import { markUiConfirmationShown } from "@/services/capture/captureTelemetry";
import { useMemoryStore } from "@/stores/memoryStore";

export function WidgetWindow() {
  const { window: win } = getPlatformAdapters();
  const { create } = useMemoryStore();
  const dragTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [dropActive, setDropActive] = useState(false);

  useEffect(() => {
    // Widget window must stay fully transparent, especially on macOS WKWebView.
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
    document.getElementById("root")?.style.setProperty("background", "transparent", "important");
    void getCurrentWindow().setBackgroundColor([0, 0, 0, 0]);
  }, []);

  useEffect(() => {
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

  const startWindowDrag = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;

    const target = event.target as HTMLElement;
    if (target.closest("button")) {
      return;
    }

    event.preventDefault();
    void getCurrentWindow().startDragging();
  };

  const saveDroppedContent = async (rawContent: string) => {
    const content = rawContent.trim();
    if (!content) return;

    const ctx = await tauriClient
      .detectAppContext()
      .catch(() => ({ sourceApp: null, sourceWindow: null }));
    const result = await create(
      buildQuickCaptureInput(
        {
          title: "",
          content,
          note: "",
          projectId: "",
        },
        ctx,
      ),
      { origin: "drop-capture" },
    );

    if (result.ok && result.traceId) {
      markUiConfirmationShown(result.traceId);
    }
  };

  const handleDragOver = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    setDropActive(true);
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    setDropActive(false);
    const content = getDroppedText(event.dataTransfer);
    if (content) {
      void saveDroppedContent(content);
    }
  };

  return (
    <div
      data-tauri-drag-region
      onMouseDown={startWindowDrag}
      onDragEnter={() => setDropActive(true)}
      onDragLeave={() => setDropActive(false)}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      style={
        {
          width: "100vw",
          height: "100vh",
          background: "transparent",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          WebkitAppRegion: "drag",
        } as React.CSSProperties
      }
    >
      <div
        className="pill"
        data-tauri-drag-region
        onMouseDown={startWindowDrag}
        style={
          {
            position: "relative",
            WebkitAppRegion: "drag",
            borderColor: dropActive ? "rgba(79,124,255,0.38)" : undefined,
          } as React.CSSProperties
        }
      >
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

        <button
          className="pill-btn"
          title="Quick Capture  Ctrl+Shift+S"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => win.openQuickSave()}
        >
          <Save size={15} strokeWidth={1.8} />
        </button>

        <div className="pill-sep" />

        <button
          className="pill-btn"
          title="Search  Alt+Space"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
          onClick={() => win.openSearchOverlay()}
        >
          <Search size={15} strokeWidth={1.8} />
        </button>

        <div className="pill-sep" />

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

function getDroppedText(dataTransfer: DataTransfer) {
  const uriList = dataTransfer
    .getData("text/uri-list")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line && !line.startsWith("#"));
  if (uriList) return uriList;

  const plainText = dataTransfer.getData("text/plain").trim();
  if (plainText) return plainText;

  return null;
}
