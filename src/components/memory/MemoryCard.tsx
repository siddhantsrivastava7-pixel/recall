import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
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
} from "@/domain/formatters";
import { tauriClient } from "@/services/api/tauri-client";
import { useMemoryStore } from "@/stores/memoryStore";

interface Props {
  memory: Memory;
  resurfaced?: boolean;
  onSelect?: (memory: Memory) => void;
}

export function MemoryCard({ memory, resurfaced, onSelect }: Props) {
  const [hovered, setHovered] = useState(false);
  const [copied, setCopied] = useState(false);
  const { remove } = useMemoryStore();
  const copyResetTimeoutRef = useRef<number | null>(null);

  const title = useMemo(() => getMemoryDisplayTitle(memory), [memory]);
  const preview = useMemo(() => getMemoryDisplayPreview(memory, 220), [memory]);
  const metadata = useMemo(() => getMemoryDisplayMetadata(memory), [memory]);
  const domain = useMemo(() => getMemoryDisplayDomain(memory), [memory]);
  const sourceTypeLabel = getMemoryDisplaySourceType(memory);
  const sourceTypeIcon =
    memory.sourceType === "bookmark" ? (
      <Globe size={10} strokeWidth={1.9} />
    ) : (
      <MessageSquare size={10} strokeWidth={1.9} />
    );

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
          WebkitLineClamp: 3,
          overflow: "hidden",
          marginBottom: 16,
        }}
      >
        {preview}
      </div>

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
