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

// v0.3.1: source-of-truth shifts to the backend's `license_state`
// SQLite row exclusively. The previous localStorage `recall.trialLicense`
// cache caused two bugs:
//
//   1. activateKey wrote a 7-day-expiry trial record for *every*
//      successful activation, including non-trial keys, so even paid
//      licenses appeared "expired" 7 days after first activation.
//   2. checkLicense prioritized localStorage over the backend, so a
//      wiped WebView storage (which can happen across MSI upgrades)
//      would force re-activation even though SQLite still had the
//      license intact.
//
// We now mirror the backend's `LicenseState` directly. `is_trial` and
// `expires_at` come from the row the Rust side wrote at activation time.

const normalizeKey = (key: string) =>
  key.trim().replace(/\s+/g, "").toUpperCase();

const isPast = (iso: string | null | undefined) =>
  Boolean(iso && new Date(iso).getTime() <= Date.now());

// One-time sweep of the legacy `recall.trialLicense` localStorage entry
// from pre-0.3.1 builds. The previous flow stored a 7-day expiry record
// here for *every* successful activation (including non-trial keys),
// then prioritized it over the backend on subsequent launches —
// causing perpetual "license expired" prompts after the first 7 days.
// Removing it on first run after the upgrade flushes the bad cache.
try {
  if (typeof window !== "undefined" && window.localStorage) {
    window.localStorage.removeItem("recall.trialLicense");
  }
} catch {
  // No storage available; nothing to clean up anyway.
}

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

      const license = await tauriClient.activateLicense(key);
      useSettingsStore.setState({ license });

      // Mirror the backend's row verbatim — is_trial and expires_at
      // are decided server-side (well, Rust-side via LicenseService),
      // so we don't second-guess them with our own 7-day default here.
      set({
        checked: true,
        status: "success",
        isLicensed: license.isActivated,
        isTrial: license.isTrial,
        isExpired: false,
        key: license.licenseKey,
        expiresAt: license.expiresAt,
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
    // Backend (`license_state` row in SQLite) is the source of truth.
    // No localStorage layer to fight with. If the row says activated,
    // the user is activated.
    if (!backendLicense?.isActivated) {
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
      return;
    }

    // Trial rows carry an `expires_at`; check it. If past, deactivate
    // and force re-activation. The Rust-side `get_state` does this
    // same check on read, so we're belt-and-braces here — frontend
    // stays consistent even if a stale cached row leaked through.
    if (backendLicense.isTrial && isPast(backendLicense.expiresAt)) {
      const license = await tauriClient.deactivateLicense();
      useSettingsStore.setState({ license });
      set({
        checked: true,
        status: "expired",
        isLicensed: false,
        isTrial: true,
        isExpired: true,
        key: backendLicense.licenseKey,
        expiresAt: backendLicense.expiresAt,
        error: "Trial expired.",
      });
      return;
    }

    set({
      checked: true,
      status: "success",
      isLicensed: true,
      isTrial: backendLicense.isTrial,
      isExpired: false,
      key: backendLicense.licenseKey,
      expiresAt: backendLicense.expiresAt,
      error: null,
    });
  },

  async clearLicense() {
    const license = await tauriClient.deactivateLicense();
    useSettingsStore.setState({ license });
    // Best-effort sweep of any legacy localStorage trial record from
    // pre-0.3.1 builds. If it's not there, removeItem is a no-op.
    try {
      window.localStorage.removeItem("recall.trialLicense");
    } catch {
      // Storage isn't available in some webview contexts; harmless.
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
}));
