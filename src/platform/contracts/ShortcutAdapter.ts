import type { ShortcutBinding } from "@/domain/types";

export interface ShortcutAdapter {
  listBindings(): Promise<ShortcutBinding[]>;
}
