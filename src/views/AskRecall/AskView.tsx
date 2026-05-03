/**
 * AskView — v0.5.12 Ask Recall surface (multi-turn).
 *
 * Conversation-grounded Q&A across the user's saved memories.
 * Pipeline:
 *
 *   1. View mounts → spawn a backend session via `newAskRecallSession`.
 *      The session id is the conversation handle; backend stores
 *      messages keyed by it.
 *   2. User types a question → click Ask (or ⌘Enter).
 *   3. Frontend appends a "pending user" bubble locally and calls
 *      `askRecall(question, sessionId)` which streams tokens via
 *      `recall://ask-recall-token` events.
 *   4. When the call resolves, we have the full response. Append it
 *      to the local thread as a committed assistant message; backend
 *      already mirrored it into the session.
 *   5. New Chat button drops the session and creates a fresh one.
 *
 * Why thread-style rather than single Q&A: follow-up questions are
 * the natural shape of memory-grounded chat. "What license keys
 * did I save? ... and which one is for Recall?" — the second
 * question only makes sense in context of the first.
 *
 * Resource discipline (unchanged from v0.4.3):
 *   * Tauri event listeners tear down on unmount.
 *   * Backend serializes generation; only one turn in flight at a
 *     time across the whole app.
 *   * Cancel button (v0.5.11) flips a flag the LLM polls every
 *     token — partial answer becomes the committed answer for the
 *     turn, conversation continues normally.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowRight,
  Loader2,
  MessageCircleQuestion,
  Plus,
  Sparkles,
  X,
} from "lucide-react";
import {
  aiClient,
  type AskRecallCitation,
  type AskRecallMessage,
  type AskRecallResponse,
} from "@/services/ai/AiClient";
import { useChatStore } from "@/stores/chatStore";
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
  cancelled?: boolean;
}

interface AskStageEvent {
  stage: "retrieving" | "prefill";
  detail?: { memories?: number };
}

type AskPhase =
  | { kind: "idle" }
  | { kind: "retrieving" }
  | { kind: "prefill"; memories: number }
  | { kind: "generating" };

const CITATION_RE = /\[memory:([0-9a-fA-F\-]+)\]/g;

export function AskView({ setView }: AskViewProps) {
  // v0.5.15: thread state lives in the shared chat store so the
  // sidebar and AskView stay in sync. Pending state (current
  // in-flight turn) stays local since it's short-lived and
  // only meaningful inside this view.
  const sessionId = useChatStore((s) => s.activeSessionId);
  const messages = useChatStore((s) => s.activeMessages);
  const newChatAction = useChatStore((s) => s.newChat);
  const appendMessageToActive = useChatStore((s) => s.appendMessageToActive);
  const refreshChats = useChatStore((s) => s.refresh);

  const [pendingUser, setPendingUser] = useState<string | null>(null);
  const [streamedText, setStreamedText] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [phase, setPhase] = useState<AskPhase>({ kind: "idle" });
  const [question, setQuestion] = useState("");
  const threadEndRef = useRef<HTMLDivElement | null>(null);

  const selectMemory = useMemoryStore((state) => state.selectMemory);

  // v0.5.15: when AskView mounts and there's no active session
  // (e.g., user just clicked "Ask Recall" in the nav for the
  // first time, or after a "New chat" click), spawn one. The
  // store's newChat handles SQLite write + sets active id.
  useEffect(() => {
    if (sessionId) return;
    let disposed = false;
    void newChatAction().catch((err) => {
      if (!disposed) {
        setError(err instanceof Error ? err.message : String(err));
      }
    });
    return () => {
      disposed = true;
    };
  }, [sessionId, newChatAction]);

  // Stream listeners. Tokens append to streamedText; phase events
  // update the spinner copy; complete events are advisory (the
  // resolved promise is the canonical "done").
  useEffect(() => {
    let unlistenToken: UnlistenFn | undefined;
    let unlistenComplete: UnlistenFn | undefined;
    let unlistenStage: UnlistenFn | undefined;
    let disposed = false;

    void listen<AskTokenEvent>("recall://ask-recall-token", (event) => {
      if (disposed) return;
      setStreamedText((prev) => prev + (event.payload?.delta ?? ""));
      setPhase((prev) =>
        prev.kind === "prefill" ? { kind: "generating" } : prev,
      );
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenToken = fn;
      }
    });

    void listen<AskCompleteEvent>("recall://ask-recall-complete", () => {
      // No-op — we use the resolved promise from askRecall instead.
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenComplete = fn;
      }
    });

    void listen<AskStageEvent>("recall://ask-recall-stage", (event) => {
      if (disposed) return;
      const payload = event.payload;
      if (!payload) return;
      if (payload.stage === "retrieving") {
        setPhase({ kind: "retrieving" });
      } else if (payload.stage === "prefill") {
        const memories = payload.detail?.memories ?? 0;
        setPhase({ kind: "prefill", memories });
      }
    }).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlistenStage = fn;
      }
    });

    return () => {
      disposed = true;
      unlistenToken?.();
      unlistenComplete?.();
      unlistenStage?.();
    };
  }, []);

  // Auto-scroll to the bottom whenever the thread grows or the
  // streaming text updates. Smooth scrolling because abrupt jumps
  // mid-token-stream feel jittery.
  useEffect(() => {
    threadEndRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages.length, streamedText, streaming]);

  const ask = useCallback(async () => {
    const trimmed = question.trim();
    if (!trimmed || streaming) return;
    if (!sessionId) {
      setError("Conversation session not ready yet — try again in a moment.");
      return;
    }
    setStreaming(true);
    setError(null);
    setStreamedText("");
    setPendingUser(trimmed);
    setQuestion("");
    setPhase({ kind: "retrieving" });
    try {
      const result: AskRecallResponse = await aiClient.askRecall(
        trimmed,
        sessionId,
      );
      // v0.5.15: append both messages to the shared store so the
      // sidebar's last_used_at + message_count stay in sync. The
      // backend has already persisted them; we mirror locally so
      // the thread renders in order without a session refetch.
      const ts = new Date().toISOString();
      appendMessageToActive({
        role: "user",
        content: trimmed,
        timestamp: ts,
      });
      appendMessageToActive({
        role: "assistant",
        content: result.text,
        retrievedSources: result.retrievedSources,
        citations: result.citations,
        tokensGenerated: result.tokensGenerated,
        latencyMs: result.latencyMs,
        tagIntent: result.tagIntent,
        timestamp: ts,
      });
      // Refresh the sidebar list so the placeholder title (set
      // server-side from the first user message) replaces "New
      // chat". The LLM-generated summary title arrives later via
      // the `recall://ask-recall-session-renamed` event.
      void refreshChats();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setStreaming(false);
      setStreamedText("");
      setPendingUser(null);
      setPhase({ kind: "idle" });
    }
  }, [question, streaming, sessionId, appendMessageToActive, refreshChats]);

  const cancel = useCallback(async () => {
    if (!streaming) return;
    try {
      await aiClient.cancelAskRecall();
    } catch {
      // Best-effort.
    }
  }, [streaming]);

  // v0.5.15: New chat just delegates to the store action; the
  // store creates a fresh session row in SQLite, sets it as
  // active, and clears activeMessages. Local pending state
  // resets here.
  const newChat = useCallback(async () => {
    if (streaming) return;
    setStreamedText("");
    setPendingUser(null);
    setError(null);
    setPhase({ kind: "idle" });
    await newChatAction();
  }, [streaming, newChatAction]);

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

  const hasContent =
    messages.length > 0 || pendingUser !== null || streaming || error !== null;

  // v0.5.13: layout restructured to put the input at the bottom
  // (Claude/ChatGPT pattern). Outer container is a flex column
  // filling the available height; thread scrolls inside its own
  // overflow region; input is pinned at the bottom and always
  // visible without scrolling. `min-height: 0` on the thread is
  // the standard CSS-flex incantation for "let me actually scroll
  // inside this flex item" — without it the thread expands to
  // its content's height and the page itself scrolls.
  return (
    <div
      className="page fade-in"
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        overflow: "hidden",
      }}
    >
      <div className="page-head" style={{ flexShrink: 0 }}>
        <div className="page-eyebrow">
          <Sparkles size={11} strokeWidth={1.7} /> Ask Recall
        </div>
        <h1 className="page-title">Ask your memories.</h1>
        <p className="page-sub">
          Multi-turn Q&amp;A grounded in your saved content — citations link
          back to the memories that backed each claim. Runs fully on-device.
        </p>
        {messages.length > 0 ? (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => void newChat()}
            disabled={streaming}
            style={{ marginTop: 8, height: 28, padding: "0 12px" }}
          >
            <Plus size={12} strokeWidth={1.8} />
            New chat
          </button>
        ) : null}
      </div>

      {/* Thread — flex:1 fills remaining space; overflow:auto so the
          conversation scrolls inside it rather than scrolling the
          whole page. min-height:0 is required for flex children to
          actually constrain to parent height instead of growing. */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          paddingTop: 4,
          paddingBottom: 12,
          display: "flex",
          flexDirection: "column",
          gap: 14,
        }}
      >
        {error ? (
          <div
            style={{
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

        {hasContent ? (
          <>
            {messages.map((msg, idx) => (
              <MessageRow
                key={`${idx}-${msg.timestamp}`}
                message={msg}
                onOpenMemory={openMemory}
              />
            ))}
            {pendingUser ? <UserBubble content={pendingUser} /> : null}
            {streaming ? (
              <AssistantStreamingBubble
                text={streamedText}
                onOpenMemory={openMemory}
              />
            ) : null}
            <div ref={threadEndRef} />
          </>
        ) : (
          <div
            style={{
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
            enough context it will say so rather than guess. Follow-ups stay
            in the same conversation; click "New chat" to reset.
          </div>
        )}
      </div>

      {/* Input box — pinned at the bottom of the view. flex-shrink:0
          keeps it visible no matter how tall the thread grows. The
          thread auto-scrolls to its end when new tokens land
          (threadEndRef effect), so the user always sees the latest
          assistant message immediately above the input. */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          flexDirection: "column",
          gap: 10,
          padding: 14,
          borderRadius: 14,
          background: "var(--panel-glass)",
          boxShadow: "0 0 0 0.5px var(--sh-window-edge)",
          marginTop: 8,
        }}
      >
        <textarea
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={
            messages.length === 0
              ? "What did I save about… (e.g., 'pricing notes from last month')"
              : "Follow up… (or click New chat to start over)"
          }
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
            {streaming ? phaseCopy(phase) : "⌘ + Enter to ask"}
          </span>
          {streaming ? (
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => void cancel()}
              style={{ marginLeft: "auto", height: 32, padding: "0 12px" }}
              title="Stop generation and keep the partial answer"
            >
              <X size={12} strokeWidth={1.8} />
              Cancel
            </button>
          ) : null}
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void ask()}
            disabled={streaming || question.trim().length === 0 || !sessionId}
            style={{
              marginLeft: streaming ? 0 : "auto",
              height: 32,
              padding: "0 14px",
            }}
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
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Per-message rendering. Three flavors: committed user, committed
   assistant, and the streaming-in-progress assistant.
   ──────────────────────────────────────────────────────────────────────── */

function MessageRow({
  message,
  onOpenMemory,
}: {
  message: AskRecallMessage;
  onOpenMemory: (memoryId: string) => void;
}) {
  if (message.role === "user") {
    return <UserBubble content={message.content} />;
  }
  return (
    <AssistantBubble
      content={message.content}
      retrievedSources={message.retrievedSources}
      citations={message.citations}
      tagIntent={message.tagIntent}
      tokensGenerated={message.tokensGenerated}
      latencyMs={message.latencyMs}
      onOpenMemory={onOpenMemory}
    />
  );
}

function UserBubble({ content }: { content: string }) {
  return (
    <div
      style={{
        alignSelf: "flex-end",
        maxWidth: "85%",
        padding: "10px 14px",
        borderRadius: 14,
        background: "var(--accent-soft, var(--panel))",
        color: "var(--t-1)",
        fontSize: 14,
        lineHeight: 1.5,
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
        userSelect: "text",
        WebkitUserSelect: "text",
        cursor: "text",
      }}
    >
      {content}
    </div>
  );
}

function AssistantBubble({
  content,
  retrievedSources,
  citations,
  tagIntent,
  tokensGenerated,
  latencyMs,
  onOpenMemory,
}: {
  content: string;
  retrievedSources: AskRecallCitation[];
  citations: AskRecallCitation[];
  tagIntent: string | null;
  tokensGenerated: number;
  latencyMs: number;
  onOpenMemory: (memoryId: string) => void;
}) {
  const citationById = useMemo(() => {
    const map = new Map<string, AskRecallCitation>();
    for (const c of citations) map.set(c.memoryId, c);
    return map;
  }, [citations]);
  const renderedAnswer = useMemo(
    () => renderAnswerWithCitations(content, citationById, onOpenMemory),
    [content, citationById, onOpenMemory],
  );
  const sources = retrievedSources.length > 0 ? retrievedSources : citations;
  const tagLabel = tagIntent ? ` matching "${tagIntent}"` : "";
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
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
          userSelect: "text",
          WebkitUserSelect: "text",
          cursor: "text",
        }}
      >
        {renderedAnswer}
      </div>
      {sources.length > 0 ? (
        <SourceList
          sources={sources}
          tagLabel={tagLabel}
          onOpenMemory={onOpenMemory}
        />
      ) : null}
      <div style={{ fontSize: 11, color: "var(--t-4)" }}>
        {tokensGenerated} tokens · {(latencyMs / 1000).toFixed(1)}s
      </div>
    </div>
  );
}

