import type { WindowLabel } from "@/domain/types";

export interface WindowAdapter {
  openMain(): Promise<void>;
  openSearchOverlay(): Promise<void>;
  openQuickSave(): Promise<void>;
  closeCurrent(): Promise<void>;
  getCurrentLabel(): Promise<WindowLabel>;
  setWidgetExpanded(expanded: boolean): Promise<void>;
}
