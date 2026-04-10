import { useEffect, useRef, useState } from "react";
import { TauriEvent } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Save, ChevronDown, ChevronUp, X } from "lucide-react";

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

    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";

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

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        background: "transparent",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
      onClick={close}
    >
      <div
        onMouseDown={handleShellMouseDown}
        onClick={(event) => event.stopPropagation()}
        style={{
          width: 500,
          borderRadius: 20,
          overflow: "hidden",
          background: "linear-gradient(145deg, #0D1628 0%, #0B1535 40%, #0E1A3A 100%)",
          border: "1px solid rgba(79,124,255,0.25)",
          boxShadow:
            "0 24px 64px rgba(0,0,0,0.6), 0 0 0 1px rgba(79,124,255,0.1), inset 0 1px 0 rgba(255,255,255,0.06)",
          backdropFilter: "blur(24px)",
          WebkitBackdropFilter: "blur(24px)",
          maxHeight: "90vh",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            height: 2,
            background:
              "linear-gradient(90deg, transparent, rgba(79,124,255,0.6) 30%, rgba(120,160,255,0.8) 50%, rgba(79,124,255,0.6) 70%, transparent)",
          }}
        />

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "14px 18px 12px",
            borderBottom: "1px solid rgba(79,124,255,0.12)",
          }}
        >
          <div
            data-tauri-drag-region
            onMouseDown={startWindowDrag}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              flex: 1,
              minWidth: 0,
              cursor: "grab",
              userSelect: "none",
              WebkitUserSelect: "none",
            }}
          >
            <div
              style={{
                width: 24,
                height: 24,
                borderRadius: 6,
                background: "rgba(79,124,255,0.2)",
                border: "1px solid rgba(79,124,255,0.35)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <Save size={12} strokeWidth={2} color="#4F7CFF" />
            </div>
            <span
              style={{
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: "0.1em",
                textTransform: "uppercase",
                color: "rgba(255,255,255,0.5)",
              }}
            >
              Quick Capture
            </span>
          </div>
          <button
            onClick={close}
            onMouseDown={(event) => event.stopPropagation()}
            style={{
              width: 24,
              height: 24,
              borderRadius: 6,
              background: "rgba(255,255,255,0.06)",
              border: "1px solid rgba(255,255,255,0.08)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "rgba(255,255,255,0.3)",
              cursor: "pointer",
            }}
          >
            <X size={13} />
          </button>
        </div>

        <div style={{ overflowY: "auto", flex: 1 }}>
          <div style={{ padding: "16px 18px 0" }}>
            <textarea
              data-no-drag="true"
              ref={textareaRef}
              value={content}
              onChange={(event) => setContent(event.target.value)}
              placeholder="Capture a thought..."
              rows={expanded ? 3 : 4}
              style={{
                width: "100%",
                background: "transparent",
                border: "none",
                outline: "none",
                color: "var(--text-primary)",
                fontSize: 15,
                fontFamily: "inherit",
                resize: "none",
                lineHeight: 1.65,
                caretColor: "#4F7CFF",
              }}
            />
          </div>

          {expanded && (
            <div
              style={{
                margin: "8px 18px 0",
                borderTop: "1px solid rgba(79,124,255,0.1)",
                paddingTop: 10,
                display: "flex",
                flexDirection: "column",
              }}
            >
              <MetaRow label="Title">
                <input
                  data-no-drag="true"
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder="Add a title..."
                  style={metaInputStyle}
                />
              </MetaRow>
              <MetaRow label="Project">
                <select
                  data-no-drag="true"
                  value={projectId}
                  onChange={(event) => setProjectId(event.target.value)}
                  style={{ ...metaInputStyle, cursor: "pointer" }}
                >
                  <option value="" style={{ background: "#0D1628" }}>
                    No project
                  </option>
                  {projects.map((project) => (
                    <option
                      key={project.id}
                      value={project.id}
                      style={{ background: "#0D1628" }}
                    >
                      {project.name}
                    </option>
                  ))}
                </select>
              </MetaRow>
              <MetaRow label="Note" last>
                <input
                  data-no-drag="true"
                  value={note}
                  onChange={(event) => setNote(event.target.value)}
                  placeholder="Add notes..."
                  style={metaInputStyle}
                />
              </MetaRow>
            </div>
          )}
        </div>

        <div
          style={{
            padding: "12px 18px 16px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            borderTop: "1px solid rgba(79,124,255,0.08)",
            flexShrink: 0,
          }}
        >
          <button
            onClick={() => setExpanded((value) => !value)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              fontSize: 12,
              color: "rgba(255,255,255,0.3)",
              background: "none",
              border: "none",
              cursor: "pointer",
              fontFamily: "inherit",
            }}
          >
            {expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
            {expanded ? "Collapse" : "Add details"}
            <span className="kbd">Tab</span>
          </button>

          <button
            onClick={handleSave}
            disabled={!content.trim() || saving}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              padding: "9px 22px",
              background: saved
                ? "rgba(79,124,255,0.3)"
                : content.trim()
                  ? "linear-gradient(135deg, #4F7CFF 0%, #6B96FF 100%)"
                  : "rgba(79,124,255,0.15)",
              color: content.trim() ? "#fff" : "rgba(255,255,255,0.3)",
              border: "1px solid rgba(79,124,255,0.35)",
              borderRadius: 10,
              fontSize: 13,
              fontWeight: 600,
              cursor: content.trim() && !saving ? "pointer" : "default",
              fontFamily: "inherit",
              transition: "all 150ms ease",
              boxShadow: content.trim() ? "0 4px 16px rgba(79,124,255,0.3)" : "none",
            }}
          >
            <Save size={13} strokeWidth={2} />
            {saved ? "Saved!" : saving ? "Saving..." : "Save"}
            {!saved && (
              <span
                className="kbd"
                style={{
                  background: "rgba(255,255,255,0.15)",
                  borderColor: "transparent",
                  color: "rgba(255,255,255,0.7)",
                }}
              >
                Ctrl+Enter
              </span>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

function MetaRow({
  label,
  children,
  last,
}: {
  label: string;
  children: React.ReactNode;
  last?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "9px 0",
        borderBottom: last ? "none" : "1px solid rgba(79,124,255,0.07)",
      }}
    >
      <span
        style={{
          fontSize: 11,
          fontWeight: 600,
          color: "rgba(255,255,255,0.28)",
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          width: 52,
          flexShrink: 0,
        }}
      >
        {label}
      </span>
      {children}
    </div>
  );
}

const metaInputStyle: React.CSSProperties = {
  flex: 1,
  background: "transparent",
  border: "none",
  outline: "none",
  color: "var(--text-primary)",
  fontSize: 14,
  fontFamily: "inherit",
};
