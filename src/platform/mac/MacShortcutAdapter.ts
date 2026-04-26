import type { ShortcutAdapter } from "@/platform/contracts/ShortcutAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class MacShortcutAdapter implements ShortcutAdapter {
  listBindings() {
    return tauriClient.listShortcuts();
  }
}
