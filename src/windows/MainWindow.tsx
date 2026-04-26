/**
 * MainWindow - "main" label
 *
 * Full desktop shell — depth + grain + 220px source-list sidebar.
 * Routes between: Dashboard, Memories, Projects, Settings.
 */

import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Download,
  FolderOpen,
  LayoutGrid,
  Layers,
  Search,
  Settings,
  X,
} from "lucide-react";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useUpdateStore } from "@/stores/updateStore";
import { Dashboard } from "@/components/dashboard/Dashboard";
import { MemoriesView } from "@/components/memory/MemoriesView";
import { ProjectsView } from "@/components/memory/ProjectsView";
import { SettingsView } from "@/components/settings/SettingsView";
import { InstantCaptureToast } from "@/components/capture/InstantCaptureToast";
import { RecallMark } from "@/components/system/RecallMark";
import { ThemeToggle } from "@/components/system/ThemeToggle";
import { tauriClient } from "@/services/api/tauri-client";
import { useRecallDataSyncEvents } from "@/hooks/useRecallDataSyncEvents";
import { useResurfaceNotifications } from "@/hooks/useResurfaceNotifications";

export type MainView = "dashboard" | "memories" | "projects" | "settings";

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
            {view === "dashboard" && <Dashboard setView={setView} />}
            {view === "memories" && <MemoriesView />}
            {view === "projects" && <ProjectsView setView={setView} />}
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
          v0.1.14
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

function SideFoot({ memoryCount }: { memoryCount: number }) {
  return (
    <div className="side-foot">
      <div className="avatar">SK</div>
      <div className="user-meta">
        <div className="user-name">You</div>
        <div className="user-status">
          <span className="dot-live" />
          {memoryCount > 0 ? `${memoryCount.toLocaleString()} indexed locally` : "Indexed locally"}
        </div>
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
