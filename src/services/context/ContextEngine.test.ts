import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Memory, Project } from "@/domain/types";
import {
  buildSessionContext,
  getProjectRelevantMemories,
  getRecallFeed,
  getRelatedMemories,
} from "@/services/context/ContextEngine";

const memory = ({
  id,
  content,
  ...overrides
}: Partial<Memory> & Pick<Memory, "id" | "content">): Memory => ({
  id,
  sourceType: "manual",
  title: null,
  content,
  note: null,
  projectId: null,
  projectName: null,
  url: null,
  domain: null,
  resolvedDomain: null,
  canonicalUrl: null,
  resolvedTitle: null,
  resolvedDescription: null,
  resolvedImage: null,
  resolvedSiteName: null,
  previewText: null,
  memoryType: null,
  topicLabels: null,
  primaryTopic: null,
  qualityScore: 40,
  bookmarkQualityScore: 0,
  isDuplicateOf: null,
  bookmarkFolderPath: null,
  enrichmentStatus: "done",
  enrichmentError: null,
  enrichedAt: null,
  lastEnrichedAt: null,
  externalId: null,
  folderPath: null,
  sourceApp: null,
  sourceWindow: null,
  lastOpenedAt: null,
  openCount: 0,
  createdAt: "2026-04-01T09:00:00.000Z",
  updatedAt: "2026-04-01T09:00:00.000Z",
  ...overrides,
});

const project = (id: string, name: string): Project => ({
  id,
  name,
  description: null,
  createdAt: "2026-04-01T09:00:00.000Z",
  updatedAt: "2026-04-01T09:00:00.000Z",
});

describe("ContextEngine", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-10T12:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses session topics to find related memories", () => {
    const memories = [
      memory({
        id: "pricing",
        title: "Pricing narrative",
        content: "Board pricing notes",
        topicLabels: ["Pricing", "Board"],
        qualityScore: 80,
      }),
      memory({
        id: "design",
        title: "Animation timing",
        content: "Motion notes",
        topicLabels: ["Motion"],
        qualityScore: 90,
      }),
    ];
    const context = buildSessionContext(memories, {
      recentQueries: ["pricing board"],
      recentlyOpenedMemoryIds: [],
      recentCaptureIds: [],
      activeProjectId: "all",
    });
    const feed = getRecallFeed(memories, [], context);

    expect(feed.youMightAlsoNeed[0]?.memory.id).toBe("pricing");
  });

  it("finds related memories from topic and domain overlap", () => {
    const current = memory({
      id: "github-docs",
      title: "GitHub Actions docs",
      content: "https://github.com/features/actions",
      domain: "github.com",
      topicLabels: ["CI", "GitHub"],
      qualityScore: 75,
    });
    const related = memory({
      id: "ci-notes",
      title: "CI release checklist",
      content: "Pipeline notes",
      topicLabels: ["CI"],
      qualityScore: 70,
    });
    const context = buildSessionContext([current, related], {
      recentQueries: [],
      recentlyOpenedMemoryIds: [],
      recentCaptureIds: [],
      activeProjectId: "all",
    });

    expect(getRelatedMemories(current, [current, related], context)[0]?.memory.id).toBe("ci-notes");
  });

  it("suggests memories relevant to an active project", () => {
    const projects = [project("project-pricing", "Pricing")];
    const memories = [
      memory({
        id: "pricing-bookmark",
        sourceType: "bookmark",
        title: "Pricing research",
        content: "https://example.com/pricing",
        topicLabels: ["Pricing"],
        qualityScore: 78,
      }),
    ];

    expect(
      getProjectRelevantMemories(memories, projects, "project-pricing")[0]?.memory.id,
    ).toBe("pricing-bookmark");
  });
});
