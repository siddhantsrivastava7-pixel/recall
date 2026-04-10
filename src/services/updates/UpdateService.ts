import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface AvailableUpdateInfo {
  version: string;
  releaseNotes: string | null;
  pubDate: string | null;
}

export interface UpdateDownloadProgress {
  downloaded: number;
  contentLength: number | null;
  progress: number;
}

type PendingUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;

let pendingUpdate: PendingUpdate | null = null;

const logUpdateEvent = (message: string, data?: unknown) => {
  const isDev = Boolean(
    (import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV,
  );
  if (isDev) {
    console.info(`[recall][updates] ${message}`, data ?? "");
  }
};

export class UpdateService {
  async getCurrentVersion() {
    return getVersion();
  }

  async checkForUpdates(): Promise<AvailableUpdateInfo | null> {
    logUpdateEvent("update check started");
    pendingUpdate = await check({ timeout: 30_000 });

    if (!pendingUpdate) {
      logUpdateEvent("no update available");
      return null;
    }

    const updateInfo = {
      version: pendingUpdate.version,
      releaseNotes: pendingUpdate.body ?? null,
      pubDate: pendingUpdate.date ?? null,
    };
    logUpdateEvent("update available", updateInfo);
    return updateInfo;
  }

  async downloadAndInstallUpdate(
    onProgress?: (progress: UpdateDownloadProgress) => void,
  ) {
    const update = pendingUpdate ?? (await check({ timeout: 30_000 }));
    if (!update) {
      throw new Error("No update is available.");
    }

    let downloaded = 0;
    let contentLength: number | null = null;

    logUpdateEvent("download started", { version: update.version });
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        downloaded = 0;
        contentLength = event.data.contentLength ?? null;
        onProgress?.({
          downloaded,
          contentLength,
          progress: 0,
        });
        return;
      }

      if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        const progress =
          contentLength && contentLength > 0
            ? Math.min(100, Math.round((downloaded / contentLength) * 100))
            : 0;
        onProgress?.({
          downloaded,
          contentLength,
          progress,
        });
        logUpdateEvent("download progress", { downloaded, contentLength, progress });
        return;
      }

      if (event.event === "Finished") {
        onProgress?.({
          downloaded,
          contentLength,
          progress: 100,
        });
        logUpdateEvent("download finished");
      }
    });

    logUpdateEvent("install completed");
    pendingUpdate = null;

    // On Windows the updater may exit the application as part of installation.
    // If control returns, relaunch so users land in the updated app.
    await relaunch();
  }
}

export const updateService = new UpdateService();
