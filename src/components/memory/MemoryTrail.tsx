// Memory Trail panel rendered on the memory detail view (v0.5.58).
//
// A trail is the chain of saved memories on the same topic over
// time — distinct from RelatedMemories (which is a flat list ranked
// by similarity). The trail is chronologically ordered, compact
// (≤7 nodes, ≥3 nodes or hidden), and each node carries a one-line
// "why connected" rationale derived from the dominant signal in
// the link score.
//
// Lazy: the backend `build_memory_trail` is cheap but not free
// (it pulls every memory's chunks for cosine), so we only call
// it when the detail view is open. Cancelled cleanly when the
// user navigates away mid-fetch.

import { GitBranch, ArrowRight } from "lucide-react";
import { useEffect, useState } from "react";

import type { Memory } from "@/domain/types";
import { aiClient, type MemoryTrailNode } from "@/services/ai/AiClient";
import {
  formatRelativeTimestamp,
  getMemoryDisplayTitle,
} from "@/domain/formatters";
import { useMemoryStore } from "@/stores/memoryStore";

interface Props {
  memory: Memory;
  onOpenMemory: (memoryId: string) => void;
}

export function MemoryTrail({ memory, onOpenMemory }: Props) {
  const [nodes, setNodes] = useState<MemoryTrailNode[] | null>(null);
  const [loading, setLoading] = useState(false);

  const memories = useMemoryStore((state) => state.memories);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    aiClient
      .buildMemoryTrail(memory.id)
      .then((result) => {
        if (cancelled) return;
        setNodes(result.nodes);
      })
      .catch((error) => {
        if (cancelled) return;
        // Trails are best-effort — when AI is off or embeddings
        // aren't ready the backend returns an empty result instead
        // of erroring, so reaching this catch usually means a real
        // infrastructure failure (DB lock, etc.). Silent.
        console.warn("[recall][trail] build failed", error);
        setNodes([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [memory.id, memory.embeddingGeneratedAt]);

  // Hide the section entirely when there's nothing to show. Trail
  // surfaces are calm by design — no "no trail yet" placeholder
  // unless the user is mid-load (avoid flicker on quick fetches).
  if (loading) {
    return null;
  }
  if (!nodes || nodes.length === 0) {
    return null;
  }

  // Hydrate each node from the in-memory store — backend only
  // returns memory_id + score + rationale to keep the wire shape
  // small. Memories not in the store (race with deletion) are
  // dropped from the trail.
  const byId = new Map<string, Memory>();
  for (const m of memories) {
    byId.set(m.id, m);
  }
  const hydrated = nodes
    .map((node) => ({
      node,
      memory: byId.get(node.memoryId),
    }))
    .filter((entry): entry is { node: MemoryTrailNode; memory: Memory } => Boolean(entry.memory));

  if (hydrated.length === 0) {
    return null;
  }

  return (
    <section style={{ marginTop: 30 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 12,
          color: "var(--t-3)",
          textTransform: "uppercase",
          letterSpacing: "0.06em",
          marginBottom: 14,
        }}
      >
        <GitBranch size={12} strokeWidth={1.9} />
        Memory trail
        <span
          style={{
            color: "var(--t-4)",
            textTransform: "none",
            letterSpacing: "0.02em",
            fontSize: 11,
            marginLeft: 4,
          }}
        >
          · {hydrated.length} memories on this thread
        </span>
      </div>

      {/*
        Vertical timeline. The connector line is a CSS-painted
        track inside the left gutter; each node draws its own
        marker on top. Keeps the markup flat (no SVG) so the
        layout reflows naturally with long titles.
      */}
      <ol
        style={{
          listStyle: "none",
          margin: 0,
          padding: 0,
          position: "relative",
        }}
      >
        {/* the vertical track */}
        <div
          aria-hidden
          style={{
            position: "absolute",
            top: 8,
            bottom: 8,
            left: 7,
            width: 1,
            background:
              "linear-gradient(180deg, transparent 0, var(--line-strong) 14px, var(--line-strong) calc(100% - 14px), transparent 100%)",
          }}
        />
        {hydrated.map(({ node, memory: nodeMemory }, idx) => (
          <TrailRow
            key={node.memoryId}
            node={node}
            memory={nodeMemory}
            isLast={idx === hydrated.length - 1}
            onOpen={() => {
              if (!node.isSeed) onOpenMemory(node.memoryId);
            }}
          />
        ))}
      </ol>
    </section>
  );
}

function TrailRow({
  node,
  memory,
  isLast,
  onOpen,
}: {
  node: MemoryTrailNode;
  memory: Memory;
  isLast: boolean;
  onOpen: () => void;
}) {
  const title = getMemoryDisplayTitle(memory);
  const dateLabel = formatRelativeTimestamp(memory.createdAt);
  const sourceLabel = sourceLabelFor(memory);

  return (
    <li
      style={{
        position: "relative",
        paddingLeft: 28,
        paddingRight: 4,
        marginBottom: isLast ? 0 : 14,
      }}
    >
      {/* The marker dot. Filled for the seed; outlined for others. */}
      <span
        aria-hidden
        style={{
          position: "absolute",
          left: 1,
          top: 5,
          width: 13,
          height: 13,
          borderRadius: 999,
          background: node.isSeed ? "var(--accent)" : "var(--bg-1)",
          boxShadow: node.isSeed
            ? "0 0 0 3px var(--accent-soft), inset 0 0 0 1px var(--accent)"
            : "inset 0 0 0 1.5px var(--t-3)",
        }}
      />

      <button
        type="button"
        onClick={onOpen}
        disabled={node.isSeed}
        style={{
          display: "block",
          width: "100%",
          textAlign: "left",
          background: node.isSeed ? "var(--accent-soft)" : "transparent",
          border: "none",
          padding: node.isSeed ? "8px 12px" : "4px 0",
          borderRadius: 8,
          cursor: node.isSeed ? "default" : "pointer",
          fontFamily: "inherit",
          color: "var(--t-1)",
          transition: "background 200ms var(--ease)",
        }}
        onMouseOver={(event) => {
          if (!node.isSeed) {
            (event.currentTarget as HTMLButtonElement).style.background = "var(--bg-hover)";
          }
        }}
        onMouseOut={(event) => {
          if (!node.isSeed) {
            (event.currentTarget as HTMLButtonElement).style.background = "transparent";
          }
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            gap: 8,
            fontSize: 11,
            color: "var(--t-4)",
            letterSpacing: "0.02em",
            marginBottom: 3,
          }}
        >
          <span>{dateLabel}</span>
          {sourceLabel ? (
            <>
              <span style={{ opacity: 0.6 }}>·</span>
              <span>{sourceLabel}</span>
            </>
          ) : null}
        </div>
        <div
          style={{
            fontSize: 13,
            fontWeight: node.isSeed ? 600 : 500,
            color: "var(--t-1)",
            lineHeight: 1.45,
            marginBottom: 4,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={title}
        >
          {title}
        </div>
        <div
          style={{
            fontSize: 11,
            color: "var(--t-3)",
            letterSpacing: "0.01em",
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
          }}
        >
          {node.isSeed ? (
            "this memory"
          ) : (
            <>
              <ArrowRight size={10} strokeWidth={1.9} style={{ opacity: 0.6 }} />
              {node.rationale}
            </>
          )}
        </div>
      </button>
    </li>
  );
}

function sourceLabelFor(memory: Memory): string | null {
  // Prefer the explicit source_app stamp when present (twitter,
  // file, folder, screenshot, ...); fall back to the resolved
  // domain for bookmarks; final fallback is null (clean rendering).
  if (memory.sourceApp) {
    return memory.sourceApp.charAt(0).toUpperCase() + memory.sourceApp.slice(1);
  }
  if (memory.resolvedDomain) {
    return memory.resolvedDomain;
  }
  if (memory.domain) {
    return memory.domain;
  }
  return null;
}
