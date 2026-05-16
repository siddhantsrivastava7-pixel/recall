/**
 * SearchWindow — "search-overlay" label
 *
 * Full-screen keyboard search overlay.
 * Tauri config: transparent bg, no decorations, centered, ~640px wide.
 * Pressing ESC calls closeCurrentWindow().
 */

import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { FileText, Globe, Search, X } from "lucide-react";

import { PointerPanel } from "@/features/pointer/PointerPanel";
import { usePointerStore } from "@/features/pointer/pointerStore";

import {
  formatRelativeTimestamp,
  getHighlightParts,
  getMemoryDisplayMetadata,
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
} from "@/domain/formatters";
import type { Memory, SearchResult } from "@/domain/types";
import { useRecallDataSyncEvents } from "@/hooks/useRecallDataSyncEvents";
import { tauriClient } from "@/services/api/tauri-client";
import { getDueResurfaceMemories } from "@/services/resurface/memoryResurface";
import { useMemoryStore } from "@/stores/memoryStore";
import { useSearchStore } from "@/stores/searchStore";

type SearchListItem = SearchResult | { memory: Memory; score: number; highlights: string[] };

export function SearchWindow() {
  const { query, results, suggestions, selectedIndex, setQuery, moveSelection, reset } = useSearchStore();
  const { memories } = useMemoryStore();
  const inputRef = useRef<HTMLInputElement>(null);
  useRecallDataSyncEvents();

  const dueMemoryIds = new Set(getDueResurfaceMemories(memories, 4).map((memory) => memory.id));
  const recentMemories = [
    ...getDueResurfaceMemories(memories, 4),
    ...memories.filter((memory) => !dueMemoryIds.has(memory.id)),
  ].slice(0, 8);
  const hasQuery = query.trim().length > 0;
  const items: SearchListItem[] = hasQuery
    ? results
    : recentMemories.map((memory) => ({ memory, score: 1, highlights: [] }));

  useEffect(() => {
    // Force the underlying NSWindow (macOS) / HWND (Windows) background to
    // fully transparent. macOS WKWebView keeps its NSWindow's default opaque
    // background otherwise, which paints as a white/gray rectangle around
    // the rounded panel in light mode.
    document.body.style.background = "transparent";
    document.documentElement.style.background = "transparent";
    document.getElementById("root")?.style.setProperty("background", "transparent", "important");
    void getCurrentWindow().setBackgroundColor([0, 0, 0, 0]);
    setTimeout(() => inputRef.current?.focus(), 60);
  }, []);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        reset();
        void tauriClient.closeCurrentWindow();
      }
      if (event.key === "ArrowDown") {
        event.preventDefault();
        moveSelection(1);
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        moveSelection(-1);
      }
      if (event.key === "Enter") {
        const item = items[selectedIndex];
        if (item) void openMemory(item.memory);
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selectedIndex, items]);

  async function openMemory(memory: Memory) {
    reset();
    await tauriClient.openMemoryInMain(memory.id);
    await tauriClient.closeCurrentWindow();
  }

  // v0.5.61 — Recall Pointer. The same search-overlay window
  // doubles as the Pointer host. On `recall://pointer-activate`
  // we take-once the stashed selection; if present, the window
  // renders PointerPanel instead of search. We also probe on
  // mount in case the event fired before this listener attached
  // (window was cold-started by the hotkey).
  const pointerSelection = usePointerStore((s) => s.selection);
  const activatePointer = usePointerStore((s) => s.activate);
  const resetPointer = usePointerStore((s) => s.reset);
  const [pointerChecked, setPointerChecked] = useState(false);

  useEffect(() => {
    let disposed = false;
    const pull = async () => {
      try {
        const sel = await tauriClient.pointerTakeSelection();
        if (!disposed && sel) {
          activatePointer(sel);
        }
      } catch {
        // Pointer is best-effort; a failed take just means the
        // window renders ordinary search.
      } finally {
        if (!disposed) setPointerChecked(true);
      }
    };
    // Cold-start case: hotkey opened the window, the stash is
    // already populated, the event may have raced us.
    void pull();
    // Warm case: window already open, hotkey fires the event.
    const un = getCurrentWindow().listen("recall://pointer-activate", () => {
      void pull();
    });
    return () => {
      disposed = true;
      void un.then((f) => f());
      // Leaving Pointer mode (window closed) clears the session
      // so the next plain search-overlay open is clean.
      resetPointer();
    };
  }, [activatePointer, resetPointer]);

  const closePointer = () => {
    resetPointer();
    void tauriClient.closeCurrentWindow();
  };

  if (pointerChecked && pointerSelection) {
    return (
      <div
        style={{
          width: "100vw",
          height: "100vh",
          background: "transparent",
        }}
      >
        <PointerPanel onClose={closePointer} />
      </div>
    );
  }

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        background: "transparent",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "14vh",
      }}
      onClick={() => {
        reset();
        void tauriClient.closeCurrentWindow();
      }}
    >
      <div
        className="glass search-overlay-panel anim-scalein"
        style={{
          position: "relative",
          width: 620,
          borderRadius: 20,
          overflow: "hidden",
          boxShadow: "0 24px 64px rgba(0,0,0,0.55)",
        }}
        onClick={(event) => event.stopPropagation()}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            pointerEvents: "none",
            background:
              "radial-gradient(140% 120% at 50% -10%, rgba(79,124,255,0.16) 0%, rgba(79,124,255,0.05) 28%, transparent 62%)",
          }}
        />
        <div
          style={{
            position: "absolute",
            inset: 0,
            pointerEvents: "none",
            boxShadow: "inset 0 1px 0 rgba(255,255,255,0.06)",
          }}
        />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "16px 20px",
            borderBottom: "1px solid rgba(255,255,255,0.08)",
          }}
        >
          <Search size={17} color="var(--t-3)" strokeWidth={1.8} />
          <input
            className="search-overlay-input"
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search memories, projects, notes…"
            style={{
              flex: 1,
              background: "transparent",
              border: "none",
              outline: "none",
              fontSize: 16,
              color: "var(--text-primary)",
              fontFamily: "inherit",
            }}
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              style={{
                color: "var(--t-4)",
                cursor: "pointer",
                display: "flex",
                background: "none",
                border: "none",
              }}
            >
              <X size={14} />
            </button>
          )}
          <span className="kbd">ESC</span>
        </div>

        <div style={{ maxHeight: 420, overflowY: "auto" }}>
          {hasQuery && items.length === 0 ? (
            <div
              style={{
                padding: "36px 20px",
                textAlign: "center",
                color: "var(--t-4)",
                fontSize: 14,
              }}
            >
              No memories found for "{query}"
            </div>
          ) : (
            <>
              <div
                style={{
                  padding: "9px 20px 5px",
                  fontSize: 11,
                  fontWeight: 600,
                  color: "var(--t-4)",
                  textTransform: "uppercase",
                  letterSpacing: "0.1em",
                }}
              >
                {hasQuery ? `${items.length} result${items.length !== 1 ? "s" : ""}` : "Recent"}
              </div>
              {hasQuery && suggestions.length > 0 && (
                <div
                  style={{
                    padding: "2px 20px 8px",
                    display: "flex",
                    flexDirection: "column",
                    gap: 6,
                  }}
                >
                  <div
                    style={{
                      fontSize: 11,
                      color: "var(--t-3)",
                      letterSpacing: "0.06em",
                      textTransform: "uppercase",
                    }}
                  >
                    You might be looking for
                  </div>
                  <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                    {suggestions.map((suggestion) => (
                      <button
                        key={suggestion.memory.id}
                        onClick={() => void openMemory(suggestion.memory)}
                        style={{
                          border: "1px solid rgba(79,124,255,0.16)",
                          background: "rgba(79,124,255,0.08)",
                          color: "rgba(229,231,235,0.84)",
                          borderRadius: 999,
                          padding: "6px 10px",
                          fontSize: 12,
                          cursor: "pointer",
                          maxWidth: 180,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                        title={suggestion.reason}
                      >
                        {getMemoryDisplayTitle(suggestion.memory)}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              {items.map((item, index) => (
                <ResultRow
                  key={item.memory.id}
                  memory={item.memory}
                  focused={index === selectedIndex}
                  query={hasQuery ? query : ""}
                  onSelect={() => void openMemory(item.memory)}
                />
              ))}
            </>
          )}
        </div>

        <div
          style={{
            borderTop: "1px solid rgba(255,255,255,0.06)",
            padding: "9px 20px",
            display: "flex",
            alignItems: "center",
            gap: 18,
          }}
        >
          <Hint keys={["↑", "↓"]} label="Navigate" />
          <Hint keys={["↵"]} label="Open" />
          <Hint keys={["ESC"]} label="Close" />
          <span
            style={{
              marginLeft: "auto",
              fontSize: 11,
              color: "var(--t-4)",
            }}
          >
            {memories.length} saved
          </span>
        </div>
      </div>
    </div>
  );
}

function ResultRow({
  memory,
  focused,
  query,
  onSelect,
}: {
  memory: Memory;
  focused: boolean;
  query: string;
  onSelect: () => void;
}) {
  const metadata = getMemoryDisplayMetadata(memory);
  const title = getMemoryDisplayTitle(memory);
  const preview = getMemoryDisplayPreview(memory, 116);
  const topics = (memory.topicLabels ?? []).slice(0, 3);

  return (
    <div
      onClick={onSelect}
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 12,
        padding: "12px 20px",
        cursor: "pointer",
        background: focused ? "rgba(79,124,255,0.10)" : "transparent",
        transition: "background 80ms ease",
      }}
      onMouseEnter={(event) => {
        if (!focused) {
          event.currentTarget.style.background = "rgba(255,255,255,0.04)";
        }
      }}
      onMouseLeave={(event) => {
        if (!focused) {
          event.currentTarget.style.background = "transparent";
        }
      }}
    >
      <div
        style={{
          width: 34,
          height: 34,
          borderRadius: 9,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: focused ? "rgba(79,124,255,0.18)" : "rgba(255,255,255,0.07)",
          color: focused ? "#4F7CFF" : "var(--t-3)",
        }}
      >
        {memory.sourceType === "bookmark" ? (
          <Globe size={14} strokeWidth={1.8} />
        ) : (
          <FileText size={14} strokeWidth={1.8} />
        )}
      </div>

      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontSize: 14,
            fontWeight: 550,
            color: focused ? "#fff" : "var(--text-primary)",
            lineHeight: 1.45,
            display: "-webkit-box",
            WebkitBoxOrient: "vertical",
            WebkitLineClamp: 1,
            overflow: "hidden",
          }}
        >
          <HighlightedText text={title} query={query} />
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--t-3)",
            marginTop: 4,
            lineHeight: 1.5,
            display: "-webkit-box",
            WebkitBoxOrient: "vertical",
            WebkitLineClamp: 2,
            overflow: "hidden",
          }}
        >
          <HighlightedText text={preview} query={query} />
        </div>
        {topics.length > 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              marginTop: 7,
              flexWrap: "wrap",
            }}
          >
            {topics.map((topic) => (
              <TopicChip key={topic} value={topic} query={query} />
            ))}
          </div>
        )}
        <div
          style={{
            fontSize: 11,
            color: "var(--t-3)",
            marginTop: topics.length > 0 ? 6 : 7,
            display: "flex",
            alignItems: "center",
            gap: 6,
            flexWrap: "wrap",
          }}
        >
          {metadata.map((item, index) => (
            <MetadataInline
              key={`${item}-${index}`}
              value={index === metadata.length - 1 ? formatRelativeTimestamp(memory.createdAt) : item}
              showSeparator={index < metadata.length - 1}
            />
          ))}
        </div>
      </div>

      <div style={{ fontSize: 12, color: "var(--t-4)", flexShrink: 0 }}>↵</div>
    </div>
  );
}

