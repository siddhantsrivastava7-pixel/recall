import { create } from "zustand";
import type { RuntimeInfo } from "@/domain/types";
import { configureRuntimePlatform } from "@/app-runtime";
import { tauriClient } from "@/services/api/tauri-client";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSettingsStore } from "@/stores/settingsStore";

const BOOTSTRAP_RETRY_DELAYS_MS = [120, 240, 480, 800, 1200];

const sleep = (ms: number) =>
  new Promise<void>((resolve) => {
    window.setTimeout(resolve, ms);
  });

const isTransientBootstrapError = (error: unknown) => {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : JSON.stringify(error);

  const normalized = message.toLowerCase();
  return (
    normalized.includes("state not managed") ||
    normalized.includes("must call `.manage()` before using this command")
  );
};

const fetchBootstrapPayload = async () => {
  let lastError: unknown;

  for (let attempt = 0; attempt <= BOOTSTRAP_RETRY_DELAYS_MS.length; attempt += 1) {
    try {
      return await tauriClient.bootstrap();
    } catch (error) {
      lastError = error;
      if (
        attempt === BOOTSTRAP_RETRY_DELAYS_MS.length ||
        !isTransientBootstrapError(error)
      ) {
        throw error;
      }
      await sleep(BOOTSTRAP_RETRY_DELAYS_MS[attempt]);
    }
  }

  throw lastError ?? new Error("Unable to start Recall.");
};

interface AppStoreState {
  isBootstrapping: boolean;
  initialized: boolean;
  runtime: RuntimeInfo | null;
  error: string | null;
  bootstrap: () => Promise<void>;
  hydrateFromImport: () => Promise<void>;
}

export const useAppStore = create<AppStoreState>((set) => ({
  isBootstrapping: true,
  initialized: false,
  runtime: null,
  error: null,

  async bootstrap() {
    set({ isBootstrapping: true, error: null });
    try {
      const payload = await fetchBootstrapPayload();
      configureRuntimePlatform(payload.runtime.platform);
      useMemoryStore.getState().hydrate(payload.memories);
      useProjectStore.getState().hydrate(payload.projects);
      useSettingsStore.getState().hydrate(payload.settings, payload.shortcuts, payload.license);
      set({
        runtime: payload.runtime,
        initialized: true,
        isBootstrapping: false,
        error: null,
      });
    } catch (error) {
      const msg =
        error instanceof Error
          ? error.message
          : typeof error === "string"
          ? error
          : JSON.stringify(error);
      set({
        error: msg || "Unable to start Recall.",
        isBootstrapping: false,
      });
    }
  },

  async hydrateFromImport() {
    const payload = await fetchBootstrapPayload();
    configureRuntimePlatform(payload.runtime.platform);
    useMemoryStore.getState().hydrate(payload.memories);
    useProjectStore.getState().hydrate(payload.projects);
    useSettingsStore.getState().hydrate(payload.settings, payload.shortcuts, payload.license);
    set({ runtime: payload.runtime });
  },
}));
