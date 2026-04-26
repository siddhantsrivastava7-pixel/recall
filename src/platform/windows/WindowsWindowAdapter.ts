import { getCurrentWindow } from "@tauri-apps/api/window";
import type { WindowLabel } from "@/domain/types";
import type { WindowAdapter } from "@/platform/contracts/WindowAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class WindowsWindowAdapter implements WindowAdapter {
  openMain() {
    return tauriClient.openMainWindow();
  }

  openSearchOverlay() {
    return tauriClient.openSearchOverlay();
  }

  openQuickSave() {
    return tauriClient.openQuickSaveWindow();
  }

  closeCurrent() {
    return tauriClient.closeCurrentWindow();
  }

  setWidgetExpanded(expanded: boolean) {
    return tauriClient.setWidgetExpanded(expanded);
  }

  async getCurrentLabel(): Promise<WindowLabel> {
    return getCurrentWindow().label as WindowLabel;
  }
}
