import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Memory } from "@/domain/types";
import { applyCapturedMemoryToStores } from "@/services/capture/captureSync";
import { useMemoryStore } from "@/stores/memoryStore";
import { useProjectStore } from "@/stores/projectStore";
import { useSearchStore } from "@/stores/searchStore";

const savedMemory: Memory = {
  id: "memory-new",
  sourceType: "manual",
  title: "Investor pricing deck",
  content: "Deck outline and pricing narrative for board review.",
  note: null,
  projectId: "project-pricing",
  projectName: "Pricing",
  url: null,
  externalId: null,
  folderPath: null,
  sourceApp: "Chrome",
  sourceWindow: "Slides",
  createdAt: "2026-04-09T11:59:00.000Z",
  updatedAt: "2026-04-09T11:59:00.000Z",
};

describe("captureSync", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-09T12:00:00.000Z"));

    useMemoryStore.setState({
      memories: [],
      filters: { projectId: "all", sortOrder: "newest", text: "" },
      selectedMemoryId: null,
      operationMessage: null,
    });
    useProjectStore.setState({
      projects: [
        {
          id: "project-pricing",
          name: "Pricing",
          description: null,
          createdAt: "2026-04-01T09:00:00.000Z",
          updatedAt: "2026-04-01T09:00:00.000Z",
        },
      ],
      activeProjectId: "all",
    });
    useSearchStore.setState({
      query: "",
      results: [],
      selectedIndex: 0,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("makes a newly saved memory immediately visible to memory lists and search", () => {
    useSearchStore.getState().setQuery("investor pricing deck");
    expect(useSearchStore.getState().results).toHaveLength(0);

    applyCapturedMemoryToStores(savedMemory);

    expect(useMemoryStore.getState().memories[0]?.id).toBe("memory-new");
    expect(useSearchStore.getState().results[0]?.memory.id).toBe("memory-new");
  });
});
