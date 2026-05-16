// v0.5.61 — Recall Pointer panel.
//
// The compact action sheet shown inside the search-overlay
// window when Pointer mode is active. Three states:
//
//   actions  — selection preview + "you have N related" + the
//              three action buttons
//   related  — inline list of semantically-related saved
//              memories (click → open in main window)
//   ask      — grounded Ask Recall answer + citation chips
//
// Dark premium Recall styling: deep charcoal glass, restrained
// blue accent, calm. No chat-first layout, no clutter. Esc (or
// the close affordance) tears down the session and closes the
// overlay.

import { useEffect } from "react";
import {
  ArrowUpRight,
  Check,
  FileText,
  Loader2,
  Search,
  Sparkles,
  X,
} from "lucide-react";

import { tauriClient } from "@/services/api/tauri-client";
import { useMemoryStore } from "@/stores/memoryStore";
import { getMemoryDisplayTitle } from "@/domain/formatters";
import { usePointerStore } from "./pointerStore";
import {
  askRecallAboutSelection,
  findRelatedForSelection,
  probeRelatedCount,
  savePointerSelection,
} from "./pointerActions";

export function PointerPanel({ onClose }: { onClose: () => void }) {
  const selection = usePointerStore((s) => s.selection);
  const mode = usePointerStore((s) => s.mode);
  const relatedCount = usePointerStore((s) => s.relatedCount);
  const relatedResults = usePointerStore((s) => s.relatedResults);
  const ask = usePointerStore((s) => s.ask);
  const busy = usePointerStore((s) => s.busy);
  const savedMemoryId = usePointerStore((s) => s.savedMemoryId);
  const errorMessage = usePointerStore((s) => s.errorMessage);
  const setMode = usePointerStore((s) => s.setMode);
  const memories = useMemoryStore((s) => s.memories);

  // Kick the background "have I seen this?" probe once per
  // selection. Cheap, non-blocking; result is a header line.
  useEffect(() => {
    if (selection) {
      void probeRelatedCount(selection);
    }
  }, [selection?.capturedAt]);

  if (!selection) return null;

  const hasText = selection.text.trim().length > 0;
  const sourceLabel =
    selection.sourceApp && selection.sourceApp !== "recall-pointer"
      ? selection.sourceApp
      : null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "16vh",
        background: "transparent",
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 540,
          maxWidth: "92vw",
          borderRadius: 16,
          background: "var(--panel-glass, rgba(20,20,24,0.92))",
          border: "1px solid var(--line-strong)",
          boxShadow:
            "0 24px 64px rgba(0,0,0,0.55), 0 0 0 0.5px rgba(255,255,255,0.06)",
          backdropFilter: "blur(20px)",
          overflow: "hidden",
          fontFamily: "var(--font)",
        }}
      >
        {/* ── Header: eyebrow + close ── */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "14px 16px 0",
          }}
        >
          <div
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 7,
              fontSize: 10,
              fontWeight: 650,
              letterSpacing: "0.14em",
              textTransform: "uppercase",
              color: "var(--accent, #0A84FF)",
            }}
          >
            <Sparkles size={11} strokeWidth={1.9} />
            Recall Pointer
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            style={{
              background: "transparent",
              border: "none",
              color: "var(--t-4)",
              cursor: "pointer",
              padding: 4,
              display: "inline-flex",
            }}
          >
            <X size={14} strokeWidth={1.9} />
          </button>
        </div>

        {/* ── Empty-clipboard hint (discovery aid) ── */}
        {!hasText ? (
          <div style={{ padding: "14px 16px 18px" }}>
            <div
              style={{
                fontSize: 13.5,
                lineHeight: 1.55,
                color: "var(--t-2)",
              }}
            >
              Copy any text first, then press the Pointer shortcut again.
            </div>
            <div
              style={{
                marginTop: 6,
                fontSize: 12,
                color: "var(--t-4)",
                lineHeight: 1.5,
              }}
            >
              Recall Pointer bridges what you're looking at now to what
              you've already saved — select &amp; copy, then trigger.
            </div>
          </div>
        ) : null}

        {/* ── Selection preview ── */}
        {hasText ? (
        <div style={{ padding: "10px 16px 4px" }}>
          <div
            style={{
              fontSize: 13.5,
              lineHeight: 1.55,
              color: "var(--t-1)",
              maxHeight: 88,
              overflow: "hidden",
              display: "-webkit-box",
              WebkitLineClamp: 4,
              WebkitBoxOrient: "vertical",
              whiteSpace: "pre-wrap",
            }}
          >
            {selection.text}
          </div>
          <div
            style={{
              marginTop: 8,
              fontSize: 11,
              color: "var(--t-4)",
              display: "flex",
              alignItems: "center",
              gap: 6,
            }}
          >
            {sourceLabel ? (
              <>
                <span>from {sourceLabel}</span>
                <span style={{ opacity: 0.5 }}>·</span>
              </>
            ) : null}
            <span>just now</span>
            {relatedCount !== null && relatedCount > 0 ? (
              <>
                <span style={{ opacity: 0.5 }}>·</span>
                <span style={{ color: "var(--accent, #0A84FF)" }}>
                  {relatedCount} related save{relatedCount === 1 ? "" : "s"} in
                  your memory
                </span>
              </>
            ) : null}
          </div>
        </div>
        ) : null}

        {/* ── Body: actions / related / ask ── */}
        {hasText ? (
        <div style={{ padding: "12px 16px 16px" }}>
          {mode === "actions" ? (
            <ActionRow
              busy={busy}
              savedMemoryId={savedMemoryId}
              onSave={() => void savePointerSelection(selection)}
              onRelated={() => void findRelatedForSelection(selection)}
              onAsk={() => void askRecallAboutSelection(selection)}
              onOpenSaved={() => {
                if (savedMemoryId && savedMemoryId !== "saved") {
                  void tauriClient.openMemoryInMain(savedMemoryId);
                  onClose();
                }
              }}
            />
          ) : null}

          {mode === "related" ? (
            <RelatedList
              busy={busy === "related"}
              hits={relatedResults}
              memories={memories}
              onBack={() => setMode("actions")}
              onOpen={(id) => {
                void tauriClient.openMemoryInMain(id);
                onClose();
              }}
            />
          ) : null}

          {mode === "ask" ? (
            <AskResult
              busy={busy === "ask"}
              answer={ask}
              memories={memories}
              onBack={() => setMode("actions")}
              onOpen={(id) => {
                void tauriClient.openMemoryInMain(id);
                onClose();
              }}
            />
          ) : null}

          {errorMessage ? (
            <div
              style={{
                marginTop: 10,
                fontSize: 12,
                color: "var(--danger, #ff6b6b)",
                lineHeight: 1.45,
              }}
            >
              {errorMessage}
            </div>
          ) : null}
        </div>
        ) : null}
      </div>
    </div>
  );
}

