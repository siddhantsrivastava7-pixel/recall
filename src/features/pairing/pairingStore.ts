import { create } from "zustand";

import type { PairingInfo } from "@/domain/types";
import { desktopPairingService } from "@/features/pairing/pairingService";

interface PairingStoreState {
  info: PairingInfo | null;
  loading: boolean;
  error: string | null;
  hydrate: () => Promise<void>;
  reset: () => Promise<void>;
  applyPairingInfo: (info: PairingInfo) => void;
}

export const usePairingStore = create<PairingStoreState>((set) => ({
  info: null,
  loading: false,
  error: null,

  async hydrate() {
    set({ loading: true, error: null });
    try {
      const info = await desktopPairingService.getPairingInfo();
      set({ info, loading: false });
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : "Unable to load pairing info.",
      });
    }
  },

  async reset() {
    set({ loading: true, error: null });
    try {
      const info = await desktopPairingService.resetPairing();
      set({ info, loading: false });
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : "Unable to reset pairing.",
      });
    }
  },

  applyPairingInfo(info) {
    set({ info, error: null });
  },
}));
