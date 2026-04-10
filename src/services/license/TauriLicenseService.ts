import type { LicenseService } from "@/services/license/LicenseService";
import { tauriClient } from "@/services/api/tauri-client";

export class TauriLicenseService implements LicenseService {
  getState() {
    return tauriClient.getLicenseState();
  }

  async activate(licenseKey: string) {
    try {
      const state = await tauriClient.activateLicense(licenseKey);
      return { ok: true, data: state };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : "Activation failed.",
      };
    }
  }

  async deactivate() {
    try {
      const state = await tauriClient.deactivateLicense();
      return { ok: true, data: state };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : "Deactivation failed.",
      };
    }
  }
}