function AssistantStreamingBubble({
  text,
  onOpenMemory,
}: {
  text: string;
  onOpenMemory: (memoryId: string) => void;
}) {
  // No citations available yet during streaming — markers render as
  // plain text until the call resolves.
  const empty = useMemo(() => new Map<string, AskRecallCitation>(), []);
  const renderedAnswer = useMemo(
    () => renderAnswerWithCitations(text, empty, onOpenMemory),
    [text, empty, onOpenMemory],
  );
  return (
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
        userSelect: "text",
        WebkitUserSelect: "text",
        cursor: "text",
      }}
    >
      {renderedAnswer}
      <span className="cursor-blink">▍</span>
    </div>
  );
}

function SourceList({
  sources,
  tagLabel,
  onOpenMemory,
}: {
  sources: AskRecallCitation[];
  tagLabel: string;
  onOpenMemory: (memoryId: string) => void;
}) {
  return (
    <div>
      <div
        style={{
          fontSize: 11,
          color: "var(--t-4)",
          textTransform: "uppercase",
          letterSpacing: 0.6,
          marginBottom: 6,
        }}
      >
        Sources ({sources.length}
        {tagLabel})
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {sources.map((c) => (
          <button
            key={c.memoryId}
            type="button"
            onClick={() => onOpenMemory(c.memoryId)}
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
            <div
              style={{
                fontSize: 12,
                fontWeight: 600,
                marginBottom: 4,
                userSelect: "text",
                WebkitUserSelect: "text",
              }}
            >
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
                userSelect: "text",
                WebkitUserSelect: "text",
              }}
            >
              {c.chunkText}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Helpers — phase copy, citation rewrite. Unchanged in shape from
   v0.5.11 except citation rewrite now accepts an empty map for the
   streaming-bubble case.
   ──────────────────────────────────────────────────────────────────────── */

function phaseCopy(phase: AskPhase): string {
  switch (phase.kind) {
    case "idle":
      return "⌘ + Enter to ask";
    case "retrieving":
      return "Searching memories…";
    case "prefill":
      return phase.memories === 1
        ? "Reading 1 memory…"
        : `Reading ${phase.memories} memories…`;
    case "generating":
      return "Generating…";
  }
}

function renderAnswerWithCitations(
  text: string,
  citationById: Map<string, AskRecallCitation>,
  onClick: (memoryId: string) => void,
): React.ReactNode {
  if (!text) return null;
  const nodes: React.ReactNode[] = [];
  let cursor = 0;
  let chipIndex = 0;
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
