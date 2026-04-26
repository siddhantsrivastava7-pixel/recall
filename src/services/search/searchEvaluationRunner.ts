import type { SearchResult } from "@/domain/types";
import { KeywordSearchProvider } from "@/services/search/KeywordSearchProvider";
import {
  rankingFixtureMemories,
  rankingFixtureProjects,
  type SearchEvaluationCase,
} from "@/services/search/searchRankingFixtures";

export interface SearchEvaluationFailure {
  type: "expectedTop" | "minimumRank" | "shouldNotAppear";
  message: string;
}

export interface SearchEvaluationCaseResult {
  testCase: SearchEvaluationCase;
  topResults: Array<{
    rank: number;
    id: string;
    title: string;
    score: number;
  }>;
  positions: Record<string, number | null>;
  score: number;
  maxScore: number;
  passed: boolean;
  failures: SearchEvaluationFailure[];
}

export interface SearchEvaluationSummary {
  caseResults: SearchEvaluationCaseResult[];
  totalScore: number;
  maxScore: number;
  percentage: number;
  passedCount: number;
  failedCount: number;
}

const DEFAULT_TOP_N = 10;
const SCORE_WEIGHTS = {
  expectedTopIds: 60,
  minimumRanks: 25,
  shouldNotAppear: 15,
};

const provider = new KeywordSearchProvider();

const resolvePositions = (
  allIds: string[],
  results: SearchResult[],
): Record<string, number | null> => {
  const positions: Record<string, number | null> = {};

  for (const id of allIds) {
    const index = results.findIndex((result) => result.memory.id === id);
    positions[id] = index >= 0 ? index + 1 : null;
  }

  return positions;
};

const scoreExpectedTopIds = (
  expectedTopIds: string[],
  positions: Record<string, number | null>,
): { score: number; maxScore: number; failures: SearchEvaluationFailure[] } => {
  if (expectedTopIds.length === 0) {
    return { score: 0, maxScore: 0, failures: [] };
  }

  const perItem = SCORE_WEIGHTS.expectedTopIds / expectedTopIds.length;
  let score = 0;
  const failures: SearchEvaluationFailure[] = [];

  expectedTopIds.forEach((id, index) => {
    const expectedRank = index + 1;
    const actualRank = positions[id];

    if (actualRank === null) {
      failures.push({
        type: "expectedTop",
        message: `${id} was expected in the top results but did not appear.`,
      });
      return;
    }

    if (actualRank === expectedRank) {
      score += perItem;
      return;
    }

    if (actualRank <= expectedTopIds.length + 1) {
      score += perItem * 0.6;
    }

    failures.push({
      type: "expectedTop",
      message: `${id} was expected near rank ${expectedRank} but appeared at rank ${actualRank}.`,
    });
  });

  return { score, maxScore: SCORE_WEIGHTS.expectedTopIds, failures };
};

const scoreMinimumRanks = (
  minimumRanks: Record<string, number> | undefined,
  positions: Record<string, number | null>,
): { score: number; maxScore: number; failures: SearchEvaluationFailure[] } => {
  if (!minimumRanks || Object.keys(minimumRanks).length === 0) {
    return { score: 0, maxScore: 0, failures: [] };
  }

  const entries = Object.entries(minimumRanks);
  const perItem = SCORE_WEIGHTS.minimumRanks / entries.length;
  let score = 0;
  const failures: SearchEvaluationFailure[] = [];

  entries.forEach(([id, threshold]) => {
    const actualRank = positions[id];
    if (actualRank !== null && actualRank <= threshold) {
      score += perItem;
    } else {
      failures.push({
        type: "minimumRank",
        message:
          actualRank === null
            ? `${id} was expected within rank ${threshold} but did not appear.`
            : `${id} was expected within rank ${threshold} but appeared at rank ${actualRank}.`,
      });
    }
  });

  return { score, maxScore: SCORE_WEIGHTS.minimumRanks, failures };
};

const scoreShouldNotAppear = (
  shouldNotAppear: string[] | undefined,
  positions: Record<string, number | null>,
): { score: number; maxScore: number; failures: SearchEvaluationFailure[] } => {
  if (!shouldNotAppear || shouldNotAppear.length === 0) {
    return { score: 0, maxScore: 0, failures: [] };
  }

  const perItem = SCORE_WEIGHTS.shouldNotAppear / shouldNotAppear.length;
  let score = 0;
  const failures: SearchEvaluationFailure[] = [];

  shouldNotAppear.forEach((id) => {
    const actualRank = positions[id];
    if (actualRank === null) {
      score += perItem;
    } else {
      failures.push({
        type: "shouldNotAppear",
        message: `${id} should not appear in the top results but showed up at rank ${actualRank}.`,
      });
    }
  });

  return { score, maxScore: SCORE_WEIGHTS.shouldNotAppear, failures };
};