function TopicChip({ value, query }: { value: string; query: string }) {
  const matched = getHighlightParts(value, query).some((part) => part.matched);

  return (
    <span
      style={{
        border: matched ? "1px solid rgba(79,124,255,0.22)" : "1px solid rgba(255,255,255,0.06)",
        background: matched ? "rgba(79,124,255,0.10)" : "rgba(255,255,255,0.04)",
        color: matched ? "var(--t-1)" : "var(--t-3)",
        borderRadius: 999,
        padding: "2px 7px",
        fontSize: 10,
        lineHeight: 1.4,
      }}
    >
      {value}
    </span>
  );
}

function HighlightedText({ text, query }: { text: string; query: string }) {
  const parts = getHighlightParts(text, query);

  return (
    <>
      {parts.map((part, index) => (
        <span
          key={`${part.text}-${index}`}
          style={
            part.matched
              ? {
                  background: "rgba(79,124,255,0.14)",
                  color: "inherit",
                  borderRadius: 4,
                  padding: "0 2px",
                }
              : undefined
          }
        >
          {part.text}
        </span>
      ))}
    </>
  );
}

function MetadataInline({
  value,
  showSeparator,
}: {
  value: string;
  showSeparator: boolean;
}) {
  return (
    <>
      <span>{value}</span>
      {showSeparator && (
        <span
          style={{
            width: 3,
            height: 3,
            borderRadius: "50%",
            background: "rgba(255,255,255,0.14)",
          }}
        />
      )}
    </>
  );
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 5 }}>
      <div style={{ display: "flex", gap: 3 }}>
        {keys.map((key) => (
          <span key={key} className="kbd">
            {key}
          </span>
        ))}
      </div>
      <span style={{ fontSize: 12, color: "var(--t-4)" }}>{label}</span>
    </div>
  );
}
