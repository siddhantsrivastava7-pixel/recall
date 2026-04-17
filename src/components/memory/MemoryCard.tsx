import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
  Clock3,
  Copy,
  Globe,
  MessageSquare,
  Trash,
} from "lucide-react";

import type { Memory } from "@/domain/types";
import {
  getMemoryDisplayDomain,
  getMemoryDisplayMetadata,
  getMemoryDisplayPreview,
  getMemoryDisplaySourceType,
  getMemoryDisplayTitle,
  normalizeDisplayText,
} from "@/domain/formatters";
import { tauriClient } from "@/services/api/tauri-client";
import {
  formatResurfaceLabel,
  fromDatetimeLocalValue,
  getResurfacePresetDate,
  isMemoryDueForResurface,
  toDatetimeLocalValue,
} from "@/services/resurface/memoryResurface";
import { useMemoryStore } from "@/stores/memoryStore";

interface Props {
  memory: Memory;
  resurfaced?: boolean;
  onSelect?: (memory: Memory) => void;
}

export function MemoryCard({ memory, resurfaced, onSelect }: Props) {
  const [hovered, setHovered] = useState(false);
  const [copied, setCopied] = useState(false);
  const [editingNote, setEditingNote] = useState(false);
  const [noteDraft, setNoteDraft] = useState(memory.note ?? "");
  const [resurfaceOpen, setResurfaceOpen] = useState(false);
  const { remove, update, setResurface, dismissResurface } = useMemoryStore();
  const copyResetTimeoutRef = useRef<number | null>(null);

  const title = useMemo(() => getMemoryDisplayTitle(memory), [memory]);
  const preview = useMemo(() => getMemoryDisplayPreview(memory, 220), [memory]);
  const metadata = useMemo(() => getMemoryDisplayMetadata(memory), [memory]);
  const domain = useMemo(() => getMemoryDisplayDomain(memory), [memory]);
  const sourceTypeLabel = getMemoryDisplaySourceType(memory);
  const noteText = useMemo(() => normalizeDisplayText(memory.note), [memory.note]);
  const resurfaceLabel = useMemo(() => formatResurfaceLabel(memory), [memory]);
  const isDue = isMemoryDueForResurface(memory);
  const topics = (memory.topicLabels ?? []).slice(0, 3);
  const sourceTypeIcon =
    memory.sourceType === "bookmark" ? (
      <Globe size={10} strokeWidth={1.9} />
    ) : (
      <MessageSquare size={10} strokeWidth={1.9} />
    );

  useEffect(() => {
    setNoteDraft(memory.note ?? "");
  }, [memory.id, memory.note]);

  useEffect(() => {
    return () => {
      if (copyResetTimeoutRef.current !== null) {
        window.clearTimeout(copyResetTimeoutRef.current);
      }
    };
  }, []);

  async function copyValue(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    await tauriClient.writeClipboardText(memory.url ?? memory.content);
    setCopied(true);
    if (copyResetTimeoutRef.current !== null) {
      window.clearTimeout(copyResetTimeoutRef.current);
    }
    copyResetTimeoutRef.current = window.setTimeout(() => {
      setCopied(false);
      copyResetTimeoutRef.current = null;
    }, 1200);
  }

  async function deleteMemory(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    await remove(memory.id);
  }

  async function saveNote(event?: React.MouseEvent<HTMLButtonElement>) {
    event?.stopPropagation();
    const result = await update(memory.id, {
      sourceType: memory.sourceType,
      title: memory.title,
      content: memory.content,
      note: noteDraft.trim() || null,
      projectId: memory.projectId,
      url: memory.url,
      externalId: memory.externalId,
      folderPath: memory.folderPath,
      sourceApp: memory.sourceApp,
      sourceWindow: memory.sourceWindow,
      createdAt: memory.createdAt,
      updatedAt: null,
    });
    if (result.ok) setEditingNote(false);
  }

  async function setBringBack(iso: string | null) {
    await setResurface(memory.id, iso);
    setResurfaceOpen(false);
  }

  async function dismissBringBack(event: React.MouseEvent<HTMLButtonElement>) {
    event.stopPropagation();
    await dismissResurface(memory.id);
    setResurfaceOpen(false);
  }

  function openMemory(event?: React.MouseEvent<HTMLButtonElement>) {
    event?.stopPropagation();
    onSelect?.(memory);
  }

  return (
    <article
      className={`memory-card ${resurfaced ? "resurfaced" : ""}`}
      onClick={() => onSelect?.(memory)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        gap: 0,
        position: "relative",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 16,
          marginBottom: 14,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <span className="tag">
            {sourceTypeIcon}
            {sourceTypeLabel}
          </span>
          {domain && (
            <span
              className="tag"
              style={{
                color: "rgba(255,255,255,0.42)",
              }}
            >
              {domain}
            </span>
          )}
          {resurfaced && (
            <span className="tag tag-blue" style={{ fontSize: 10, padding: "2px 8px" }}>
              Resurfaced
            </span>
          )}
          {resurfaceLabel && (
            <span className={`tag ${isDue ? "tag-blue" : ""}`} style={{ fontSize: 10, padding: "2px 8px" }}>
              <Clock3 size={10} strokeWidth={1.9} />
              {resurfaceLabel}
            </span>
          )}
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            opacity: hovered || copied ? 1 : 0,
            transition: "opacity 120ms ease",
            pointerEvents: hovered || copied ? "auto" : "none",
            flexShrink: 0,
          }}
        >
          <CardAction
            icon={<Clock3 size={13} strokeWidth={1.9} />}
            label="Bring back"
            onClick={(event) => {
              event.stopPropagation();
              setResurfaceOpen((value) => !value);
            }}
            active={resurfaceOpen || Boolean(memory.resurfaceAt)}
          />
          <CardAction
            icon={<ArrowUpRight size={13} strokeWidth={1.9} />}
            label="Open"
            onClick={openMemory}
          />
          <CardAction
            icon={
              copied ? (
                <Check size={13} strokeWidth={2} />
              ) : (
                <Copy size={13} strokeWidth={1.9} />
              )
            }
            label={copied ? "Copied" : "Copy"}
            onClick={copyValue}
            active={copied}
          />
          <CardAction
            icon={<Trash size={13} strokeWidth={1.9} />}
            label="Delete"
            danger
            onClick={deleteMemory}
          />
        </div>
      </div>

      {resurfaceOpen && (
        <BringBackMenu
          currentValue={memory.resurfaceAt}
          due={isDue}
          onPreset={(preset) => void setBringBack(getResurfacePresetDate(preset))}
          onCustom={(value) => void setBringBack(fromDatetimeLocalValue(value))}
          onClear={() => void setBringBack(null)}
          onDismiss={dismissBringBack}
        />
      )}

      <div
        style={{
          fontSize: 16,
          fontWeight: 620,
          color: "var(--text-primary)",
          letterSpacing: "-0.018em",
          lineHeight: 1.35,
          marginBottom: 10,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 2,
          overflow: "hidden",
        }}
      >
        {title}
      </div>

      <div
        style={{
          fontSize: 13,
          color: "var(--text-secondary)",
          lineHeight: 1.65,
          display: "-webkit-box",
          WebkitBoxOrient: "vertical",
          WebkitLineClamp: 2,
          overflow: "hidden",
          marginBottom: 16,
        }}
      >
        {preview}
      </div>

      {topics.length > 0 && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            flexWrap: "wrap",
            marginBottom: 14,
          }}
        >
          {topics.map((topic) => (
            <span
              key={topic}
              className="tag"
              style={{
                fontSize: 10,
                padding: "2px 7px",
                color: "rgba(255,255,255,0.34)",
              }}
            >
              {topic}
            </span>
          ))}
        </div>
      )}

      {(editingNote || noteText || hovered) && (
        <div
          onClick={(event) => event.stopPropagation()}
          style={{
            marginBottom: 16,
            padding: editingNote ? "10px 12px" : "0",
            borderRadius: 14,
            background: editingNote ? "rgba(255,255,255,0.035)" : "transparent",
            border: editingNote ? "1px solid rgba(255,255,255,0.06)" : "1px solid transparent",
          }}
        >
          {editingNote ? (
            <>
              <textarea
                value={noteDraft}
                onChange={(event) => setNoteDraft(event.target.value)}
                placeholder="Add a short note..."
                rows={3}
                autoFocus
                style={{
                  width: "100%",
                  resize: "vertical",
                  background: "transparent",
                  border: "none",
                  outline: "none",
                  color: "rgba(255,255,255,0.72)",
                  fontFamily: "inherit",
                  fontSize: 13,
                  lineHeight: 1.55,
                }}
                onKeyDown={(event) => {
                  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                    void saveNote();
                  }
                  if (event.key === "Escape") {
                    setNoteDraft(memory.note ?? "");
                    setEditingNote(false);
                  }
                }}
              />
              <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 8 }}>
                <button className="btn-ghost" style={{ padding: "6px 10px", fontSize: 12 }} onClick={() => {
                  setNoteDraft(memory.note ?? "");
                  setEditingNote(false);
                }}>
                  Cancel
                </button>
                <button className="btn-primary" style={{ padding: "6px 10px", fontSize: 12 }} onClick={saveNote}>
                  Save note
                </button>
              </div>
            </>
          ) : (
            <button
              onClick={() => setEditingNote(true)}
              style={{
                width: "100%",
                textAlign: "left",
                background: "none",
                border: "none",
                padding: 0,
                color: noteText ? "rgba(255,255,255,0.46)" : "rgba(255,255,255,0.26)",
                fontSize: 12,
                lineHeight: 1.55,
                fontFamily: "inherit",
                cursor: "text",
                display: hovered || noteText ? "block" : "none",
              }}
            >
              {noteText ? `Note: ${noteText}` : "Add note"}
            </button>
          )}
        </div>
      )}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginTop: "auto",
          fontSize: 12,
          color: "rgba(255,255,255,0.32)",
          flexWrap: "wrap",
        }}
      >
        {metadata.map((item, index) => (
          <MetadataPart key={`${item}-${index}`} value={item} showSeparator={index < metadata.length - 1} />
        ))}
      </div>
    </article>
  );
}

