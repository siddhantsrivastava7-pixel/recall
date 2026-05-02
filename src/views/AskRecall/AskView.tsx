/**
 * AskView — v0.4.3 Ask Recall surface.
 *
 * Single-shot Q&A grounded in the user's saved memories. Pipeline:
 *
 *   1. User types a question → click Ask (or ⌘Enter).
 *   2. Frontend kicks off the `ask_recall` Tauri command and starts
 *      listening to `recall://ask-recall-token` events.
 *   3. Tokens stream into a transcript area as they arrive.
 *   4. When `ask_recall` resolves we have the full answer + a list of
 *      `[memory:<uuid>]` citations the LLM emitted. We rewrite each
 *      marker into a small clickable chip that opens the underlying
 *      memory in the All Memories view.
 *
 * Why a dedicated view (not a modal): on Tier C / 7B Q4_K_M the
 * generation latency is ~2.6 tok/s, so a 300-token answer takes
 * ~115s. Streaming makes the wait feel like reading along; a
 * full-screen surface gives that reading the room it deserves.
 *
 * Resource discipline:
 *   * No background polling — events flow through Tauri's listener
 *     channel and tear down on unmount.
 *   * The model is loaded lazily on first `ask_recall`. Settings
 *     owns the unload UX; this view never touches lifecycle.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ArrowRight, Loader2, MessageCircleQuestion, Sparkles } from "lucide-react";
import {
  aiClient,
  type AskRecallCitation,
  type AskRecallResponse,
} from "@/services/ai/AiClient";
import { useMemoryStore } from "@/stores/memoryStore";
import type { MainView } from "@/windows/MainWindow";

interface AskViewProps {
  setView: (view: MainView) => void;
}

interface AskTokenEvent {
  delta: string;
}

interface AskCompleteEvent {
  tokens: number;
  latencyMs: number;
}

const CITATION_RE = /\[memory:([0-9a-fA-F\-]+)\]/g;

export function AskView({ setView }: AskViewProps) {
  const [question, setQuestion] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [streamedText, setStreamedText] = useState("");
  const [response, setResponse] = useState<AskRecallResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [completionMeta, setCompletionMeta] = useState<AskCompleteEvent | null>(null);
  const lastQuestion = useRef<string>("");

  const selectMemory = useMemoryStore((state) => state.selectMemory);

  // Wire up streaming token listener for the duration of the view.
  // Tokens arrive whether or not we currently care; we only append to
  // `streamedText` while we're actively streaming so a stale token
  // can't pollute a new question.
  useEffect(() => {
    let unlistenToken: UnlistenFn | undefined;
    let unlistenComplete: UnlistenFn | undefined;
    let disposed = false;

    void listen<AskTokenEvent>("recall://ask-recall-token", (event) => {
      if (disposed) return;
      setStreamedText((prev) => prev + (event.payload?.delta ?? ""));
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenToken = fn;
      }
    });

    void listen<AskCompleteEvent>("recall://ask-recall-complete", (event) => {
      if (disposed) return;
      setCompletionMeta(event.payload ?? null);
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenComplete = fn;
      }
    });

    return () => {
      disposed = true;
      unlistenToken?.();
      unlistenComplete?.();
    };
  }, []);

  const ask = useCallback(async () => {
    const trimmed = question.trim();
    if (!trimmed || streaming) return;
    setStreaming(true);
    setError(null);
    setStreamedText("");
    setResponse(null);
    setCompletionMeta(null);
    lastQuestion.current = trimmed;
    try {
      const result = await aiClient.askRecall(trimmed);
      setResponse(result);
      // Backend's `text` is the canonical answer; replace any partial
      // streamed text with it so the citation marker positions line
      // up exactly with what the citation parser saw.
      setStreamedText(result.text);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setStreaming(false);
    }
  }, [question, streaming]);

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        event.preventDefault();
        void ask();
      }
    },
    [ask],
  );

  const openMemory = useCallback(
    (memoryId: string) => {
      selectMemory(memoryId);
      setView("memories");
    },
    [selectMemory, setView],
  );

  // Build a citation lookup by memory id for fast chip rendering.
  const citationById = useMemo(() => {
    const map = new Map<string, AskRecallCitation>();
    for (const c of response?.citations ?? []) {
      map.set(c.memoryId, c);
    }
    return map;
  }, [response?.citations]);

  // Render the streamed/final text with `[memory:<uuid>]` markers
  // rewritten into clickable chips. We re-run this on every token
  // tick — it's a few regex passes over a string under ~2 KB so
  // the cost is negligible.
  const renderedAnswer = useMemo(
    () => renderAnswerWithCitations(streamedText, citationById, openMemory),
    [streamedText, citationById, openMemory],
  );

  const hasAnswer = streamedText.length > 0 || streaming;

  return (
    <div className="page fade-in">
      <div className="page-head">
        <div className="page-eyebrow">
          <Sparkles size={11} strokeWidth={1.7} /> Ask Recall
        </div>
        <h1 className="page-title">Ask your memories.</h1>
        <p className="page-sub">
          Single-shot Q&amp;A grounded in your saved content — citations link
          back to the memories that backed each claim. Runs fully on-device.
        </p>
      </div>

      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 10,
          padding: 14,
          borderRadius: 14,
          background: "var(--panel-glass)",
          boxShadow: "0 0 0 0.5px var(--sh-window-edge)",
        }}
      >
        <textarea
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="What did I save about… (e.g., 'pricing notes from last month')"
          rows={3}
          disabled={streaming}
          style={{
            width: "100%",
            resize: "vertical",
            border: "none",
            outline: "none",
            background: "transparent",
            color: "var(--t-1)",
            fontSize: 14,
            lineHeight: 1.5,
            fontFamily: "inherit",
            padding: 0,
          }}
        />
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 11, color: "var(--t-4)" }}>
            {streaming ? "Generating…" : "⌘ + Enter to ask"}
          </span>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void ask()}
            disabled={streaming || question.trim().length === 0}
            style={{ marginLeft: "auto", height: 32, padding: "0 14px" }}
          >
            {streaming ? (
              <>
                <Loader2 size={12} strokeWidth={1.8} className="spin" />
                Thinking
              </>
            ) : (
              <>
                <MessageCircleQuestion size={12} strokeWidth={1.8} />
                Ask
                <ArrowRight size={11} strokeWidth={1.8} />
              </>
            )}
          </button>
        </div>
      </div>

      {error ? (
        <div
          style={{
            marginTop: 14,
            padding: "10px 12px",
            borderRadius: 10,
            background: "var(--bad-bg)",
            color: "var(--bad)",
            fontSize: 12,
          }}
        >
          {error}
        </div>
      ) : null}

      {hasAnswer ? (
        <div style={{ marginTop: 18 }}>
          <div
            style={{
              fontSize: 11,
              color: "var(--t-4)",
              textTransform: "uppercase",
              letterSpacing: 0.6,
              marginBottom: 8,
            }}
          >
            {streaming ? "Streaming answer" : "Answer"}
          </div>
          <div
            style={{
              padding: 16,
              borderRadius: 12,
              background: "var(--panel)",
              boxShadow: "0 0 0 0.5px var(--sh-window-edge)",
              fontSize: 14,
              lineHeight: 1.6,
              color: "var(--t-1)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
            }}
          >
            {renderedAnswer}
            {streaming ? <span className="cursor-blink">▍</span> : null}
          </div>

          {response && response.citations.length > 0 ? (
            <div style={{ marginTop: 14 }}>
              <div
                style={{
                  fontSize: 11,
                  color: "var(--t-4)",
                  textTransform: "uppercase",
                  letterSpacing: 0.6,
                  marginBottom: 8,
                }}
              >
                Sources ({response.citations.length})
              </div>
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                }}
              >
                {response.citations.map((c) => (
                  <button
                    key={c.memoryId}
                    type="button"
                    onClick={() => openMemory(c.memoryId)}
                    style={{
                      textAlign: "left",
                      padding: "10px 12px",
                      borderRadius: 10,
                      border: "none",
                      background: "var(--panel)",
                      boxShadow: "0 0 0 0.5px var(--sh-window-edge)",
                      cursor: "pointer",
                      color: "var(--t-1)",
                    }}
                  >
                    <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 4 }}>
                      {c.title || "Untitled memory"}
                    </div>
                    <div
                      style={{
                        fontSize: 11,
                        color: "var(--t-3)",
                        display: "-webkit-box",
                        WebkitLineClamp: 3,
                        WebkitBoxOrient: "vertical",
                        overflow: "hidden",
                      }}
                    >
                      {c.chunkText}
                    </div>
                  </button>
                ))}
              </div>
            </div>
          ) : null}

          {response && completionMeta ? (
            <div
              style={{
                marginTop: 12,
                fontSize: 11,
                color: "var(--t-4)",
              }}
            >
              {response.contextChunks} memor
              {response.contextChunks === 1 ? "y" : "ies"} cited ·{" "}
              {completionMeta.tokens} tokens · {(completionMeta.latencyMs / 1000).toFixed(1)}s
            </div>
          ) : null}
        </div>
      ) : (
        <div
          style={{
            marginTop: 18,
            padding: "12px 14px",
            borderRadius: 12,
            border: "1px dashed var(--sh-window-edge)",
            color: "var(--t-3)",
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          Tips: be specific. Ask for "license keys I saved" or "what did I
          save about pricing last week" rather than "summarize everything".
          Recall only answers from your own memories — if it doesn't have
          enough context it will say so rather than guess.
        </div>
      )}
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Citation rendering — splits the answer text on every `[memory:<uuid>]`
   marker and replaces matching ids with a small inline button. Markers
   for ids the citation parser dropped (rare — usually a malformed uuid)
   render as plain text so the user still sees the LLM's intent.
   ──────────────────────────────────────────────────────────────────────── */

