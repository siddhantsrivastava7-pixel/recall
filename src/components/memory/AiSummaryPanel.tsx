/**
 * v0.5.18 — AI summary panel for Daily recap memories.
 *
 * Renders the LLM-generated summary at the top of a Daily recap
 * memory's detail view. Three states:
 *
 *   1. Cached and fresh → render the summary text directly.
 *   2. Cached but stale (memory.updatedAt > aiSummaryGeneratedAt
 *      by more than a small grace window) → render the cached
 *      text immediately AND kick off a regeneration in the
 *      background. Cached → fresh swap when the call resolves.
 *   3. Missing → render a one-line skeleton ("Generating
 *      summary…") and trigger generation on mount.
 *
 * Generation is fire-and-forget per panel mount — we intentionally
 * don't dedupe across multiple opens of the same memory in one
 * session because the user almost never re-opens the same recap
 * twice in 5 minutes, and the dedupe machinery would be more code
 * than it's worth at v0.5.x scale.
 *
 * Errors render a small fallback line ("Couldn't generate — try
 * again") with a retry button that re-fires generation. The body
 * of the recap memory still renders the rule-based summary below
 * this panel, so the user is never left with no summary at all.
 */

import { Sparkles, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { aiClient } from "@/services/ai/AiClient";
import type { Memory } from "@/domain/types";

interface AiSummaryPanelProps {
  memory: Memory;
}

/// True when this memory is the per-day Daily recap memory. Backend
/// stamps these as `sourceApp = "spoken"` AND `externalId` starting
/// with `"spoken-daily:"` (the prefix kept for backwards compat
/// with v0.1.x users who already had a Daily transcript memory).
export function isDailyRecapMemory(memory: Memory): boolean {
  return (
    memory.sourceApp === "spoken" &&
    Boolean(memory.externalId?.startsWith("spoken-daily:"))
  );
}

/// Five-minute grace window. The recap is rebuilt on every save
/// (post-save hook), which bumps `updatedAt` even when the body
/// changes by one bullet point. Without a grace window, every
/// Saved-notes-section addition would invalidate the cached AI
/// summary and re-fire generation. Five minutes lets routine
/// captures coalesce — only the first detail-view open after a
/// stretch of activity triggers regeneration.
const STALE_GRACE_MS = 5 * 60 * 1000;

function isSummaryStale(memory: Memory): boolean {
  if (!memory.aiSummaryGeneratedAt) return true;
  const generatedAt = new Date(memory.aiSummaryGeneratedAt).getTime();
  const updatedAt = new Date(memory.updatedAt).getTime();
  if (Number.isNaN(generatedAt) || Number.isNaN(updatedAt)) return false;
  return updatedAt - generatedAt > STALE_GRACE_MS;
}

export function AiSummaryPanel({ memory }: AiSummaryPanelProps) {
  // We seed local state from the memory row. Once a regeneration
  // resolves, we update local state directly so the user sees the
  // new summary without a memory-store refetch round trip; the
  // backend has already persisted it so the next open of the
  // memory detail picks it up too.
  const [summary, setSummary] = useState<string | null>(memory.aiSummary ?? null);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Track the memory id we fired for so a navigate-to-different-recap
  // doesn't produce a stale write into the new memory's panel.
  const lastFiredFor = useRef<string | null>(null);

  const generate = useCallback(async () => {
    setGenerating(true);
    setError(null);
    try {
      const result = await aiClient.generateDailyRecapSummary(memory.id);
      // Only commit if the panel is still showing the same memory
      // it was when generation kicked off.
      if (lastFiredFor.current === memory.id) {
        setSummary(result.summary);
      }
    } catch (err) {
      if (lastFiredFor.current === memory.id) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (lastFiredFor.current === memory.id) {
        setGenerating(false);
      }
    }
  }, [memory.id]);

  // Reset local state and fire generation when the memory changes
  // OR when the cached summary is stale relative to the memory's
  // updatedAt.
  useEffect(() => {
    lastFiredFor.current = memory.id;
    const cached = memory.aiSummary ?? null;
    setSummary(cached);
    setError(null);
    if (cached && !isSummaryStale(memory)) {
      // Fresh cache hit — nothing to do.
      return;
    }
    void generate();
  }, [memory.id, memory.aiSummary, memory.aiSummaryGeneratedAt, memory.updatedAt, generate]);

  return (
    <section
      style={{
        marginBottom: 22,
        padding: "14px 16px",
        borderRadius: 12,
        background: "var(--panel-glass, var(--panel))",
        boxShadow: "0 0 0 0.5px var(--sh-window-edge)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          marginBottom: 10,
          color: "var(--t-3)",
          fontSize: 11,
          fontWeight: 650,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
        }}
      >
        <Sparkles size={11} strokeWidth={1.9} />
        AI Summary
        {generating ? (
          <span
            style={{
              marginLeft: "auto",
              fontSize: 10,
              fontWeight: 500,
              letterSpacing: 0,
              textTransform: "none",
              color: "var(--t-4)",
            }}
          >
            {summary ? "Refreshing…" : "Generating…"}
          </span>
        ) : null}
      </div>

      {summary ? (
        <div
          style={{
            fontSize: 14,
            lineHeight: 1.6,
            color: "var(--t-1)",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            userSelect: "text",
            WebkitUserSelect: "text",
          }}
        >
          {summary}
        </div>
      ) : generating ? (
        <div
          style={{
            fontSize: 13,
            lineHeight: 1.6,
            color: "var(--t-3)",
          }}
        >
          Reading your captures and writing a summary of the day…
        </div>
      ) : error ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ fontSize: 12, color: "var(--bad)" }}>
            Couldn't generate — {error}
          </div>
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => void generate()}
            style={{
              alignSelf: "flex-start",
              height: 28,
              padding: "0 10px",
              fontSize: 12,
            }}
          >
            <RefreshCw size={11} strokeWidth={1.8} />
            Try again
          </button>
        </div>
      ) : (
        <div style={{ fontSize: 13, color: "var(--t-3)" }}>
          No summary yet.
        </div>
      )}
    </section>
  );
}
