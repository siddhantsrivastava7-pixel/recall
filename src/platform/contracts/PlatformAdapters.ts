import type { AppContextAdapter } from "@/platform/contracts/AppContextAdapter";
import type { ClipboardAdapter } from "@/platform/contracts/ClipboardAdapter";
import type { FileSystemAdapter } from "@/platform/contracts/FileSystemAdapter";
import type { ShortcutAdapter } from "@/platform/contracts/ShortcutAdapter";
import type { WindowAdapter } from "@/platform/contracts/WindowAdapter";

export interface PlatformAdapters {
  appContext: AppContextAdapter;
  clipboard: ClipboardAdapter;
  fileSystem: FileSystemAdapter;
  shortcuts: ShortcutAdapter;
  window: WindowAdapter;
}
