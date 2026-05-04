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

import {
  Component,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ErrorInfo,
  type ReactNode,
} from "react";
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

/* ────────────────────────────────────────────────────────────────────────
   v0.5.16 — top-level ErrorBoundary around AskView.

   v0.5.15 introduced persistent chat history. When a user clicked an
   old chat in the sidebar, AskView rehydrated `activeMessages` from
   SQLite and re-rendered the thread. A render-throw inside any
   message row (e.g. malformed citations JSON, an unexpected null
   field, a renderer assumption) used to blow up the whole AskView
   subtree and leave the user staring at a blank pane with no clear
   recovery path.

   The boundary catches that throw, surfaces the actual error message
   so we can diagnose, and offers a "Start a new chat" button that
   resets the active session. The user keeps their data — only this
   one session's render is broken — and they're never stuck on a
   blank screen.
   ──────────────────────────────────────────────────────────────────────── */

interface AskViewBoundaryProps {
  setView: (view: MainView) => void;
  children: ReactNode;
}

interface AskViewBoundaryState {
  error: Error | null;
}

class AskViewErrorBoundary extends Component<
  AskViewBoundaryProps,
  AskViewBoundaryState
> {
  state: AskViewBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): AskViewBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Console-only — surfacing this through any persistence layer
    // would itself need its own error handling, and we'd rather
    // not turn one bad render into a crash loop.
    console.error("[AskView] render error:", error, info);
  }

  reset = () => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    if (this.state.error) {
      return (
        <AskViewErrorFallback
          error={this.state.error}
          onReset={this.reset}
        />
      );
    }
    return this.props.children;
  }
}

function AskViewErrorFallback({
  error,
  onReset,
}: {
  error: Error;
  onReset: () => void;
}) {
  const newChatAction = useChatStore((s) => s.newChat);
  const handleNewChat = useCallback(async () => {
    await newChatAction();
    onReset();
  }, [newChatAction, onReset]);
  return (
    <div
      className="page fade-in"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 12,
        padding: 20,
      }}
    >
      <div style={{ fontSize: 13, fontWeight: 600, color: "var(--bad)" }}>
        Couldn&apos;t render this conversation.
      </div>
      <div style={{ fontSize: 12, color: "var(--t-3)", lineHeight: 1.5 }}>
        Something went wrong while loading this chat. Your saved chats
        and memories are safe — only this view crashed. Start a new chat
        to continue.
      </div>
      <pre
        style={{
          fontSize: 11,
          color: "var(--t-4)",
          background: "var(--panel)",
          padding: 10,
          borderRadius: 8,
          maxHeight: 160,
          overflow: "auto",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
        }}
      >
        {error.message || String(error)}
      </pre>
      <button
        type="button"
        className="btn btn-primary"
        onClick={() => void handleNewChat()}
        style={{ alignSelf: "flex-start", height: 32, padding: "0 14px" }}
      >
        <Plus size={12} strokeWidth={1.8} />
        Start a new chat
      </button>
    </div>
  );
}

export function AskView(props: AskViewProps) {
  return (
    <AskViewErrorBoundary setView={props.setView}>
      <AskViewInner {...props} />
    </AskViewErrorBoundary>
  );
}

