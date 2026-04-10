import { tauriClient } from "@/services/api/tauri-client";

export interface LicenseValidationResult {
  valid: boolean;
  expired: boolean;
}

export type LicenseValidationErrorCode =
  | "network"
  | "timeout"
  | "invalid-response";

export class LicenseValidationError extends Error {
  constructor(
    public readonly code: LicenseValidationErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "LicenseValidationError";
  }
}

export async function validateLicenseKey(
  key: string,
  _options: { timeoutMs?: number } = {},
): Promise<LicenseValidationResult> {
  try {
    const data = await tauriClient.validateLicenseKey(key);
    if (typeof data.valid !== "boolean" || typeof data.expired !== "boolean") {
      throw new LicenseValidationError(
        "invalid-response",
        "The license server returned an unexpected response.",
      );
    }

    return {
      valid: data.valid,
      expired: data.expired,
    };
  } catch (error) {
    if (error instanceof LicenseValidationError) {
      throw error;
    }

    const message = error instanceof Error ? error.message : String(error);
    if (message.toLowerCase().includes("timeout")) {
      throw new LicenseValidationError(
        "timeout",
        "License validation timed out. Please try again.",
      );
    }

    throw new LicenseValidationError(
      "network",
      "Unable to reach the license server. Check your connection and try again.",
    );
  }
}
