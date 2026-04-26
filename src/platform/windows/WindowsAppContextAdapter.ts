import type { AppContextAdapter } from "@/platform/contracts/AppContextAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class WindowsAppContextAdapter implements AppContextAdapter {
  getRuntimeInfo() {
    return tauriClient.getRuntimeInfo();
  }

  detectCurrentContext() {
    return tauriClient.detectAppContext();
  }
}