function ActionRow({
  busy,
  savedMemoryId,
  onSave,
  onRelated,
  onAsk,
  onOpenSaved,
}: {
  busy: string | null;
  savedMemoryId: string | null;
  onSave: () => void;
  onRelated: () => void;
  onAsk: () => void;
  onOpenSaved: () => void;
}) {
  if (savedMemoryId) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 7,
            fontSize: 13,
            color: "var(--t-2)",
          }}
        >
          <Check size={14} strokeWidth={2} color="#3FB950" />
          Saved to Recall
        </span>
        {savedMemoryId !== "saved" ? (
          <button
            type="button"
            onClick={onOpenSaved}
            style={ghostBtn}
          >
            Open in Recall
            <ArrowUpRight size={12} strokeWidth={1.9} />
          </button>
        ) : null}
      </div>
    );
  }
  return (
    <div style={{ display: "flex", gap: 8 }}>
      <PointerButton
        label="Save"
        primary
        busy={busy === "save"}
        onClick={onSave}
        icon={<FileText size={13} strokeWidth={1.9} />}
      />
      <PointerButton
        label="Find related"
        busy={busy === "related"}
        onClick={onRelated}
        icon={<Search size={13} strokeWidth={1.9} />}
      />
      <PointerButton
        label="Ask Recall"
        busy={busy === "ask"}
        onClick={onAsk}
        icon={<Sparkles size={13} strokeWidth={1.9} />}
      />
    </div>
  );
}

