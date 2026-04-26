import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Memory } from "@/domain/types";
import { getSearchSuggestions } from "@/services/search/searchSuggestions";

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
  qualityScore: 0,
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
  createdAt: "2026-04-01T09:00:00.000Z",
  updatedAt: "2026-04-01T09:00:00.000Z",
  ...overrides,
});

describe("getSearchSuggestions", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-09T12:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns top topic-led suggestions using quality and recency as tie breakers", () => {
    const suggestions = getSearchSuggestions(
      [
        memory({
          id: "pricing-strategy",
          title: "Pricing strategy",
          content: "Pricing notes",
          topicLabels: ["Pricing", "Go To Market"],
          qualityScore: 88,
          updatedAt: "2026-04-09T10:00:00.000Z",
        }),
        memory({
          id: "old-pricing-note",
          title: "Old pricing note",
          content: "Pricing archive",
          topicLabels: ["Pricing"],
          qualityScore: 30,
          updatedAt: "2026-02-01T10:00:00.000Z",
        }),
        memory({
          id: "animation-note",
          title: "Animation note",
          content: "Motion",
          topicLabels: ["Motion"],
          qualityScore: 95,
          updatedAt: "2026-04-09T10:00:00.000Z",
        }),
      ],
      "that thing about pricing",
    );

    expect(suggestions.map((suggestion) => suggestion.memory.id)).toEqual([
      "pricing-strategy",
      "old-pricing-note",
    ]);
    expect(suggestions[0]?.reason).toContain("Pricing");
  });
});
