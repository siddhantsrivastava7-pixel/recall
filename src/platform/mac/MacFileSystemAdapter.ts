import type { FileSystemAdapter } from "@/platform/contracts/FileSystemAdapter";
import { tauriClient } from "@/services/api/tauri-client";

export class MacFileSystemAdapter implements FileSystemAdapter {
  exportData() {
    return tauriClient.exportData();
  }

  importData() {
    return tauriClient.importData();
  }
}