function BringBackMenu({
  currentValue,
  due,
  onPreset,
  onCustom,
  onClear,
  onDismiss,
}: {
  currentValue?: string | null;
  due: boolean;
  onPreset: (preset: "later_today" | "tomorrow" | "next_week") => void;
  onCustom: (value: string) => void;
  onClear: () => void;
  onDismiss: (event: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <div
      onClick={(event) => event.stopPropagation()}
      style={{
        position: "absolute",
        top: 50,
        right: 52,
        zIndex: 4,
        width: 210,
        padding: 8,
        borderRadius: 16,
        background: "rgba(17,24,39,0.96)",
        border: "1px solid rgba(255,255,255,0.08)",
        boxShadow: "0 18px 50px rgba(0,0,0,0.34)",
      }}
    >
      <MenuButton label="Later today" onClick={() => onPreset("later_today")} />
      <MenuButton label="Tomorrow" onClick={() => onPreset("tomorrow")} />
      <MenuButton label="Next week" onClick={() => onPreset("next_week")} />
      <input
        type="datetime-local"
        defaultValue={toDatetimeLocalValue(currentValue)}
        onChange={(event) => onCustom(event.target.value)}
        style={{
          width: "100%",
          marginTop: 6,
          marginBottom: 6,
          background: "rgba(255,255,255,0.04)",
          border: "1px solid rgba(255,255,255,0.06)",
          borderRadius: 10,
          color: "rgba(255,255,255,0.66)",
          padding: "7px 8px",
          fontSize: 12,
          fontFamily: "inherit",
        }}
      />
      {due && <MenuButton label="Dismiss for now" onClick={onDismiss} />}
      {currentValue && <MenuButton label="Clear bring-back" onClick={onClear} muted />}
    </div>
  );
}

function MenuButton({
  label,
  onClick,
  muted = false,
}: {
  label: string;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  muted?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        width: "100%",
        padding: "8px 9px",
        border: "none",
        borderRadius: 10,
        background: "transparent",
        color: muted ? "rgba(255,255,255,0.34)" : "rgba(255,255,255,0.68)",
        fontSize: 12,
        textAlign: "left",
        cursor: "pointer",
        fontFamily: "inherit",
      }}
      onMouseEnter={(event) => {
        event.currentTarget.style.background = "rgba(255,255,255,0.06)";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.background = "transparent";
      }}
    >
      {label}
    </button>
  );
}

