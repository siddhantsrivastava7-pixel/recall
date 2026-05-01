// Related-memories panel rendered on the memory detail view (v0.3.0+).
//
// Shows up to 5 semantically-similar memories ranked by chunk-level
// cosine + MMR diversity, with a one-line excerpt from the best-matching
// chunk. Clicking a result opens that memory in the same detail pane.
//
// The list is loaded lazily — calling `find_related` is cheap (a single
// brute-force cosine pass over all embedded chunks) but only the open
// detail view needs the answer, so we wait until the component mounts
// rather than precomputing everywhere.

import { Sparkles, AlertCircle } from "lucide-react";
import { useEffect, useState } from "react";

import type { Memory } from "@/domain/types";
import { aiClient, type RelatedMemoryView } from "@/services/ai/AiClient";
import { useMemoryStore } from "@/stores/memoryStore";

interface Props {
  memory: Memory;
  onOpenMemory: (memoryId: string) => void;
}

export function RelatedMemories({ memory, onOpenMemory }: Props) {
  const [results, setResults] = useState<RelatedMemoryView[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const memories = useMemoryStore((state) => state.memories);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setNotice(null);
    aiClient
      .findRelated(memory.id, 5)
      .then((rows) => {
        if (cancelled) return;
        setResults(rows);
        if (rows.length === 0) {
          setNotice(
            "Nothing related yet — embeddings may still be queueing, or this memory is too short to match.",
          );
        }
      })
      .catch((error) => {
        if (cancelled) return;
        const msg =
          error instanceof Error ? error.message : "Couldn't load related memories.";
        setNotice(msg);
        setResults([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [memory.id, memory.embeddingGeneratedAt]);

  if (!loading && (results == null || results.length === 0) && !notice) {
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
          marginBottom: 12,
        }}
      >
        <Sparkles size={12} strokeWidth={1.9} />
        Related
      </div>

      {loading && results == null ? (
        <div style={{ fontSize: 13, color: "var(--text-muted)" }}>Looking for related memories…</div>
      ) : null}

      {results && results.length > 0 ? (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))",
            gap: 10,
          }}
        >
          {results.map((row) => {
            const target = memories.find((m) => m.id === row.memoryId);
            const title =
              target?.title ?? target?.summaryText ?? row.chunkText.split("\n")[0] ?? "Untitled";
            const excerpt = makeExcerpt(row.chunkText, 180);
            return (
              <button
                key={row.memoryId}
                type="button"
                onClick={() => onOpenMemory(row.memoryId)}
                style={{
                  textAlign: "left",
                  padding: "12px 14px",
                  borderRadius: 10,
                  background: "rgba(255,255,255,0.02)",
                  border: "1px solid rgba(255,255,255,0.06)",
                  color: "var(--text-primary)",
                  cursor: "pointer",
                  fontFamily: "inherit",
                  display: "flex",
                  flexDirection: "column",
                  gap: 6,
                  transition: "background 100ms",
                }}
                onMouseEnter={(e) =>
                  (e.currentTarget.style.background = "rgba(255,255,255,0.05)")
                }
                onMouseLeave={(e) =>
                  (e.currentTarget.style.background = "rgba(255,255,255,0.02)")
                }
              >
                <div
                  style={{
                    fontSize: 13,
                    fontWeight: 500,
                    color: "var(--text-primary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {title}
                </div>
                <div
                  style={{
                    fontSize: 12,
                    color: "var(--text-muted)",
                    lineHeight: 1.5,
                    display: "-webkit-box",
                    WebkitLineClamp: 2,
                    WebkitBoxOrient: "vertical",
                    overflow: "hidden",
                  }}
                >
                  {excerpt}
                </div>
                <div
                  style={{
                    fontSize: 11,
                    color: "var(--t-4)",
                    marginTop: 2,
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {(row.score * 100).toFixed(0)}% match
                </div>
              </button>
            );
          })}
        </div>
      ) : null}

      {notice ? (
        <div
          style={{
            fontSize: 12,
            color: "var(--text-muted)",
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginTop: 6,
          }}
        >
          <AlertCircle size={12} />
          {notice}
        </div>
      ) : null}
    </section>
  );
}

function makeExcerpt(text: string, maxChars: number): string {
  const collapsed = text.replace(/\s+/g, " ").trim();
  if (collapsed.length <= maxChars) return collapsed;
  return `${collapsed.slice(0, maxChars - 1)}…`;
}
