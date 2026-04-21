import type { PairingInfo, PairingQrPayload } from "@/domain/types";

export const parsePairingQrPayload = (payload: string): PairingQrPayload | null => {
  try {
    const parsed = JSON.parse(payload) as Partial<PairingQrPayload>;
    if (
      parsed.protocol !== "recall-local-pairing" ||
      parsed.version !== 1 ||
      typeof parsed.deviceId !== "string" ||
      typeof parsed.desktopName !== "string" ||
      typeof parsed.secret !== "string"
    ) {
      return null;
    }

    return {
      protocol: "recall-local-pairing",
      version: 1,
      deviceId: parsed.deviceId,
      desktopName: parsed.desktopName,
      endpoint: typeof parsed.endpoint === "string" ? parsed.endpoint : null,
      secret: parsed.secret,
    };
  } catch {
    return null;
  }
};

export const pairingInfoToQrPayload = (info: PairingInfo): PairingQrPayload => ({
  protocol: "recall-local-pairing",
  version: 1,
  deviceId: info.deviceId,
  desktopName: info.desktopName,
  endpoint: info.endpoint,
  secret: info.pairingSecret,
});