function CardAction({
  icon,
  label,
  onClick,
  danger = false,
  active = false,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  danger?: boolean;
  active?: boolean;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      style={{
        width: 28,
        height: 28,
        borderRadius: 8,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: active ? "var(--blue-dim)" : "rgba(255,255,255,0.04)",
        border: `1px solid ${active ? "var(--blue-border)" : "rgba(255,255,255,0.05)"}`,
        color: active
          ? "var(--blue)"
          : danger
            ? "rgba(248,113,113,0.75)"
            : "rgba(255,255,255,0.38)",
        cursor: "pointer",
        transition: "background 120ms ease, color 120ms ease, border-color 120ms ease",
      }}
      onMouseEnter={(event) => {
        if (active) {
          event.currentTarget.style.background = "var(--blue-dim)";
          event.currentTarget.style.borderColor = "var(--blue-border)";
          event.currentTarget.style.color = "var(--blue)";
          return;
        }
        event.currentTarget.style.background = danger
          ? "rgba(248,113,113,0.10)"
          : "rgba(255,255,255,0.08)";
        event.currentTarget.style.borderColor = danger
          ? "rgba(248,113,113,0.16)"
          : "rgba(255,255,255,0.09)";
        event.currentTarget.style.color = danger ? "var(--danger)" : "var(--text-primary)";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.background = active
          ? "var(--blue-dim)"
          : "rgba(255,255,255,0.04)";
        event.currentTarget.style.borderColor = active
          ? "var(--blue-border)"
          : "rgba(255,255,255,0.05)";
        event.currentTarget.style.color = active
          ? "var(--blue)"
          : danger
            ? "rgba(248,113,113,0.75)"
            : "rgba(255,255,255,0.38)";
      }}
    >
      {icon}
    </button>
  );
}

function MetadataPart({
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
            background: "rgba(255,255,255,0.15)",
          }}
        />
      )}
    </>
  );
}