function AskViewInner({ setView }: AskViewProps) {
  // v0.5.15: thread state lives in the shared chat store so the
  // sidebar and AskView stay in sync. Pending state (current
  // in-flight turn) stays local since it's short-lived and
  // only meaningful inside this view.
  const sessionId = useChatStore((s) => s.activeSessionId);
  const messages = useChatStore((s) => s.activeMessages);
  const newChatAction = useChatStore((s) => s.newChat);
  const appendMessageToSession = useChatStore((s) => s.appendMessageToSession);
  const refreshChats = useChatStore((s) => s.refresh);

  const [pendingUser, setPendingUser] = useState<string | null>(null);
  const [streamedText, setStreamedText] = useState("");
  const [streaming, setStreaming] = useState(false);
  // v0.5.17: which session the in-flight turn was started for.
  // The user can switch chats mid-stream; if they do, we want to
  // hide the streaming bubble in the new chat (it doesn't belong
  // there) and route the completed messages back to the original
  // session — never to whichever session happens to be active
  // when the await resolves.
  const [turnSessionId, setTurnSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [phase, setPhase] = useState<AskPhase>({ kind: "idle" });
  const [question, setQuestion] = useState("");
  const threadEndRef = useRef<HTMLDivElement | null>(null);

  // True when the in-flight turn (if any) belongs to the currently
  // viewed session. Gates pending+streaming bubble rendering so a
  // mid-stream session switch doesn't leak the OCR turn's bubble
  // into the License Keys chat.
  const turnInThisChat = streaming && turnSessionId === sessionId;

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
    // v0.5.17: capture the originating session id in a closure.
    // If the user switches chats mid-stream, the await below
    // resolves with `useChatStore.activeSessionId` pointing at
    // the WRONG session — we must persist locally to `turnSid`,
    // never to whatever's active at completion time.
    const turnSid = sessionId;
    setStreaming(true);
    setTurnSessionId(turnSid);
    setError(null);
    setStreamedText("");
    setPendingUser(trimmed);
    setQuestion("");
    setPhase({ kind: "retrieving" });
    try {
      const result: AskRecallResponse = await aiClient.askRecall(
        trimmed,
        turnSid,
      );
      // v0.5.15: append both messages to the shared store so the
      // sidebar's last_used_at + message_count stay in sync. The
      // backend has already persisted them; we mirror locally so
      // the thread renders in order without a session refetch.
      //
      // v0.5.17: route to `turnSid` (the originating session),
      // not the currently-active session. The store's
      // `appendMessageToSession` mirrors into `activeMessages`
      // only when `turnSid` IS the active session; otherwise it
      // just bumps the sidebar row's count + last_used_at, and
      // the next `openChat` for `turnSid` refetches the truth.
      const ts = new Date().toISOString();
      appendMessageToSession(turnSid, {
        role: "user",
        content: trimmed,
        timestamp: ts,
      });
      appendMessageToSession(turnSid, {
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
      setTurnSessionId(null);
      setPhase({ kind: "idle" });
    }
  }, [question, streaming, sessionId, appendMessageToSession, refreshChats]);

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

  // v0.5.17: empty-state vs thread-state. Pending/streaming bubbles
  // only count toward "has content" when the in-flight turn belongs
  // to the currently viewed session — otherwise switching to an
  // empty chat mid-stream would briefly hide the empty-state tips
  // even though nothing renders here.
  const hasContent =
    messages.length > 0 ||
    (pendingUser !== null && turnInThisChat) ||
    turnInThisChat ||
    error !== null;

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
            {messages.map((msg, idx) => {
              // v0.5.18: assistant bubbles offer a "Save as memory"
              // button that needs the prior user question to compose
              // the saved Q&A. Walk back to the most recent user
              // message — for a well-formed thread that's idx-1, but
              // we scan defensively in case streaming-cancel left an
              // odd shape.
              let priorUserContent: string | null = null;
              if (msg.role === "assistant") {
                for (let i = idx - 1; i >= 0; i--) {
                  if (messages[i].role === "user") {
                    priorUserContent = messages[i].content ?? null;
                    break;
                  }
                }
              }
              return (
                <MessageRow
                  key={`${idx}-${msg.timestamp}`}
                  message={msg}
                  priorUserContent={priorUserContent}
                  onOpenMemory={openMemory}
                  setView={setView}
                />
              );
            })}
            {pendingUser && turnInThisChat ? (
              <UserBubble content={pendingUser} />
            ) : null}
            {turnInThisChat ? (
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
          {/* v0.5.17: when a turn is in flight in another chat, the
              user has switched away mid-stream. Cancel only makes
              sense for the originating chat (the partial answer
              belongs there), so we hide it here and surface a
              hint that something is generating elsewhere. */}
          <span style={{ fontSize: 11, color: "var(--t-4)" }}>
            {turnInThisChat
              ? phaseCopy(phase)
              : streaming
                ? "Generating in another chat…"
                : "⌘ + Enter to ask"}
          </span>
          {turnInThisChat ? (
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
  priorUserContent,
  onOpenMemory,
  setView,
}: {
  message: AskRecallMessage;
  priorUserContent: string | null;
  onOpenMemory: (memoryId: string) => void;
  setView: (view: MainView) => void;
}) {
  if (message.role === "user") {
    return <UserBubble content={message.content ?? ""} />;
  }
  // v0.5.16: defensive defaults. If a persisted assistant message
  // lands with missing citations / retrievedSources arrays (older
  // schema, partial migration, or a serialization edge case), the
  // render path used to throw on `.map`/`for…of` and blank the
  // whole AskView. Empty-array fallbacks keep the bubble visible
  // even if its source data is malformed.
  return (
    <AssistantBubble
      content={message.content ?? ""}
      priorUserContent={priorUserContent}
      retrievedSources={message.retrievedSources ?? []}
      citations={message.citations ?? []}
      tagIntent={message.tagIntent ?? null}
      tokensGenerated={message.tokensGenerated ?? 0}
      latencyMs={message.latencyMs ?? 0}
      onOpenMemory={onOpenMemory}
      setView={setView}
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
  priorUserContent,
  retrievedSources,
  citations,
  tagIntent,
  tokensGenerated,
  latencyMs,
  onOpenMemory,
  setView,
}: {
  content: string;
  priorUserContent: string | null;
  retrievedSources: AskRecallCitation[];
  citations: AskRecallCitation[];
  tagIntent: string | null;
  tokensGenerated: number;
  latencyMs: number;
  onOpenMemory: (memoryId: string) => void;
  setView: (view: MainView) => void;
}) {
  const citationById = useMemo(() => {
    const map = new Map<string, AskRecallCitation>();
    // v0.5.16: skip nullish entries so a single bad row in
    // persisted citations can't blow up the render of an entire
    // session.
    for (const c of citations ?? []) {
      if (c && c.memoryId) {
        map.set(c.memoryId, c);
      }
    }
    return map;
  }, [citations]);
  const renderedAnswer = useMemo(
    () => renderAnswerWithCitations(content, citationById, onOpenMemory),
    [content, citationById, onOpenMemory],
  );
  const safeRetrieved = retrievedSources ?? [];
  const safeCitations = citations ?? [];
  const sources = safeRetrieved.length > 0 ? safeRetrieved : safeCitations;
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
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
        }}
      >
        <div style={{ fontSize: 11, color: "var(--t-4)" }}>
          {tokensGenerated} tokens · {(latencyMs / 1000).toFixed(1)}s
        </div>
        {/* v0.5.18: opt-in "Save as memory" button. Composes a
            new memory from the question + answer so a useful Q&A
            becomes a first-class searchable memory. Hidden when
            we don't have the question (defensive — should never
            happen for committed assistant bubbles in a normal
            thread). */}
        {priorUserContent ? (
          <SaveQaButton
            question={priorUserContent}
            answer={content}
            onOpenMemory={onOpenMemory}
            setView={setView}
          />
        ) : null}
      </div>
    </div>
  );
}

/// v0.5.18: small button that turns a Q&A into a memory. Three
/// visible states:
///   - idle   → "Save as memory"
///   - saving → "Saving…" (disabled)
///   - saved  → "Saved · Open memory" (clickable, navigates)
///   - error  → "Couldn't save · Try again"
/// The transitions are local — once a Q&A has been saved in this
/// session the button stays in `saved` so the user doesn't double-
/// save by accident. A page refresh resets the state, but the
/// memory is persisted on the backend so re-saving creates a
/// duplicate (acceptable for v0.5.18; v0.5.19 may add dedup).
function SaveQaButton({
  question,
  answer,
  onOpenMemory,
  setView,
}: {
  question: string;
  answer: string;
  onOpenMemory: (memoryId: string) => void;
  setView: (view: MainView) => void;
}) {
  const [state, setState] = useState<
    | { kind: "idle" }
    | { kind: "saving" }
    | { kind: "saved"; memoryId: string }
    | { kind: "error"; message: string }
  >({ kind: "idle" });

  const handleSave = useCallback(async () => {
    if (state.kind === "saving") return;
    setState({ kind: "saving" });
    try {
      const result = await aiClient.saveQaAsMemory(question, answer);
      setState({ kind: "saved", memoryId: result.memoryId });
    } catch (err) {
      setState({
        kind: "error",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, [question, answer, state.kind]);

  const handleOpen = useCallback(() => {
    if (state.kind !== "saved") return;
    onOpenMemory(state.memoryId);
    setView("memories");
  }, [state, onOpenMemory, setView]);

  if (state.kind === "saved") {
    return (
      <button
        type="button"
        className="btn btn-ghost"
        onClick={handleOpen}
        style={{
          marginLeft: "auto",
          height: 26,
          padding: "0 10px",
          fontSize: 11,
          color: "var(--good, var(--t-2))",
        }}
        title="Open the saved Q&A memory"
      >
        ✓ Saved · Open memory
      </button>
    );
  }
  if (state.kind === "error") {
    return (
      <button
        type="button"
        className="btn btn-ghost"
        onClick={() => void handleSave()}
        style={{
          marginLeft: "auto",
          height: 26,
          padding: "0 10px",
          fontSize: 11,
          color: "var(--bad)",
        }}
        title={state.message}
      >
        Couldn't save · Try again
      </button>
    );
  }
  return (
    <button
      type="button"
      className="btn btn-ghost"
      onClick={() => void handleSave()}
      disabled={state.kind === "saving"}
      style={{
        marginLeft: "auto",
        height: 26,
        padding: "0 10px",
        fontSize: 11,
      }}
      title="Save this Q&A as a memory you can search later"
    >
      {state.kind === "saving" ? "Saving…" : "Save as memory"}
    </button>
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
