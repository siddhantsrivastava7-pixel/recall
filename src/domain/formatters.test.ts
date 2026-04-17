import { describe, expect, it } from "vitest";

import {
  getMemoryDetailReadingText,
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
  hasMeaningfulMemoryPreview,
} from "@/domain/formatters";
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

  it("prefers resolved title over a low-signal domain title", () => {
    const result = getMemoryDisplayTitle({
      ...baseMemory,
      title: "x.com",
      content: "https://x.com/example/status/123",
      url: "https://x.com/example/status/123",
      domain: "x.com",
      resolvedDomain: "x.com",
      resolvedTitle: "X post by @example",
    });

    expect(result).toBe("X post by @example");
  });

  it("prefers resolved title over Reddit verification shell title", () => {
    const result = getMemoryDisplayTitle({
      ...baseMemory,
      title: "Reddit - Please wait for verification",
      content: "https://www.reddit.com/r/rust/comments/1abc234/tauri_app_architecture/",
      url: "https://www.reddit.com/r/rust/comments/1abc234/tauri_app_architecture/",
      domain: "reddit.com",
      resolvedDomain: "reddit.com",
      resolvedTitle: "Reddit - Tauri App Architecture",
    });

    expect(result).toBe("Reddit - Tauri App Architecture");
  });

  it("generates a useful title from GitHub URLs when metadata is missing", () => {
    const result = getMemoryDisplayTitle({
      ...baseMemory,
      title: "github.com",
      content: "https://github.com/siddhantsrivastava7-pixel/merapolicyadvisor",
      url: "https://github.com/siddhantsrivastava7-pixel/merapolicyadvisor",
      domain: "github.com",
      resolvedTitle: null,
    });

    expect(result).toBe("GitHub - siddhantsrivastava7-pixel/merapolicyadvisor");
  });
});

describe("getMemoryDisplayPreview", () => {
  it("uses parsable link context before falling back to raw URL content", () => {
    const result = getMemoryDisplayPreview({
      ...baseMemory,
      content: "https://example.com/raw/link",
      url: "https://example.com/raw/link",
      domain: "example.com",
      summaryText: "Saved link from example.com. Open the source to view the saved page.",
    });

    expect(result).toBe("example.com - Link.");
  });

  it("prefers extracted preview over generic bookmark summaries", () => {
    const memory = {
      ...baseMemory,
      sourceType: "bookmark" as const,
      content: "https://x.com/VaibhavSisinty/status/204846683083919547",
      url: "https://x.com/VaibhavSisinty/status/204846683083919547",
      domain: "x.com",
      resolvedDomain: "x.com",
      previewText:
        "Let me explain what OpenAI just did with the new Codex update. Because most people are going to miss the actual story here.",
      summaryText: "Saved link from x.com. Open the source to view the saved page.",
    };

    expect(getMemoryDisplayPreview(memory, 220)).toBe(
      "Let me explain what OpenAI just did with the new Codex update. Because most people are going to miss the actual story here.",
    );
    expect(hasMeaningfulMemoryPreview(memory)).toBe(true);
  });
});

describe("getMemoryDetailReadingText", () => {
  it("shows extracted social post text instead of a generic saved-link fallback", () => {
    const result = getMemoryDetailReadingText({
      ...baseMemory,
      content: "https://x.com/VaibhavSisinty/status/204846683083919547",
      url: "https://x.com/VaibhavSisinty/status/204846683083919547",
      domain: "x.com",
      resolvedDomain: "x.com",
      previewText:
        "Let me explain what OpenAI just did with the new Codex update. Because most people are going to miss the actual story here.",
      summaryText: "Saved link from x.com. Open the source to view the saved page.",
    });

    expect(result).toBe(
      "Let me explain what OpenAI just did with the new Codex update. Because most people are going to miss the actual story here.",
    );
  });

  it("falls back to parsable URL context before generic link text", () => {
    const result = getMemoryDetailReadingText({
      ...baseMemory,
      content: "https://www.reddit.com/r/ClaudeAI/comments/1sg4x27/codex_vs_claude_brutal/",
      url: "https://www.reddit.com/r/ClaudeAI/comments/1sg4x27/codex_vs_claude_brutal/",
      domain: "reddit.com",
      resolvedDomain: "reddit.com",
      previewText: null,
      resolvedDescription: null,
      summaryText: "Saved link from reddit.com. Open the source to view the saved page.",
    });

    expect(result).toBe("Reddit thread in r/ClaudeAI: Codex Vs Claude Brutal.");
  });

  it("shows repository context for GitHub links without fetched metadata", () => {
    const result = getMemoryDetailReadingText({
      ...baseMemory,
      content: "https://github.com/siddhantsrivastava7-pixel/merapolicyadvisor",
      url: "https://github.com/siddhantsrivastava7-pixel/merapolicyadvisor",
      domain: "github.com",
      previewText: null,
      resolvedDescription: null,
      summaryText: "Saved link from github.com. Open the source to view the saved page.",
    });

    expect(result).toBe("GitHub repository: siddhantsrivastava7-pixel/merapolicyadvisor.");
  });
});
