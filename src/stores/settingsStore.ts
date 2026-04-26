import { create } from "zustand";
import type { AppSettings, LicenseState, ShortcutBinding } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";

interface SettingsStoreState {
  settings: AppSettings;
  shortcuts: ShortcutBinding[];
  license: LicenseState | null;
  hydrate: (settings: AppSettings, shortcuts: ShortcutBinding[], license: LicenseState) => void;
  updateSettings: (settings: AppSettings) => Promise<{ ok: boolean; error?: string }>;
  updateShortcuts: (
    shortcuts: ShortcutBinding[],
  ) => Promise<{ ok: boolean; data?: ShortcutBinding[]; error?: string }>;
  activateLicense: (licenseKey: string) => Promise<{ ok: boolean; error?: string }>;
  deactivateLicense: () => Promise<{ ok: boolean; error?: string }>;
}

export const useSettingsStore = create<SettingsStoreState>((set) => ({
  settings: {
    floatingWidgetEnabled: true,
    launchOnStartupEnabled: false,
    updateAutoCheckEnabled: true,
    bookmarkAutoSyncEnabled: true,
    bookmarkSyncIntervalMinutes: 15,
    bookmarkSyncBrowsers: ["chrome", "edge", "brave"],
    bookmarkLastSyncedAt: null,
  },
  shortcuts: [],
  license: null,

  hydrate(settings, shortcuts, license) {
    set({ settings, shortcuts, license });
  },

  async updateSettings(settings) {
    try {
      const updated = await tauriClient.updateSettings(settings);
      set({ settings: updated });
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Failed to save settings." };
    }
  },

  async updateShortcuts(shortcuts) {
    try {
      const updated = await tauriClient.updateShortcuts(shortcuts);
      set({ shortcuts: updated });
      return { ok: true, data: updated };
    } catch (e) {
      return {
        ok: false,
        error: e instanceof Error ? e.message : "Failed to update shortcuts.",
      };
    }
  },

  async activateLicense(licenseKey) {
    try {
      const license = await tauriClient.activateLicense(licenseKey);
      set({ license });
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Activation failed." };
    }
  },

  async deactivateLicense() {
    try {
      const license = await tauriClient.deactivateLicense();
      set({ license });
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Deactivation failed." };
    }
  },
}));
