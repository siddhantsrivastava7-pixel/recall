import type { ClipboardAdapter } from "@/platform/contracts/ClipboardAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class MacClipboardAdapter implements ClipboardAdapter {
  readText() {
    return tauriClient.readClipboardText();
  }

  writeText(text: string) {
    return tauriClient.writeClipboardText(text);
  }
}
