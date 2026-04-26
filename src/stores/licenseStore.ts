import { create } from "zustand";

import type { LicenseState } from "@/domain/types";
import { validateLicenseKey, LicenseValidationError } from "@/services/licenseService";
import { tauriClient } from "@/services/api/tauri-client";
import { useSettingsStore } from "@/stores/settingsStore";

type LicenseStatus =
  | "unchecked"
  | "empty"
  | "validating"
  | "success"
  | "invalid"
  | "expired"
  | "network-error"
  | "failed";

interface TrialLicenseRecord {
  key: string;
  activatedAt: string;
  expiresAt: string;
}

interface LicenseStoreState {
  checked: boolean;
  status: LicenseStatus;
  isLicensed: boolean;
  isTrial: boolean;
  isExpired: boolean;
  key: string | null;
  expiresAt: string | null;
  error: string | null;
  activateKey: (key: string) => Promise<{ ok: boolean; error?: string }>;
  checkLicense: (backendLicense?: LicenseState | null) => Promise<void>;
  clearLicense: () => Promise<void>;
}

const STORAGE_KEY = "recall.trialLicense";
const TRIAL_DAYS = 7;

const normalizeKey = (key: string) =>
  key.trim().replace(/\s+/g, "").toUpperCase();

const trialExpiresAt = (activatedAt: Date) =>
  new Date(activatedAt.getTime() + TRIAL_DAYS * 24 * 60 * 60 * 1000).toISOString();

const isPast = (iso: string | null | undefined) =>
  Boolean(iso && new Date(iso).getTime() <= Date.now());

const readStoredTrial = (): TrialLicenseRecord | null => {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;

    const parsed = JSON.parse(raw) as Partial<TrialLicenseRecord>;
    if (!parsed.key || !parsed.activatedAt || !parsed.expiresAt) {
      return null;
    }

    return {
      key: parsed.key,
      activatedAt: parsed.activatedAt,
      expiresAt: parsed.expiresAt,
    };
  } catch {
    return null;
  }
};

const writeStoredTrial = (record: TrialLicenseRecord) => {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(record));
};

const clearStoredTrial = () => {
  window.localStorage.removeItem(STORAGE_KEY);
};

const backendTrialRecord = (license?: LicenseState | null): TrialLicenseRecord | null => {
  if (!license?.isActivated || !license.isTrial || !license.licenseKey || !license.activatedAt || !license.expiresAt) {
    return null;
  }

  return {
    key: license.licenseKey,
    activatedAt: license.activatedAt,
    expiresAt: license.expiresAt,
  };
};

export const useLicenseStore = create<LicenseStoreState>((set) => ({
  checked: false,
  status: "unchecked",
  isLicensed: false,
  isTrial: false,
  isExpired: false,
  key: null,
  expiresAt: null,
  error: null,

  async activateKey(rawKey) {
    const key = normalizeKey(rawKey);
    if (!key) {
      const error = "Enter your trial key.";
      set({ status: "empty", error });
      return { ok: false, error };
    }

    set({
      status: "validating",
      error: null,
      isExpired: false,
    });

    try {
      const validation = await validateLicenseKey(key);
      if (!validation.valid) {
        const error = "Invalid key.";
        set({ status: "invalid", error, isLicensed: false });
        return { ok: false, error };
      }

      if (validation.expired) {
        const error = "Trial expired.";
        clearStoredTrial();
        set({
          status: "expired",
          error,
          isLicensed: false,
          isTrial: true,
          isExpired: true,
          key,
          expiresAt: null,
          checked: true,
        });
        return { ok: false, error };
      }

      const activatedAt = new Date();
      const record = {
        key,
        activatedAt: activatedAt.toISOString(),
        expiresAt: trialExpiresAt(activatedAt),
      };

      const license = await tauriClient.activateLicense(key);
      useSettingsStore.setState({ license });
      writeStoredTrial(record);

      set({
        checked: true,
        status: "success",
        isLicensed: true,
        isTrial: true,
        isExpired: false,
        key,
        expiresAt: record.expiresAt,
        error: null,
      });

      return { ok: true };
    } catch (error) {
      const message =
        error instanceof LicenseValidationError
          ? error.message
          : error instanceof Error
            ? error.message
            : "Activation failed. Please try again.";
      set({
        status: error instanceof LicenseValidationError ? "network-error" : "failed",
        error: message,
        isLicensed: false,
      });
      return { ok: false, error: message };
    }
  },

  async checkLicense(backendLicense) {
    const trial = readStoredTrial() ?? backendTrialRecord(backendLicense);
    const backendIsFullLicense =
      Boolean(backendLicense?.isActivated) && !backendLicense?.isTrial;

    if (trial) {
      if (isPast(trial.expiresAt)) {
        if (backendLicense?.isActivated) {
          const license = await tauriClient.deactivateLicense();
          useSettingsStore.setState({ license });
        }
        set({
          checked: true,
          status: "expired",
          isLicensed: false,
          isTrial: true,
          isExpired: true,
          key: trial.key,
          expiresAt: trial.expiresAt,
          error: "Trial expired.",
        });
        return;
      }

      writeStoredTrial(trial);
      set({
        checked: true,
        status: "success",
        isLicensed: true,
        isTrial: true,
        isExpired: false,
        key: trial.key,
        expiresAt: trial.expiresAt,
        error: null,
      });
      return;
    }

    if (backendIsFullLicense) {
      set({
        checked: true,
        status: "success",
        isLicensed: true,
        isTrial: false,
        isExpired: false,
        key: backendLicense?.licenseKey ?? null,
        expiresAt: null,
        error: null,
      });
      return;
    }

    set({
      checked: true,
      status: "empty",
      isLicensed: false,
      isTrial: false,
      isExpired: false,
      key: null,
      expiresAt: null,
      error: null,
    });
  },

  async clearLicense() {
    clearStoredTrial();
    const license = await tauriClient.deactivateLicense();
    useSettingsStore.setState({ license });
    set({
      checked: true,
      status: "empty",
      isLicensed: false,
      isTrial: false,
      isExpired: false,
      key: null,
      expiresAt: null,
      error: null,
    });
  },
}));