export const runSearchEvaluationCase = (
  testCase: SearchEvaluationCase,
): SearchEvaluationCaseResult => {
  const topN = testCase.topN ?? DEFAULT_TOP_N;
  const results = provider.search({
    memories: rankingFixtureMemories,
    projects: rankingFixtureProjects,
    query: { text: testCase.query, limit: topN },
  });

  const trackedIds = Array.from(
    new Set([
      ...testCase.expectedTopIds,
      ...Object.keys(testCase.minimumRanks ?? {}),
      ...(testCase.shouldNotAppear ?? []),
    ]),
  );
  const positions = resolvePositions(trackedIds, results);

  const expectedScore = scoreExpectedTopIds(testCase.expectedTopIds, positions);
  const minimumRankScore = scoreMinimumRanks(testCase.minimumRanks, positions);
  const shouldNotAppearScore = scoreShouldNotAppear(testCase.shouldNotAppear, positions);

  const score = expectedScore.score + minimumRankScore.score + shouldNotAppearScore.score;
  const maxScore =
    expectedScore.maxScore + minimumRankScore.maxScore + shouldNotAppearScore.maxScore;
  const failures = [
    ...expectedScore.failures,
    ...minimumRankScore.failures,
    ...shouldNotAppearScore.failures,
  ];

  return {
    testCase,
    topResults: results.map((result, index) => ({
      rank: index + 1,
      id: result.memory.id,
      title: result.memory.title ?? result.memory.content,
      score: result.score,
    })),
    positions,
    score,
    maxScore,
    passed: failures.length === 0,
    failures,
  };
};

export const runSearchEvaluationSuite = (
  cases: SearchEvaluationCase[],
): SearchEvaluationSummary => {
  const caseResults = cases.map(runSearchEvaluationCase);
  const totalScore = caseResults.reduce((sum, result) => sum + result.score, 0);
  const maxScore = caseResults.reduce((sum, result) => sum + result.maxScore, 0);
  const passedCount = caseResults.filter((result) => result.passed).length;
  const failedCount = caseResults.length - passedCount;

  return {
    caseResults,
    totalScore,
    maxScore,
    percentage: maxScore === 0 ? 100 : (totalScore / maxScore) * 100,
    passedCount,
    failedCount,
  };
};

export const formatSearchEvaluationSummary = (summary: SearchEvaluationSummary) => {
  const lines: string[] = [];
  lines.push(
    `Search evaluation: ${summary.passedCount}/${summary.caseResults.length} passed  |  score ${summary.totalScore.toFixed(1)}/${summary.maxScore.toFixed(1)} (${summary.percentage.toFixed(1)}%)`,
  );
  lines.push("");

  summary.caseResults.forEach((result) => {
    lines.push(`${result.passed ? "PASS" : "FAIL"}  ${result.testCase.query}`);
    lines.push(`  score:    ${result.score.toFixed(1)}/${result.maxScore.toFixed(1)}`);
    lines.push(`  expect:   ${result.testCase.expectedTopIds.join(", ")}`);
    if (result.testCase.minimumRanks && Object.keys(result.testCase.minimumRanks).length > 0) {
      lines.push(
        `  ranks<=:  ${Object.entries(result.testCase.minimumRanks)
          .map(([id, rank]) => `${id}@${rank}`)
          .join(", ")}`,
      );
    }
    if (result.testCase.shouldNotAppear && result.testCase.shouldNotAppear.length > 0) {
      lines.push(`  exclude:  ${result.testCase.shouldNotAppear.join(", ")}`);
    }
    lines.push(
      `  top:      ${result.topResults
        .slice(0, 5)
        .map((item) => `${item.rank}.${item.id}`)
        .join("  ") || "(no results)"}`,
    );

    if (result.failures.length > 0) {
      result.failures.forEach((failure) => {
        lines.push(`  mismatch: ${failure.message}`);
      });
    }

    lines.push(`  note:     ${result.testCase.note}`);
    lines.push("");
  });

  return lines.join("\n");
};
