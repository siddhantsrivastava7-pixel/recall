import type { ShortcutAdapter } from "@/platform/contracts/ShortcutAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class WindowsShortcutAdapter implements ShortcutAdapter {
  listBindings() {
    return tauriClient.listShortcuts();
  }
}