function RelatedList({
  busy,
  hits,
  memories,
  onBack,
  onOpen,
}: {
  busy: boolean;
  hits: { memoryId: string; semanticScore: number }[];
  memories: { id: string }[];
  onBack: () => void;
  onOpen: (id: string) => void;
}) {
  if (busy) return <Spinner label="Searching your memories…" />;
  if (hits.length === 0) {
    return (
      <EmptyState
        text="Nothing related in your saved memories yet."
        onBack={onBack}
      />
    );
  }
  const byId = new Map(memories.map((m: any) => [m.id, m]));
  return (
    <div>
      <BackBar onBack={onBack} label="Related memories" />
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {hits.slice(0, 6).map((hit) => {
          const memory = byId.get(hit.memoryId);
          if (!memory) return null;
          return (
            <button
              key={hit.memoryId}
              type="button"
              onClick={() => onOpen(hit.memoryId)}
              style={resultRow}
              onMouseOver={(e) => {
                (e.currentTarget as HTMLButtonElement).style.background =
                  "var(--bg-hover)";
              }}
              onMouseOut={(e) => {
                (e.currentTarget as HTMLButtonElement).style.background =
                  "transparent";
              }}
            >
              <FileText
                size={13}
                strokeWidth={1.8}
                style={{ color: "var(--t-4)", flexShrink: 0, marginTop: 2 }}
              />
              <span
                style={{
                  flex: 1,
                  fontSize: 13,
                  color: "var(--t-1)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {getMemoryDisplayTitle(memory as any)}
              </span>
              <ArrowUpRight
                size={12}
                strokeWidth={1.9}
                style={{ color: "var(--t-4)", flexShrink: 0 }}
              />
            </button>
          );
        })}
      </div>
    </div>
  );
}

function AskResult({
  busy,
  answer,
  memories,
  onBack,
  onOpen,
}: {
  busy: boolean;
  answer: { text: string; citations: { memoryId: string }[] } | null;
  memories: { id: string }[];
  onBack: () => void;
  onOpen: (id: string) => void;
}) {
  if (busy) return <Spinner label="Asking Recall…" />;
  if (!answer) {
    return <EmptyState text="No answer yet." onBack={onBack} />;
  }
  const byId = new Map(memories.map((m: any) => [m.id, m]));
  // Strip the inline [memory:<id>] markers for readability; the
  // citation chips below carry the source links.
  const clean = answer.text.replace(/\[memory:[^\]]+\]/g, "").trim();
  return (
    <div>
      <BackBar onBack={onBack} label="Ask Recall" />
      <div
        style={{
          fontSize: 13,
          lineHeight: 1.6,
          color: "var(--t-1)",
          maxHeight: 220,
          overflowY: "auto",
          whiteSpace: "pre-wrap",
        }}
      >
        {clean}
      </div>
      {answer.citations.length > 0 ? (
        <div
          style={{
            marginTop: 12,
            display: "flex",
            flexWrap: "wrap",
            gap: 6,
          }}
        >
          {answer.citations.slice(0, 6).map((c, i) => {
            const memory = byId.get(c.memoryId);
            const label = memory
              ? getMemoryDisplayTitle(memory as any).slice(0, 28)
              : `Source ${i + 1}`;
            return (
              <button
                key={c.memoryId + i}
                type="button"
                onClick={() => onOpen(c.memoryId)}
                style={citationChip}
              >
                {label}
                <ArrowUpRight size={10} strokeWidth={1.9} />
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function PointerButton({
  label,
  icon,
  primary,
  busy,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  primary?: boolean;
  busy?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      style={{
        flex: 1,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 7,
        height: 38,
        borderRadius: 10,
        fontSize: 13,
        fontWeight: 550,
        fontFamily: "inherit",
        cursor: busy ? "default" : "pointer",
        border: "none",
        color: primary ? "#fff" : "var(--t-1)",
        background: primary
          ? "linear-gradient(180deg, oklch(0.74 0.09 245), oklch(0.66 0.10 250))"
          : "var(--bg-hover)",
        boxShadow: primary
          ? "inset 0 0.5px 0 rgba(255,255,255,0.3), 0 1px 2px rgba(0,0,0,0.3)"
          : "inset 0 0 0 0.5px var(--line-strong)",
        opacity: busy ? 0.7 : 1,
        transition: "all 160ms var(--ease)",
      }}
    >
      {busy ? (
        <Loader2 size={13} strokeWidth={2} className="spin" />
      ) : (
        <span style={{ display: "inline-flex" }}>{icon}</span>
      )}
      {label}
    </button>
  );
}

function BackBar({ onBack, label }: { onBack: () => void; label: string }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        marginBottom: 10,
      }}
    >
      <button
        type="button"
        onClick={onBack}
        style={{
          ...ghostBtn,
          padding: "3px 8px",
          fontSize: 11,
        }}
      >
        ← Back
      </button>
      <span
        style={{
          fontSize: 10,
          fontWeight: 650,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color: "var(--t-4)",
        }}
      >
        {label}
      </span>
    </div>
  );
}

function Spinner({ label }: { label: string }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 9,
        padding: "10px 0",
        fontSize: 13,
        color: "var(--t-3)",
      }}
    >
      <Loader2 size={14} strokeWidth={2} className="spin" />
      {label}
    </div>
  );
}

function EmptyState({
  text,
  onBack,
}: {
  text: string;
  onBack: () => void;
}) {
  return (
    <div>
      <BackBar onBack={onBack} label="Result" />
      <div style={{ fontSize: 13, color: "var(--t-3)", padding: "4px 0" }}>
        {text}
      </div>
    </div>
  );
}

const ghostBtn: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  padding: "5px 10px",
  borderRadius: 7,
  fontSize: 12,
  fontFamily: "inherit",
  color: "var(--t-2)",
  background: "var(--bg-hover)",
  border: "none",
  boxShadow: "inset 0 0 0 0.5px var(--line-strong)",
  cursor: "pointer",
};

const resultRow: React.CSSProperties = {
  display: "flex",
  alignItems: "flex-start",
  gap: 9,
  padding: "8px 10px",
  borderRadius: 8,
  background: "transparent",
  border: "none",
  cursor: "pointer",
  textAlign: "left",
  fontFamily: "inherit",
  transition: "background 140ms var(--ease)",
};

const citationChip: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  padding: "4px 9px",
  borderRadius: 999,
  fontSize: 11,
  fontFamily: "inherit",
  color: "var(--accent, #0A84FF)",
  background: "var(--accent-soft, rgba(10,132,255,0.14))",
  border: "none",
  cursor: "pointer",
};
