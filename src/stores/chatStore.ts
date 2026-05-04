/**
 * v0.5.15 — Ask Recall conversations store.
 *
 * Owns the sidebar's RECENT CHATS list, the currently-active
 * conversation id, and the rehydrated thread for that session.
 * Backed entirely by SQLite via the Tauri commands; this store is
 * a thin cache + local-only optimistic-update layer.
 *
 * What lives here:
 *   • `sessions` — the list shown in the left sidebar
 *   • `activeSessionId` — which chat is currently open in AskView
 *   • `activeMessages` — that chat's full message history (loaded
 *     on demand when the session is opened)
 *
 * What does NOT live here:
 *   • Streaming token state (lives in AskView local state — short-
 *     lived, not worth promoting to a global store)
 *   • Cancel handle (per-process, not per-session in v0.5.x)
 *
 * The store exposes async actions (`refresh`, `newChat`, etc.)
 * that wrap the AiClient calls, mutate local state on success,
 * and return success/error so the caller can render error states.
 */

import { create } from "zustand";
import {
  aiClient,
  chatDisplayTitle,
  type AskRecallMessage,
  type AskRecallSessionFull,
  type AskRecallSessionSummary,
} from "@/services/ai/AiClient";

interface ChatStoreState {
  sessions: AskRecallSessionSummary[];
  activeSessionId: string | null;
  activeMessages: AskRecallMessage[];
  /// True while the sidebar list is being fetched. Used to gate
  /// the empty-state copy ("No conversations yet" only shows
  /// once we've actually loaded and confirmed the list is empty).
  hydrating: boolean;

  /// Refresh the sidebar list from the backend. Cheap — single
  /// SQL query newest-first. Called on app boot, after creating
  /// or deleting a chat, and when the title-renamed event fires.
  refresh: () => Promise<void>;

  /// Create a new session, append it to the local list (so the
  /// sidebar updates instantly), set it as active, and clear
  /// activeMessages. Returns the new session id.
  newChat: () => Promise<string | null>;

  /// Load an existing session and set it as active. Falls back
  /// to a fresh chat if the session id has been deleted from
  /// another surface.
  openChat: (sessionId: string) => Promise<void>;

  /// Drop a session. If it was active, fall back to no-active
  /// state so the AskView shows the empty "Ask anything" surface
  /// rather than a stale thread.
  deleteChat: (sessionId: string) => Promise<void>;

  /// Manually rename a chat. Optimistic: updates the local list
  /// before the backend confirms; reverts on failure.
  renameChat: (sessionId: string, title: string) => Promise<{ ok: boolean; error?: string }>;

  /// Append a message to activeMessages locally. Called by
  /// AskView after a turn completes so the thread stays in sync
  /// without a full refetch round-trip.
  appendMessageToActive: (message: AskRecallMessage) => void;

  /// v0.5.17: append a message to a SPECIFIC session. If that
  /// session is currently active, the message is mirrored into
  /// `activeMessages` for instant render. Otherwise we just
  /// bump the sidebar row's count + last_used_at — the next
  /// `openChat` for that session will refetch from SQLite and
  /// pick up the persisted message.
  ///
  /// This exists because Ask Recall turns can outlive a session
  /// switch: a user can ask a question in chat A, switch to
  /// chat B mid-stream, and the completion handler must persist
  /// to A — never to whichever session happens to be active
  /// when the await resolves.
  appendMessageToSession: (sessionId: string, message: AskRecallMessage) => void;

  /// Apply a server-side title rename event (LLM-generated title
  /// landed). Updates the matching sidebar row in place.
  applyTitleEvent: (sessionId: string, title: string) => void;
}

export const useChatStore = create<ChatStoreState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  activeMessages: [],
  hydrating: true,

  async refresh() {
    try {
      const sessions = await aiClient.listAskRecallSessions();
      set({ sessions, hydrating: false });
    } catch (err) {
      console.error("[chatStore] refresh failed:", err);
      set({ hydrating: false });
    }
  },

  async newChat() {
    try {
      const sessionId = await aiClient.newAskRecallSession();
      // Optimistically add the row. Title is the placeholder
      // until the user's first turn lands; refresh() after the
      // first turn picks up the LLM-generated title.
      const now = new Date().toISOString();
      set((state) => ({
        sessions: [
          {
            sessionId,
            title: "New chat",
            llmTitle: null,
            createdAt: now,
            lastUsedAt: now,
            messageCount: 0,
          },
          ...state.sessions,
        ],
        activeSessionId: sessionId,
        activeMessages: [],
      }));
      return sessionId;
    } catch (err) {
      console.error("[chatStore] newChat failed:", err);
      return null;
    }
  },

  async openChat(sessionId) {
    try {
      const session = await aiClient.getAskRecallSession(sessionId);
      if (!session) {
        // Session was deleted from another surface; fall back
        // to a fresh chat so the user isn't stuck.
        await get().newChat();
        return;
      }
      set({
        activeSessionId: session.sessionId,
        activeMessages: session.messages,
      });
    } catch (err) {
      console.error("[chatStore] openChat failed:", err);
    }
  },

  async deleteChat(sessionId) {
    try {
      await aiClient.deleteAskRecallSession(sessionId);
      set((state) => {
        const filtered = state.sessions.filter((s) => s.sessionId !== sessionId);
        const wasActive = state.activeSessionId === sessionId;
        return {
          sessions: filtered,
          activeSessionId: wasActive ? null : state.activeSessionId,
          activeMessages: wasActive ? [] : state.activeMessages,
        };
      });
    } catch (err) {
      console.error("[chatStore] deleteChat failed:", err);
    }
  },

  async renameChat(sessionId, title) {
    const before = get().sessions;
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.sessionId === sessionId ? { ...s, title } : s,
      ),
    }));
    try {
      await aiClient.renameAskRecallSession(sessionId, title);
      return { ok: true };
    } catch (err) {
      // Revert on failure.
      set({ sessions: before });
      return {
        ok: false,
        error: err instanceof Error ? err.message : "Rename failed.",
      };
    }
  },

  appendMessageToActive(message) {
    set((state) => ({
      activeMessages: [...state.activeMessages, message],
      // Bump the active session's last_used_at + count locally
      // so the sidebar reflects activity without a full refetch.
      sessions: state.sessions.map((s) =>
        s.sessionId === state.activeSessionId
          ? {
              ...s,
              lastUsedAt: new Date().toISOString(),
              messageCount: s.messageCount + 1,
            }
          : s,
      ),
    }));
  },

  appendMessageToSession(sessionId, message) {
    set((state) => {
      const isActive = state.activeSessionId === sessionId;
      return {
        activeMessages: isActive
          ? [...state.activeMessages, message]
          : state.activeMessages,
        sessions: state.sessions.map((s) =>
          s.sessionId === sessionId
            ? {
                ...s,
                lastUsedAt: new Date().toISOString(),
                messageCount: s.messageCount + 1,
              }
            : s,
        ),
      };
    });
  },

  applyTitleEvent(sessionId, title) {
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.sessionId === sessionId ? { ...s, llmTitle: title } : s,
      ),
    }));
  },
}));

export { chatDisplayTitle };
