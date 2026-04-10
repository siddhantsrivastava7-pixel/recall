import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { KeywordSearchProvider } from "@/services/search/KeywordSearchProvider";
import { searchEvaluationCases } from "@/services/search/searchRankingFixtures";
import { runSearchEvaluationCase } from "@/services/search/searchEvaluationRunner";

describe("KeywordSearchProvider ranking", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-09T12:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each(searchEvaluationCases)("$id", (testCase) => {
    const result = runSearchEvaluationCase(testCase);
    expect(result.failures).toEqual([]);
  });

  it("prefers enriched high-quality bookmarks over duplicate bookmark rows", () => {
    const provider = new KeywordSearchProvider();
    const results = provider.search({
      memories: [
        {
          id: "bookmark-primary",
          sourceType: "bookmark",
          title: "https://platform.openai.com/docs/pricing",
          content: "https://platform.openai.com/docs/pricing",
          note: null,
          projectId: null,
          projectName: null,
          url: "https://platform.openai.com/docs/pricing",
          domain: "platform.openai.com",
          resolvedDomain: "platform.openai.com",
          canonicalUrl: "https://platform.openai.com/docs/pricing",
          resolvedTitle: "OpenAI pricing guide",
          resolvedDescription: "Review model pricing and token tiers for API planning.",
          resolvedImage: null,
          resolvedSiteName: "OpenAI Docs",
          topicLabels: ["Pricing", "API"],
          bookmarkQualityScore: 84,
          isDuplicateOf: null,
          bookmarkFolderPath: "Bookmarks Bar / Research / Pricing",
          enrichmentStatus: "done",
          enrichedAt: "2026-04-01T09:00:00.000Z",
          lastEnrichedAt: "2026-04-01T09:00:00.000Z",
          externalId: "bookmark-1",
          folderPath: "Bookmarks Bar / Research / Pricing",
          sourceApp: "chrome",
          sourceWindow: null,
          createdAt: "2026-04-01T09:00:00.000Z",
          updatedAt: "2026-04-01T09:00:00.000Z",
        },
        {
          id: "bookmark-duplicate",
          sourceType: "bookmark",
          title: "Pricing docs",
          content: "https://platform.openai.com/docs/pricing?utm_source=x",
          note: null,
          projectId: null,
          projectName: null,
          url: "https://platform.openai.com/docs/pricing?utm_source=x",
          domain: "platform.openai.com",
          resolvedDomain: "platform.openai.com",
          canonicalUrl: "https://platform.openai.com/docs/pricing",
          resolvedTitle: "Pricing docs",
          resolvedDescription: null,
          resolvedImage: null,
          resolvedSiteName: "OpenAI Docs",
          topicLabels: ["Pricing"],
          bookmarkQualityScore: 32,
          isDuplicateOf: "bookmark-primary",
          bookmarkFolderPath: "Bookmarks Bar / Random",
          enrichmentStatus: "done",
          enrichedAt: "2026-04-01T09:00:00.000Z",
          lastEnrichedAt: "2026-04-01T09:00:00.000Z",
          externalId: "bookmark-2",
          folderPath: "Bookmarks Bar / Random",
          sourceApp: "chrome",
          sourceWindow: null,
          createdAt: "2026-04-01T09:05:00.000Z",
          updatedAt: "2026-04-01T09:05:00.000Z",
        },
      ],
      projects: [],
      query: { text: "openai pricing", limit: 10 },
    });

    expect(results[0]?.memory.id).toBe("bookmark-primary");
  });
});
