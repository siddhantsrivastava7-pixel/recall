import type { RuntimePlatform } from "@/domain/types";
import type { PlatformAdapters } from "@/platform/contracts/PlatformAdapters";
import { MacAppContextAdapter } from "@/platform/mac/MacAppContextAdapter";
import { MacClipboardAdapter } from "@/platform/mac/MacClipboardAdapter";
import { MacFileSystemAdapter } from "@/platform/mac/MacFileSystemAdapter";
import { MacShortcutAdapter } from "@/platform/mac/MacShortcutAdapter";
import { MacWindowAdapter } from "@/platform/mac/MacWindowAdapter";
import { WindowsAppContextAdapter } from "@/platform/windows/WindowsAppContextAdapter";
import { WindowsClipboardAdapter } from "@/platform/windows/WindowsClipboardAdapter";
import { WindowsFileSystemAdapter } from "@/platform/windows/WindowsFileSystemAdapter";
import { WindowsShortcutAdapter } from "@/platform/windows/WindowsShortcutAdapter";
import { WindowsWindowAdapter } from "@/platform/windows/WindowsWindowAdapter";

export const createPlatformAdapters = (platform: RuntimePlatform): PlatformAdapters => {
  if (platform === "macos") {
    return {
      appContext: new MacAppContextAdapter(),
      clipboard: new MacClipboardAdapter(),
      fileSystem: new MacFileSystemAdapter(),
      shortcuts: new MacShortcutAdapter(),
      window: new MacWindowAdapter(),
    };
  }

  return {
    appContext: new WindowsAppContextAdapter(),
    clipboard: new WindowsClipboardAdapter(),
    fileSystem: new WindowsFileSystemAdapter(),
    shortcuts: new WindowsShortcutAdapter(),
    window: new WindowsWindowAdapter(),
  };
};
