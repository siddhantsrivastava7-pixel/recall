import type { AppSettings, ServiceResult } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";

export const updateSettings = async (settings: AppSettings) => {
  try {
    const updated = await tauriClient.updateSettings(settings);
    return { ok: true, data: updated } satisfies ServiceResult<AppSettings>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to update settings.",
    } satisfies ServiceResult<AppSettings>;
  }
};