function renderAnswerWithCitations(
  text: string,
  citationById: Map<string, AskRecallCitation>,
  onClick: (memoryId: string) => void,
): React.ReactNode {
  if (!text) return null;
  const nodes: React.ReactNode[] = [];
  let cursor = 0;
  let chipIndex = 0;
  // Build a stable index per memory id so the same citation always
  // gets the same number, even if it appears multiple times.
  const numberByMemoryId = new Map<string, number>();
  let nextNumber = 1;
  const re = new RegExp(CITATION_RE.source, "g");
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    const [marker, memoryId] = match;
    if (match.index > cursor) {
      nodes.push(text.slice(cursor, match.index));
    }
    if (citationById.has(memoryId)) {
      let n = numberByMemoryId.get(memoryId);
      if (n === undefined) {
        n = nextNumber++;
        numberByMemoryId.set(memoryId, n);
      }
      nodes.push(
        <button
          key={`${memoryId}-${chipIndex++}`}
          type="button"
          onClick={() => onClick(memoryId)}
          title={citationById.get(memoryId)?.title ?? memoryId}
          style={{
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            minWidth: 18,
            height: 18,
            padding: "0 5px",
            margin: "0 2px",
            borderRadius: 4,
            border: "none",
            background: "var(--accent-soft, var(--panel))",
            color: "var(--accent, var(--t-1))",
            fontSize: 10,
            fontWeight: 600,
            cursor: "pointer",
            verticalAlign: "baseline",
            lineHeight: 1.2,
          }}
        >
          {n}
        </button>,
      );
    } else {
      nodes.push(marker);
    }
    cursor = match.index + marker.length;
  }
  if (cursor < text.length) {
    nodes.push(text.slice(cursor));
  }
  return nodes;
}
