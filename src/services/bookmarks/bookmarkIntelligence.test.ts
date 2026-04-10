import { describe, expect, it } from "vitest";

import type { Memory, Project } from "@/domain/types";
import {
  getBookmarksRelatedToActiveProject,
  getRecentBookmarks,
  getTopBookmarkDomains,
  getUsefulForgottenBookmarks,
} from "@/services/bookmarks/bookmarkIntelligence";

const baseBookmark = (overrides: Partial<Memory>): Memory => ({
  id: overrides.id ?? "memory-1",
  sourceType: "bookmark",
  title: overrides.title ?? "OpenAI pricing",
  content: overrides.content ?? "https://platform.openai.com/docs/pricing",
  note: overrides.note ?? null,
  projectId: overrides.projectId ?? null,
  projectName: overrides.projectName ?? null,
  url: overrides.url ?? "https://platform.openai.com/docs/pricing",
  domain: overrides.domain ?? "platform.openai.com",
  resolvedDomain: overrides.resolvedDomain ?? "platform.openai.com",
  canonicalUrl: overrides.canonicalUrl ?? "https://platform.openai.com/docs/pricing",
  resolvedTitle: overrides.resolvedTitle ?? "OpenAI pricing guide",
  resolvedDescription:
    overrides.resolvedDescription ??
    "Review API pricing, token tiers, and model usage decisions.",
  resolvedImage: overrides.resolvedImage ?? null,
  resolvedSiteName: overrides.resolvedSiteName ?? "OpenAI Docs",
  topicLabels: overrides.topicLabels ?? ["Pricing", "API"],
  bookmarkQualityScore: overrides.bookmarkQualityScore ?? 68,
  isDuplicateOf: overrides.isDuplicateOf ?? null,
  bookmarkFolderPath: overrides.bookmarkFolderPath ?? "Bookmarks Bar / Research / Pricing",
  enrichmentStatus: overrides.enrichmentStatus ?? "done",
  enrichedAt: overrides.enrichedAt ?? "2026-04-01T09:00:00.000Z",
  lastEnrichedAt: overrides.lastEnrichedAt ?? "2026-04-01T09:00:00.000Z",
  externalId: overrides.externalId ?? null,
  folderPath: overrides.folderPath ?? "Bookmarks Bar / Research / Pricing",
  sourceApp: overrides.sourceApp ?? "chrome",
  sourceWindow: overrides.sourceWindow ?? null,
  createdAt: overrides.createdAt ?? "2026-04-01T09:00:00.000Z",
  updatedAt: overrides.updatedAt ?? "2026-04-01T09:00:00.000Z",
});

const manualMemory: Memory = {
  ...baseBookmark({ id: "manual-1" }),
  sourceType: "manual",
  title: "Strategy notes",
  content: "Pricing memo",
  url: null,
  domain: null,
  resolvedDomain: null,
  canonicalUrl: null,
  topicLabels: null,
  bookmarkQualityScore: null,
  bookmarkFolderPath: null,
};

const projects: Project[] = [
  {
    id: "project-pricing",
    name: "Pricing Research",
    description: "Collect pricing and monetization references",
    createdAt: "2026-03-01T09:00:00.000Z",
    updatedAt: "2026-04-01T09:00:00.000Z",
  },
];

describe("bookmark intelligence dashboard hooks", () => {
  it("filters recent bookmarks to non-duplicates", () => {
    const bookmarks = [
      baseBookmark({ id: "bookmark-new", updatedAt: "2026-04-09T09:00:00.000Z" }),
      baseBookmark({
        id: "bookmark-dup",
        updatedAt: "2026-04-10T09:00:00.000Z",
        isDuplicateOf: "bookmark-new",
      }),
      manualMemory,
    ];

    expect(getRecentBookmarks(bookmarks, 3).map((bookmark) => bookmark.id)).toEqual([
      "bookmark-new",
    ]);
  });

  it("finds useful forgotten bookmarks by quality and age", () => {
    const bookmarks = [
      baseBookmark({
        id: "bookmark-forgotten",
        bookmarkQualityScore: 82,
        updatedAt: "2026-03-15T09:00:00.000Z",
      }),
      baseBookmark({
        id: "bookmark-recent",
        bookmarkQualityScore: 90,
        updatedAt: "2026-04-09T09:00:00.000Z",
      }),
    ];

    expect(getUsefulForgottenBookmarks(bookmarks, 5).map((bookmark) => bookmark.id)).toEqual([
      "bookmark-forgotten",
    ]);
  });

  it("aggregates top domains", () => {
    const bookmarks = [
      baseBookmark({ id: "a", resolvedDomain: "github.com", domain: "github.com" }),
      baseBookmark({
        id: "b",
        resolvedDomain: "github.com",
        domain: "github.com",
        bookmarkQualityScore: 80,
      }),
      baseBookmark({
        id: "c",
        resolvedDomain: "youtube.com",
        domain: "youtube.com",
      }),
    ];

    const insights = getTopBookmarkDomains(bookmarks, 3);
    expect(insights[0]?.domain).toBe("github.com");
    expect(insights[0]?.count).toBe(2);
  });

  it("matches bookmarks to the active project through topics and resolved metadata", () => {
    const bookmarks = [
      baseBookmark({
        id: "pricing-match",
        topicLabels: ["Pricing", "Monetization"],
        resolvedDescription: "Pricing experiments and monetization breakdowns.",
      }),
      baseBookmark({
        id: "unrelated",
        title: "Workout planning guide",
        url: "https://example.com/fitness/workout-plans",
        domain: "example.com",
        resolvedDomain: "example.com",
        canonicalUrl: "https://example.com/fitness/workout-plans",
        resolvedTitle: "Workout planning guide",
        topicLabels: ["Fitness", "Wellness"],
        resolvedDescription: "Workout planning resources.",
        bookmarkFolderPath: "Bookmarks Bar / Health / Fitness",
        folderPath: "Bookmarks Bar / Health / Fitness",
      }),
    ];

    expect(
      getBookmarksRelatedToActiveProject(
        bookmarks,
        projects,
        "project-pricing",
        4,
      ).map((bookmark) => bookmark.id),
    ).toEqual(["pricing-match"]);
  });
});
