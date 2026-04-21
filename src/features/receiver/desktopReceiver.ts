import type { PairingInfo } from "@/domain/types";
import { desktopPairingService } from "@/features/pairing/pairingService";

export interface DesktopReceiverStatus {
  running: boolean;
  endpoint: string | null;
  pairingStatus: string;
}

export const getDesktopReceiverStatus = async (): Promise<DesktopReceiverStatus> => {
  const info = await desktopPairingService.getPairingInfo();
  return pairingInfoToReceiverStatus(info);
};

export const pairingInfoToReceiverStatus = (info: PairingInfo): DesktopReceiverStatus => ({
  running: info.receiverRunning,
  endpoint: info.endpoint,
  pairingStatus: info.pairingStatus,
});
