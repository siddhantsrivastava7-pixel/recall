import { create } from "zustand";

import { updateService } from "@/services/updates/UpdateService";

type UpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "restart-needed"
  | "failed";

interface UpdateStoreState {
  currentVersion: string | null;
  status: UpdateStatus;
  checking: boolean;
  updateAvailable: boolean;
  availableVersion: string | null;
  releaseNotes: string | null;
  pubDate: string | null;
  downloading: boolean;
  downloadProgress: number;
  installing: boolean;
  lastCheckedAt: string | null;
  error: string | null;
  hydrateCurrentVersion: () => Promise<void>;
  checkForUpdates: (options?: { silent?: boolean }) => Promise<void>;
  downloadAndInstallUpdate: () => Promise<void>;
  maybeCheckOnStartup: (enabled: boolean) => Promise<void>;
  resetError: () => void;
}

const errorMessage = (error: unknown) =>
  error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "Update failed. Please try again.";

export const useUpdateStore = create<UpdateStoreState>((set, get) => ({
  currentVersion: null,
  status: "idle",
  checking: false,
  updateAvailable: false,
  availableVersion: null,
  releaseNotes: null,
  pubDate: null,
  downloading: false,
  downloadProgress: 0,
  installing: false,
  lastCheckedAt: null,
  error: null,

  async hydrateCurrentVersion() {
    if (get().currentVersion) return;
    try {
      const currentVersion = await updateService.getCurrentVersion();
      set({ currentVersion });
    } catch {
      set({ currentVersion: null });
    }
  },

  async checkForUpdates(options) {
    if (get().checking || get().downloading || get().installing) return;

    set({
      status: "checking",
      checking: true,
      error: null,
    });

    try {
      await get().hydrateCurrentVersion();
      const update = await updateService.checkForUpdates();
      const checkedAt = new Date().toISOString();

      if (!update) {
        set({
          status: "up-to-date",
          checking: false,
          updateAvailable: false,
          availableVersion: null,
          releaseNotes: null,
          pubDate: null,
          lastCheckedAt: checkedAt,
          error: null,
        });
        return;
      }

      set({
        status: "available",
        checking: false,
        updateAvailable: true,
        availableVersion: update.version,
        releaseNotes: update.releaseNotes,
        pubDate: update.pubDate,
        lastCheckedAt: checkedAt,
        error: null,
      });
    } catch (error) {
      const checkedAt = new Date().toISOString();
      set({
        status: options?.silent ? "idle" : "failed",
        checking: false,
        lastCheckedAt: checkedAt,
        error: options?.silent ? null : errorMessage(error),
      });
    }
  },

  async downloadAndInstallUpdate() {
    if (get().downloading || get().installing) return;

    set({
      status: "downloading",
      downloading: true,
      downloadProgress: 0,
      installing: false,
      error: null,
    });

    try {
      await updateService.downloadAndInstallUpdate((progress) => {
        set({
          downloadProgress: progress.progress,
          status: progress.progress >= 100 ? "installing" : "downloading",
          installing: progress.progress >= 100,
        });
      });

      set({
        status: "restart-needed",
        downloading: false,
        installing: false,
        downloadProgress: 100,
      });
    } catch (error) {
      set({
        status: "failed",
        downloading: false,
        installing: false,
        error: errorMessage(error),
      });
    }
  },

  async maybeCheckOnStartup(enabled) {
    if (!enabled || get().lastCheckedAt || get().checking) return;
    await get().hydrateCurrentVersion();
    await get().checkForUpdates({ silent: true });
  },

  resetError() {
    set({ error: null });
  },
}));
