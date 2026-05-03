/**
 * MainWindow - "main" label
 *
 * Full desktop shell — depth + grain + 220px source-list sidebar.
 * Routes between: Dashboard, Memories, Projects, Settings.
 */

import { useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  Download,
  FolderOpen,
  LayoutGrid,
  Layers,
  Plus,
  Search,
  Settings,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useChatStore, chatDisplayTitle } from "@/stores/chatStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useUpdateStore } from "@/stores/updateStore";
import { HomeBriefing } from "@/components/dashboard/HomeBriefing";
import { MemoriesView } from "@/components/memory/MemoriesView";
import { ProjectsView } from "@/components/memory/ProjectsView";
import { SettingsView } from "@/components/settings/SettingsView";
import { AskView } from "@/views/AskRecall/AskView";
import { InstantCaptureToast } from "@/components/capture/InstantCaptureToast";
import { RecallMark } from "@/components/system/RecallMark";
import { ThemeToggle } from "@/components/system/ThemeToggle";
import { tauriClient } from "@/services/api/tauri-client";
import { useRecallDataSyncEvents } from "@/hooks/useRecallDataSyncEvents";
import { useResurfaceNotifications } from "@/hooks/useResurfaceNotifications";

export type MainView = "dashboard" | "memories" | "projects" | "ask" | "settings";

