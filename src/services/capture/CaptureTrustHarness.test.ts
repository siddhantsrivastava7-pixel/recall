import { describe, expect, it, vi } from "vitest";

import type { BookmarkSyncSummary, BootstrapPayload, Memory } from "@/domain/types";
import { captureTrustCases } from "@/services/capture/captureTrustFixtures";
import {
  formatCaptureTrustSummary,
  runCaptureTrustSuite,
} from "@/services/capture/captureTrustRunner";
import {
  executeCaptureTrustCase,
  type CaptureTrustBackendAdapter,
} from "@/services/capture/captureTrustTestSupport";

const backendState = vi.hoisted(() => ({
  bootstrapPayload: null as BootstrapPayload | null,
  bookmarkSyncSummary: null as BookmarkSyncSummary | null,
  createQueue: [] as Memory[],
  createCallCount: 0,
}));

const tauriClientMock = vi.hoisted(() => ({
  bootstrap: vi.fn(async () => backendState.bootstrapPayload),
  listMemories: vi.fn(),
  getMemory: vi.fn(),
  createMemory: vi.fn(async () => {
    backendState.createCallCount += 1;
    const next = backendState.createQueue.shift();
    if (!next) {
      throw new Error("No mocked createMemory response available.");
    }
    return next;
  }),
  updateMemory: vi.fn(),
  deleteMemory: vi.fn(),
  duplicateMemory: vi.fn(),
  listProjects: vi.fn(),
  createProject: vi.fn(),
  updateProject: vi.fn(),
  deleteProject: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  listBookmarkSources: vi.fn(),
  importBookmarks: vi.fn(),
  syncBookmarksNow: vi.fn(async () => backendState.bookmarkSyncSummary),
  readClipboardText: vi.fn(),
  writeClipboardText: vi.fn(),
  detectAppContext: vi.fn(),
  getRuntimeInfo: vi.fn(),
  exportData: vi.fn(),
  importData: vi.fn(),
  clearAllData: vi.fn(),
  listShortcuts: vi.fn(),
  activateLicense: vi.fn(),
  deactivateLicense: vi.fn(),
  getLicenseState: vi.fn(),
  openMainWindow: vi.fn(),
  openSearchOverlay: vi.fn(),
  openQuickSaveWindow: vi.fn(),
  openMemoryInMain: vi.fn(),
  closeCurrentWindow: vi.fn(),
  setWidgetExpanded: vi.fn(),
  saveWidgetPosition: vi.fn(),
  seedSampleData: vi.fn(),
}));

vi.mock("@/services/api/tauri-client", () => ({
  tauriClient: tauriClientMock,
}));

const backendAdapter: CaptureTrustBackendAdapter = {
  reset() {
    backendState.createQueue = [];
    backendState.createCallCount = 0;
    backendState.bootstrapPayload = null;
    backendState.bookmarkSyncSummary = null;
    tauriClientMock.bootstrap.mockClear();
    tauriClientMock.createMemory.mockClear();
    tauriClientMock.syncBookmarksNow.mockClear();
  },
  enqueueCreateResponses(memories) {
    backendState.createQueue.push(...memories);
  },
  setBootstrapPayload(payload) {
    backendState.bootstrapPayload = payload;
  },
  setBookmarkSyncSummary(summary) {
    backendState.bookmarkSyncSummary = summary;
  },
  getCreateCallCount() {
    return backendState.createCallCount;
  },
};

describe("Capture trust harness", () => {
  it("prints a save-to-search reliability summary", async () => {
    const summary = await runCaptureTrustSuite(captureTrustCases, (testCase) =>
      executeCaptureTrustCase(testCase, backendAdapter),
    );

    console.log(formatCaptureTrustSummary(summary));
    expect(summary.failed).toBe(0);
    expect(summary.trustScore).toBe(100);
  });
});
