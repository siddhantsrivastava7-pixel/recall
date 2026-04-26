import { tauriClient } from "@/services/api/tauri-client";
import type { PairingInfo } from "@/domain/types";
import { parsePairingQrPayload, pairingInfoToQrPayload } from "@/features/pairing/qrPayload";

export interface PairingService {
  getPairingInfo(): Promise<PairingInfo>;
  resetPairing(): Promise<PairingInfo>;
}

export const desktopPairingService: PairingService = {
  async getPairingInfo() {
    const info = await tauriClient.getPairingInfo();
    return normalizePairingInfo(info);
  },

  async resetPairing() {
    const info = await tauriClient.resetPairing();
    return normalizePairingInfo(info);
  },
};

const normalizePairingInfo = (info: PairingInfo): PairingInfo => {
  const parsed = parsePairingQrPayload(info.qrPayload);
  const qrPayload = parsed ?? pairingInfoToQrPayload(info);
  return {
    ...info,
    endpoint: info.endpoint ?? null,
    port: info.port ?? null,
    qrPayload: JSON.stringify(qrPayload),
  };
};
