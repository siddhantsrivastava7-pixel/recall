import { describe, expect, it } from "vitest";

import { getMemoryDisplayTitle } from "@/domain/formatters";
import type { Memory } from "@/domain/types";

const baseMemory: Memory = {
  id: "memory-1",
  sourceType: "manual",
  title: null,
  content: "https://github.com/tauri-apps/tauri",
  note: null,
  projectId: null,
  projectName: null,
  url: "https://github.com/tauri-apps/tauri",
  domain: "github.com",
  resolvedTitle: null,
  resolvedDescription: null,
  resolvedImage: null,
  resolvedSiteName: null,
  enrichmentStatus: "pending",
  enrichedAt: null,
  externalId: null,
  folderPath: null,
  sourceApp: "Chrome",
  sourceWindow: "GitHub",
  createdAt: "2026-04-09T12:00:00.000Z",
  updatedAt: "2026-04-09T12:00:00.000Z",
};

describe("getMemoryDisplayTitle", () => {
  it("prefers resolved title over a synthetic raw-url title", () => {
    const result = getMemoryDisplayTitle({
      ...baseMemory,
      title: "https://github.com/tauri-apps/tauri",
      resolvedTitle: "tauri-apps/tauri: Build smaller, faster, and more secure desktop apps",
    });

    expect(result).toBe(
      "tauri-apps/tauri: Build smaller, faster, and more secure desktop apps",
    );
  });

  it("keeps a real user-written title ahead of resolved metadata", () => {
    const result = getMemoryDisplayTitle({
      ...baseMemory,
      title: "Tauri repo for later reading",
      resolvedTitle: "tauri-apps/tauri: Build smaller, faster, and more secure desktop apps",
    });

    expect(result).toBe("Tauri repo for later reading");
  });
});
