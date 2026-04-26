import { useEffect, useRef, useState } from "react";
import { TauriEvent } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronDown, Plus, Save, X } from "lucide-react";

import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { tauriClient } from "@/services/api/tauri-client";
import { buildQuickCaptureInput } from "@/services/capture/CaptureInputBuilder";
import { markUiConfirmationShown } from "@/services/capture/captureTelemetry";

export function QuickSaveWindow() {
  const [content, setContent] = useState("");
  const [title, setTitle] = useState("");
  const [note, setNote] = useState("");
  const [projectId, setProjectId] = useState("");
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const shouldRefreshOnFocusRef = useRef(true);

  const { create } = useMemoryStore();
  const { projects } = useProjectStore();

  useEffect(() => {
    async function refreshDraftFromClipboard() {
      setTitle("");
      setNote("");
      setProjectId("");
      setExpanded(false);
      setSaving(false);
      setSaved(false);

      const clipboardText = await tauriClient.readClipboardText().catch(() => null);
      setContent(clipboardText?.trim() ?? "");
      shouldRefreshOnFocusRef.current = false;
      window.setTimeout(() => textareaRef.current?.focus(), 60);
    }

    // Force the underlying NSWindow / HWND background fully transparent. The
    // macOSPrivateApi feature in v0.1.19 lets Tauri actually honor this on
    // macOS; this stays as a defense-in-depth signal to the webview itself.
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
    document.getElementById("root")?.style.setProperty("background", "transparent", "important");
    void getCurrentWindow().setBackgroundColor([0, 0, 0, 0]);

    void refreshDraftFromClipboard();

    let unlistenFocus: (() => void) | undefined;
    void getCurrentWindow()
      .listen(TauriEvent.WINDOW_FOCUS, () => {
        if (!shouldRefreshOnFocusRef.current) {
          return;
        }
        void refreshDraftFromClipboard();
      })
      .then((unlisten) => {
        unlistenFocus = unlisten;
      })
      .catch(() => {});

    return () => {
      unlistenFocus?.();
    };
  }, []);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        close();
      }
      if (event.key === "Tab") {
        event.preventDefault();
        setExpanded((value) => !value);
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        void handleSave();
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [content, title, note, projectId, saving, saved]);

  function close() {
    shouldRefreshOnFocusRef.current = true;
    void tauriClient.closeCurrentWindow();
  }

  function startWindowDrag() {
    void getCurrentWindow().startDragging().catch(() => {});
  }

  function handleShellMouseDown(event: React.MouseEvent<HTMLDivElement>) {
    const target = event.target as HTMLElement | null;
    if (
      target?.closest(
        "button, input, textarea, select, option, a, [data-no-drag='true']",
      )
    ) {
      return;
    }
    startWindowDrag();
  }

  async function handleSave() {
    if (!content.trim() || saving || saved) {
      return;
    }

    setSaving(true);
    const ctx = await tauriClient
      .detectAppContext()
      .catch(() => ({ sourceApp: null, sourceWindow: null }));

    const result = await create(
      buildQuickCaptureInput(
        {
          title,
          content,
          note,
          projectId,
        },
        ctx,
      ),
      { origin: "quick-capture" },
    );

    setSaving(false);
    if (!result.ok) {
      console.error(
        "[recall][capture] quick save failed:",
        result.error ?? "Unknown capture failure.",
      );
      return;
    }

    if (result.traceId) {
      markUiConfirmationShown(result.traceId);
    }

    setSaved(true);
    window.setTimeout(() => close(), 700);
  }

  const helper = content.trim()
    ? "Clipboard ready. Save now or add a little context."
    : "Paste or type anything worth keeping.";
  const canSave = content.trim().length > 0 && !saving && !saved;

  return (
    <div className="overlay-host" onClick={close}>
      <div
        onMouseDown={handleShellMouseDown}
        onClick={(event) => event.stopPropagation()}
        className="search-panel"
        style={{
          width: "100%",
          height: "100%",
          maxWidth: "none",
          display: "flex",
          flexDirection: "column",
          borderRadius: 14,
        }}
      >
        {/* Header — drag region + close button */}
        <div
          data-tauri-drag-region
          onMouseDown={startWindowDrag}
          style={{
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: 12,
            padding: "16px 18px",
            borderBottom: "0.5px solid var(--line)",
            cursor: "grab",
            userSelect: "none",
            WebkitUserSelect: "none",
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                fontSize: 10,
                fontWeight: 600,
                letterSpacing: "0.1em",
                textTransform: "uppercase",
                color: "var(--t-4)",
                marginBottom: 4,
                display: "flex",
                alignItems: "center",
                gap: 5,
              }}
            >
              <Plus size={11} strokeWidth={1.8} /> Quick Capture
            </div>
            <div style={{ fontSize: 13, color: "var(--t-2)" }}>{helper}</div>
          </div>
          <button
            data-no-drag="true"
            type="button"
            onClick={close}
            onMouseDown={(event) => event.stopPropagation()}
            className="detail-close"
            aria-label="Close quick capture"
          >
            <X size={13} strokeWidth={1.8} />
          </button>
        </div>

        {/* Body — content textarea + optional details */}
        <div className="no-drag" style={{ flex: 1, overflow: "auto", padding: 14 }}>
          <textarea
            data-no-drag="true"
            ref={textareaRef}
            value={content}
            onChange={(event) => setContent(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.metaKey &&
                !event.ctrlKey
              ) {
                event.preventDefault();
                void handleSave();
              }
            }}
            placeholder="Capture a thought…"
            className="textarea"
            style={{ minHeight: 160, fontSize: 14, lineHeight: 1.55 }}
          />

          <div style={{ marginTop: 10, display: "flex", alignItems: "center", gap: 8 }}>
            <button
              type="button"
              className="btn btn-quiet"
              onClick={() => setExpanded((value) => !value)}
            >
              <ChevronDown
                size={12}
                strokeWidth={1.8}
                style={{
                  transform: expanded ? "rotate(180deg)" : undefined,
                  transition: "transform 200ms var(--ease)",
                }}
              />
              {expanded ? "Hide details" : "Add details"}
              <span className="kbd">Tab</span>
            </button>
          </div>

          {expanded ? (
            <div
              style={{
                marginTop: 12,
                padding: 14,
                borderRadius: 12,
                background: "var(--tint-1)",
                boxShadow: "inset 0 0 0 0.5px var(--line)",
                display: "grid",
                gap: 10,
              }}
            >
              <input
                data-no-drag="true"
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="Title"
                className="input"
              />
              <select
                data-no-drag="true"
                value={projectId}
                onChange={(event) => setProjectId(event.target.value)}
                className="select"
              >
                <option value="">No project</option>
                {projects.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
              </select>
              <textarea
                data-no-drag="true"
                value={note}
                onChange={(event) => setNote(event.target.value)}
                placeholder="Why is this worth keeping?"
                className="textarea"
                style={{ minHeight: 70 }}
              />
            </div>
          ) : null}
        </div>

        {/* Footer — status hint + save button */}
        <div
          className="no-drag"
          style={{
            padding: "10px 18px",
            borderTop: "0.5px solid var(--line)",
            display: "flex",
            alignItems: "center",
            gap: 12,
            flexShrink: 0,
            fontSize: 11,
            color: "var(--t-3)",
          }}
        >
          <span style={{ flex: 1, minWidth: 0 }}>
            Saved locally on this device only.
          </span>
          <button
            data-no-drag="true"
            type="button"
            className="btn btn-quiet"
            onClick={close}
          >
            Cancel
          </button>
          <button
            data-no-drag="true"
            type="button"
            className="btn btn-primary"
            onClick={handleSave}
            disabled={!canSave}
          >
            <Save size={12} strokeWidth={1.8} />
            {saved ? "Saved" : saving ? "Saving…" : "Save"}
            {!saved && !saving ? (
              <span
                className="kbd"
                style={{
                  background: "rgba(255,255,255,0.18)",
                  color: "rgba(255,255,255,0.92)",
                }}
              >
                {navigator.platform.includes("Mac") ? "⌘↵" : "Ctrl+↵"}
              </span>
            ) : null}
          </button>
        </div>
      </div>
    </div>
  );
}
