/**
 * MainWindow - "main" label
 *
 * Full desktop shell:
 * 64px icon sidebar + content area.
 * Routes between: Dashboard, Memories, Projects, Settings.
 */

import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { LayoutGrid, FileText, FolderOpen, Search, Settings, Download, X } from "lucide-react";
import { useAppStore } from "@/stores/appStore";
import { useMemoryStore } from "@/stores/memoryStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useUpdateStore } from "@/stores/updateStore";
import { Dashboard } from "@/components/dashboard/Dashboard";
import { MemoriesView } from "@/components/memory/MemoriesView";
import { ProjectsView } from "@/components/memory/ProjectsView";
import { SettingsView } from "@/components/settings/SettingsView";
import { InstantCaptureToast } from "@/components/capture/InstantCaptureToast";
import { tauriClient } from "@/services/api/tauri-client";
import { useRecallDataSyncEvents } from "@/hooks/useRecallDataSyncEvents";

export type MainView = "dashboard" | "memories" | "projects" | "settings";

export function MainWindow() {
  const [view, setView] = useState<MainView>("dashboard");
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const runtime = useAppStore((s) => s.runtime);
  const selectMemory = useMemoryStore((state) => state.selectMemory);
  const settings = useSettingsStore((state) => state.settings);
  const updateAvailable = useUpdateStore((state) => state.updateAvailable);
  const availableVersion = useUpdateStore((state) => state.availableVersion);
  const downloading = useUpdateStore((state) => state.downloading);
  const installing = useUpdateStore((state) => state.installing);
  const maybeCheckOnStartup = useUpdateStore((state) => state.maybeCheckOnStartup);
  const downloadAndInstallUpdate = useUpdateStore((state) => state.downloadAndInstallUpdate);
  useRecallDataSyncEvents();

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
    <div
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        overflow: "hidden",
        background: "linear-gradient(135deg, #0B0F1A 0%, #0E1424 60%, #0B1020 100%)",
        position: "relative",
      }}
    >
      <div className="recall-noise" />

      <div
        style={{
          position: "fixed",
          width: 600,
          height: 600,
          borderRadius: "50%",
          background: "radial-gradient(circle, rgba(79,124,255,0.08) 0%, transparent 70%)",
          top: -120,
          right: 80,
          pointerEvents: "none",
          zIndex: 0,
        }}
      />
      <div
        style={{
          position: "fixed",
          width: 400,
          height: 400,
          borderRadius: "50%",
          background: "radial-gradient(circle, rgba(79,124,255,0.05) 0%, transparent 70%)",
          bottom: 60,
          left: 80,
          pointerEvents: "none",
          zIndex: 0,
        }}
      />

      <Sidebar view={view} setView={setView} />

      <main
        style={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          position: "relative",
          zIndex: 1,
        }}
      >
        {showUpdateBanner && (
          <UpdateAvailableBanner
            version={availableVersion}
            busy={downloading || installing}
            onInstall={() => void downloadAndInstallUpdate()}
            onDismiss={() => setDismissedUpdateVersion(availableVersion)}
          />
        )}
        {view === "dashboard" && <Dashboard setView={setView} />}
        {view === "memories" && <MemoriesView />}
        {view === "projects" && <ProjectsView setView={setView} />}
        {view === "settings" && <SettingsView />}
        <InstantCaptureToast />
      </main>
    </div>
  );
}

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
        top: 22,
        right: 26,
        zIndex: 20,
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 12px 10px 14px",
        borderRadius: 999,
        background: "rgba(17,24,39,0.82)",
        border: "1px solid rgba(255,255,255,0.10)",
        boxShadow: "0 18px 50px rgba(0,0,0,0.28)",
        backdropFilter: "blur(18px)",
      }}
    >
      <div style={{ fontSize: 13, color: "var(--text-primary)", whiteSpace: "nowrap" }}>
        Recall {version} is available
      </div>
      <button className="btn-primary" onClick={onInstall} disabled={busy} style={{ padding: "7px 10px" }}>
        <Download size={12} />
        {busy ? "Installing" : "Install"}
      </button>
      <button
        onClick={onDismiss}
        aria-label="Dismiss update"
        style={{
          width: 26,
          height: 26,
          borderRadius: "50%",
          border: "1px solid rgba(255,255,255,0.08)",
          background: "rgba(255,255,255,0.05)",
          color: "rgba(255,255,255,0.5)",
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <X size={13} />
      </button>
    </div>
  );
}

function Sidebar({ view, setView }: { view: MainView; setView: (v: MainView) => void }) {
  const shortcuts = useSettingsStore((state) => state.shortcuts);
  const searchShortcutLabel =
    shortcuts.find((shortcut) => shortcut.action === "open-search")?.accelerator ?? "Alt+Space";

  return (
    <nav className="sidebar" style={{ position: "relative", zIndex: 2 }}>
      <div
        style={{
          width: 30,
          height: 30,
          borderRadius: "50%",
          background: "rgba(79,124,255,0.18)",
          border: "1.5px solid rgba(79,124,255,0.4)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          marginBottom: 14,
        }}
      >
        <div style={{ width: 9, height: 9, borderRadius: "50%", background: "#4F7CFF" }} />
      </div>

      <NavIcon
        icon={<LayoutGrid size={18} strokeWidth={1.7} />}
        label="Dashboard"
        active={view === "dashboard"}
        onClick={() => setView("dashboard")}
      />
      <NavIcon
        icon={<FileText size={18} strokeWidth={1.7} />}
        label="Memories"
        active={view === "memories"}
        onClick={() => setView("memories")}
      />
      <NavIcon
        icon={<FolderOpen size={18} strokeWidth={1.7} />}
        label="Projects"
        active={view === "projects"}
        onClick={() => setView("projects")}
      />
      <NavIcon
        icon={<Search size={18} strokeWidth={1.7} />}
        label={`Search ${searchShortcutLabel}`}
        active={false}
        onClick={() => tauriClient.openSearchOverlay()}
      />

      <div style={{ flex: 1 }} />

      <NavIcon
        icon={<Settings size={18} strokeWidth={1.7} />}
        label="Settings"
        active={view === "settings"}
        onClick={() => setView("settings")}
      />
    </nav>
  );
}

function NavIcon({
  icon,
  label,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button className={`nav-icon ${active ? "active" : ""}`} title={label} onClick={onClick}>
      {icon}
    </button>
  );
}
