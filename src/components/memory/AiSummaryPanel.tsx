/**
 * v0.5.18 — AI summary panel for Daily recap memories.
 * v0.5.19 — switched to explicit button-triggered generation.
 *
 * Renders the LLM-generated summary at the top of a Daily recap
 * memory's detail view. States:
 *
 *   1. Cached → render the summary directly. Optional Regenerate.
 *   2. No cache → render a Generate button. Click runs the LLM.
 *   3. Generating → spinner.
 *   4. Error → error line + Try again button.
 *
 * Why button-triggered (v0.5.19): the v0.5.18 auto-fire-on-mount
 * variant collided with concurrent LLM access (Ask Recall in
 * flight) and with the post-save hook bumping `updatedAt`, which
 * retriggered the generate effect via deps. The crash showed up
 * 2-3 seconds after opening a recap memory — right when the LLM
 * call would have started or when an unrelated capture's recap
 * rebuild was racing with the open. Making it explicit eliminates
 * the race entirely — the LLM only runs when the user asks for
 * it, and the rule-based summary in the recap body remains
 * available as the always-on fallback.
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

  // Reset local state when the user navigates between recap
  // memories (different memory.id). Don't kick off generation
  // here — that's user-triggered now. The rule-based summary in
  // the recap body still renders below this panel as the always-
  // on fallback.
  useEffect(() => {
    lastFiredFor.current = memory.id;
    setSummary(memory.aiSummary ?? null);
    setError(null);
    setGenerating(false);
  }, [memory.id, memory.aiSummary]);

  const generate = useCallback(async () => {
    if (generating) return;
    lastFiredFor.current = memory.id;
    setGenerating(true);
    setError(null);
    try {
      // 90s frontend cap. The LLM call itself usually returns in
      // 2-15s on tier B. If it's hanging past 90s something is
      // wrong on the backend (model crashed, deadlock, OOM in
      // progress) — give up so the UI doesn't sit on a spinner
      // forever and the user can retry.
      const TIMEOUT_MS = 90_000;
      const timeoutPromise = new Promise<never>((_, reject) => {
        setTimeout(
          () =>
            reject(
              new Error(
                "Generation timed out. The model may be loading — try again in a moment.",
              ),
            ),
          TIMEOUT_MS,
        );
      });
      const result = await Promise.race([
        aiClient.generateDailyRecapSummary(memory.id),
        timeoutPromise,
      ]);
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
  }, [memory.id, generating]);

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
          marginBottom: summary || generating || error ? 10 : 0,
          color: "var(--t-3)",
          fontSize: 11,
          fontWeight: 650,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
        }}
      >
        <Sparkles size={11} strokeWidth={1.9} />
        AI Summary
        {summary && !generating ? (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => void generate()}
            title="Re-run the LLM to refresh this summary."
            style={{
              marginLeft: "auto",
              height: 24,
              padding: "0 8px",
              fontSize: 10,
              fontWeight: 500,
              letterSpacing: 0,
              textTransform: "none",
              color: "var(--t-3)",
            }}
          >
            <RefreshCw size={10} strokeWidth={1.8} />
            Regenerate
          </button>
        ) : null}
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
            Couldn&apos;t generate — {error}
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
        // No cached summary, no in-flight generation — explicit CTA.
        // The Summary block in the recap body below is the rule-
        // based fallback the user can read in the meantime.
        <button
          type="button"
          className="btn btn-ghost"
          onClick={() => void generate()}
          style={{
            alignSelf: "flex-start",
            height: 28,
            padding: "0 12px",
            fontSize: 12,
          }}
          title="Run the local LLM to summarize today's captures."
        >
          <Sparkles size={11} strokeWidth={1.8} />
          Generate AI summary
        </button>
      )}
    </section>
  );
}