export function MainWindow() {
  const [view, setView] = useState<MainView>("dashboard");
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const selectMemory = useMemoryStore((state) => state.selectMemory);
  const settings = useSettingsStore((state) => state.settings);
  const updateAvailable = useUpdateStore((state) => state.updateAvailable);
  const availableVersion = useUpdateStore((state) => state.availableVersion);
  const downloading = useUpdateStore((state) => state.downloading);
  const installing = useUpdateStore((state) => state.installing);
  const maybeCheckOnStartup = useUpdateStore((state) => state.maybeCheckOnStartup);
  const downloadAndInstallUpdate = useUpdateStore((state) => state.downloadAndInstallUpdate);

  useRecallDataSyncEvents();
  useResurfaceNotifications();

  useEffect(() => {
    let disposed = false;
    const unlistenPromise = listen<string>("recall://open-memory", async (event) => {
      if (disposed) return;
      setView("memories");
      selectMemory(event.payload);
    });

    return () => {
      disposed = true;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [selectMemory]);

  useEffect(() => {
    void maybeCheckOnStartup(settings.updateAutoCheckEnabled);
  }, [settings.updateAutoCheckEnabled, maybeCheckOnStartup]);

  const showUpdateBanner =
    updateAvailable &&
    availableVersion !== null &&
    dismissedUpdateVersion !== availableVersion;

  return (
    <div className="window">
      <div className="titlebar">
        <div className="tl-title">Recall</div>
        <ThemeToggle />
      </div>
      <div className="app-body">
        <Sidebar view={view} setView={setView} />
        <main className="main">
          <div className="main-scroll">
            {view === "dashboard" && <HomeBriefing setView={setView} />}
            {view === "memories" && <MemoriesView />}
            {view === "projects" && <ProjectsView setView={setView} />}
            {view === "ask" && <AskView setView={setView} />}
            {view === "settings" && <SettingsView />}
          </div>
          {showUpdateBanner && availableVersion ? (
            <UpdateAvailableBanner
              version={availableVersion}
              busy={downloading || installing}
              onInstall={() => void downloadAndInstallUpdate()}
              onDismiss={() => setDismissedUpdateVersion(availableVersion)}
            />
          ) : null}
          <InstantCaptureToast />
        </main>
      </div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Sidebar — 220px source list with brand mark, Library section, pinned
   projects, and a footer "indexed locally" status row.
   ──────────────────────────────────────────────────────────────────────── */

function Sidebar({ view, setView }: { view: MainView; setView: (v: MainView) => void }) {
  const shortcuts = useSettingsStore((state) => state.shortcuts);
  const memories = useMemoryStore((state) => state.memories);
  const projects = useProjectStore((state) => state.projects);
  const memoryCount = memories.length;
  const projectCount = projects.length;
  const pinned = projects.slice(0, 3);
  const [appVersion, setAppVersion] = useState<string>("");

  // v0.5.15: chat sidebar state. The store hydrates on first
  // mount, then listens for the LLM-title-renamed event so the
  // sidebar reflects async title generation without polling.
  const chatSessions = useChatStore((s) => s.sessions);
  const activeSessionId = useChatStore((s) => s.activeSessionId);
  const chatHydrating = useChatStore((s) => s.hydrating);
  const refreshChats = useChatStore((s) => s.refresh);
  const newChat = useChatStore((s) => s.newChat);
  const openChat = useChatStore((s) => s.openChat);
  const deleteChat = useChatStore((s) => s.deleteChat);
  const applyTitleEvent = useChatStore((s) => s.applyTitleEvent);

  useEffect(() => {
    void refreshChats();
  }, [refreshChats]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen<{ sessionId: string; title: string }>(
      "recall://ask-recall-session-renamed",
      (event) => {
        if (disposed) return;
        if (!event.payload) return;
        applyTitleEvent(event.payload.sessionId, event.payload.title);
      },
    ).then((fn) => {
      if (disposed) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyTitleEvent]);

  useEffect(() => {
    let active = true;
    void getVersion()
      .then((v) => {
        if (active) setAppVersion(v);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  const projectCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const memory of memories) {
      if (!memory.projectId) continue;
      counts.set(memory.projectId, (counts.get(memory.projectId) ?? 0) + 1);
    }
    return counts;
  }, [memories]);

  const searchShortcutLabel =
    shortcuts.find((shortcut) => shortcut.action === "open-search")?.accelerator ?? "Alt+Space";

  return (
    <aside className="sidebar">
      <div className="brand drag-region">
        <RecallMark size={18} />
        <span>Recall</span>
        <span
          className="no-drag"
          style={{ marginLeft: "auto", fontSize: 10, color: "var(--t-4)", fontWeight: 500 }}
        >
          {appVersion ? `v${appVersion}` : ""}
        </span>
      </div>

      <div className="nav-group">
        <div className="nav-label">Library</div>
        <NavRow
          icon={<LayoutGrid size={15} strokeWidth={1.6} />}
          label="Home"
          active={view === "dashboard"}
          onClick={() => setView("dashboard")}
        />
        <NavRow
          icon={<Layers size={15} strokeWidth={1.6} />}
          label="All Memories"
          count={memoryCount}
          active={view === "memories"}
          onClick={() => setView("memories")}
        />
        <NavRow
          icon={<FolderOpen size={15} strokeWidth={1.6} />}
          label="Projects"
          count={projectCount}
          active={view === "projects"}
          onClick={() => setView("projects")}
        />
        <NavRow
          icon={<Sparkles size={15} strokeWidth={1.6} />}
          label="Ask Recall"
          active={view === "ask"}
          onClick={() => setView("ask")}
        />
        <NavRow
          icon={<Search size={15} strokeWidth={1.6} />}
          label="Search"
          kbd={searchShortcutLabel}
          onClick={() => void tauriClient.openSearchOverlay()}
        />
        <NavRow
          icon={<Settings size={15} strokeWidth={1.6} />}
          label="Settings"
          active={view === "settings"}
          onClick={() => setView("settings")}
        />
      </div>

      {/* v0.5.15: RECENT CHATS — persistent Ask Recall conversation
          list. Always visible (no view-gated hiding) so users have
          peripheral awareness of recent chats from any surface,
          ChatGPT/Claude-style. Click a row to open the chat in
          AskView; hover reveals trash icon for delete. The "+ New
          chat" button at the top creates a fresh session and
          switches to AskView. */}
      <div className="nav-group">
        <div
          className="nav-label"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span>Recent chats</span>
          <button
            type="button"
            onClick={async () => {
              const id = await newChat();
              if (id) setView("ask");
            }}
            title="Start a new chat"
            style={{
              background: "none",
              border: "none",
              padding: 2,
              cursor: "pointer",
              color: "var(--t-3)",
              display: "flex",
              alignItems: "center",
            }}
          >
            <Plus size={12} strokeWidth={1.8} />
          </button>
        </div>
        {chatSessions.length === 0 ? (
          <div
            style={{
              fontSize: 11,
              color: "var(--t-4)",
              padding: "4px 10px 8px",
              fontStyle: "italic",
            }}
          >
            {chatHydrating ? "Loading…" : "No conversations yet."}
          </div>
        ) : (
          chatSessions.slice(0, 50).map((s) => (
            <ChatRow
              key={s.sessionId}
              title={chatDisplayTitle(s)}
              active={view === "ask" && activeSessionId === s.sessionId}
              onClick={async () => {
                await openChat(s.sessionId);
                setView("ask");
              }}
              onDelete={async () => {
                await deleteChat(s.sessionId);
              }}
            />
          ))
        )}
      </div>

      {pinned.length > 0 ? (
        <div className="nav-group">
          <div className="nav-label">Pinned Projects</div>
          {pinned.map((project, index) => (
            <NavRow
              key={project.id}
              icon={
                <span
                  style={{
                    width: 8,
                    height: 8,
                    borderRadius: 3,
                    background: projectDotForIndex(index),
                    display: "block",
                  }}
                />
              }
              label={project.name}
              count={projectCounts.get(project.id) ?? 0}
              onClick={() => {
                setView("projects");
              }}
            />
          ))}
        </div>
      ) : null}

      <SideFoot memoryCount={memoryCount} />
    </aside>
  );
}

function NavRow({
  icon,
  label,
  count,
  kbd,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  kbd?: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`nav-item${active ? " selected" : ""}`}
      onClick={onClick}
    >
      <span className="nav-icon-slot">{icon}</span>
      <span style={{ flex: "0 1 auto", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {label}
      </span>
      {count !== undefined ? (
        <span className="nav-count">{count.toLocaleString()}</span>
      ) : kbd ? (
        <span className="nav-count" style={{ fontFamily: "var(--font-mono)", fontSize: 10 }}>
          {kbd}
        </span>
      ) : null}
    </button>
  );
}

/**
 * v0.5.15: a single row in the RECENT CHATS sidebar list.
 * Hover reveals a trash icon on the right; click trash → tiny
 * inline confirm row replaces the title. No modal — keeps the
 * sidebar fluid. Click anywhere else on the row → opens the
 * chat. Active conversation gets the same `selected` highlight
 * as the LIBRARY rows.
 */
function ChatRow({
  title,
  active,
  onClick,
  onDelete,
}: {
  title: string;
  active: boolean;
  onClick: () => void | Promise<void>;
  onDelete: () => void | Promise<void>;
}) {
  const [hovered, setHovered] = useState(false);
  const [confirming, setConfirming] = useState(false);

  if (confirming) {
    return (
      <div
        className="nav-item"
        style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: 26 }}
      >
        <span style={{ fontSize: 11, color: "var(--t-3)" }}>Delete?</span>
        <button
          type="button"
          onClick={() => void onDelete()}
          style={{
            fontSize: 11,
            color: "var(--bad, #d33)",
            background: "none",
            border: "none",
            cursor: "pointer",
          }}
        >
          Yes
        </button>
        <button
          type="button"
          onClick={() => setConfirming(false)}
          style={{
            fontSize: 11,
            color: "var(--t-4)",
            background: "none",
            border: "none",
            cursor: "pointer",
          }}
        >
          Cancel
        </button>
      </div>
    );
  }

  return (
    <button
      type="button"
      className={`nav-item${active ? " selected" : ""}`}
      onClick={() => void onClick()}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{ paddingLeft: 26, position: "relative" }}
    >
      <span
        style={{
          flex: "1 1 auto",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          textAlign: "left",
        }}
      >
        {title || "Untitled chat"}
      </span>
      {hovered ? (
        <span
          role="button"
          tabIndex={0}
          onClick={(event) => {
            event.stopPropagation();
            setConfirming(true);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.stopPropagation();
              setConfirming(true);
            }
          }}
          title="Delete this conversation"
          style={{
            background: "none",
            border: "none",
            padding: 2,
            cursor: "pointer",
            color: "var(--t-4)",
            display: "flex",
            alignItems: "center",
          }}
        >
          <Trash2 size={11} strokeWidth={1.6} />
        </span>
      ) : null}
    </button>
  );
}

function SideFoot({ memoryCount }: { memoryCount: number }) {
  return (
    <div className="side-foot" title="Recall is fully offline. No accounts. No sync. Memories stay on this device.">
      <span className="dot-live" />
      <div className="user-meta">
        <div className="user-name">
          {memoryCount > 0
            ? `${memoryCount.toLocaleString()} memories`
            : "Local memory layer"}
        </div>
        <div className="user-status">Stored on this device</div>
      </div>
    </div>
  );
}

const projectDotForIndex = (index: number) => {
  const hues = [245, 30, 145, 305, 80, 200];
  return `oklch(0.7 0.08 ${hues[index % hues.length]})`;
};

/* ────────────────────────────────────────────────────────────────────────
   Update banner — restyled as a small floating chip in the upper right of
   the main column. Functionality (install / dismiss) preserved.
   ──────────────────────────────────────────────────────────────────────── */

function UpdateAvailableBanner({
  version,
  busy,
  onInstall,
  onDismiss,
}: {
  version: string;
  busy: boolean;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  return (
    <div
      style={{
        position: "absolute",
        top: 16,
        right: 16,
        zIndex: 20,
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "8px 8px 8px 14px",
        borderRadius: 10,
        background: "var(--panel-glass)",
        boxShadow: "0 0 0 0.5px var(--sh-window-edge), 0 12px 32px var(--sh-overlay)",
        backdropFilter: "blur(20px) saturate(160%)",
        WebkitBackdropFilter: "blur(20px) saturate(160%)",
      }}
    >
      <div style={{ fontSize: 12, color: "var(--t-1)", whiteSpace: "nowrap" }}>
        Recall {version} is available
      </div>
      <button
        type="button"
        className="btn btn-primary"
        onClick={onInstall}
        disabled={busy}
        style={{ height: 26, padding: "0 10px", fontSize: 12 }}
      >
        <Download size={11} strokeWidth={1.8} />
        {busy ? "Installing…" : "Install"}
      </button>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss update"
        className="detail-close"
        style={{ width: 22, height: 22 }}
      >
        <X size={11} strokeWidth={1.8} />
      </button>
    </div>
  );
}
