import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  searchEvaluationCases,
} from "@/services/search/searchRankingFixtures";
import {
  formatSearchEvaluationSummary,
  runSearchEvaluationSuite,
} from "@/services/search/searchEvaluationRunner";

describe("Search evaluation harness", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-09T12:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("prints the representative ranking matrix", () => {
    const summary = runSearchEvaluationSuite(searchEvaluationCases);
    console.log(formatSearchEvaluationSummary(summary));
    expect(summary.failedCount).toBe(0);
    expect(summary.percentage).toBe(100);
  });
});
