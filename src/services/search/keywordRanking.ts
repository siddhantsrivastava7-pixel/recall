import type { Memory, Project } from "@/domain/types";

type SearchField =
  | "title"
  | "project"
  | "note"
  | "content"
  | "url"
  | "folder"
  | "sourceApp"
  | "topics";
type MatchQuality = "exact" | "prefix" | "partial" | "fuzzy";

interface QueryProfile {
  rawText: string;
  normalizedText: string;
  allTokens: string[];
  effectiveTokens: string[];
  looksLikeTitleOrProject: boolean;
  looksLikeSourceLookup: boolean;
}

interface TokenMatch {
  queryToken: string;
  quality: MatchQuality;
  position: number;
}

interface FieldAnalysis {
  field: SearchField;
  label: string;
  text: string;
  tokens: string[];
  exactFieldMatch: boolean;
  exactPhraseMatch: boolean;
  matches: TokenMatch[];
}

interface ScoreConfig {
  fieldExactMatch: number;
  phraseBonus: number;
  exactToken: number;
  prefixToken: number;
  partialToken: number;
  fuzzyToken: number;
  orderBonus: number;
  proximityBonus: number;
  contiguousBonus: number;
  fuzzyOnlyPenalty: number;
  weakSingleTokenPenalty: number;
  longBodyWeakPenalty: number;
}

interface RankedFieldScore {
  label: string;
  score: number;
}

export interface KeywordRankingResult {
  score: number;
  highlights: string[];
}

const FIELD_LABELS: Record<SearchField, string> = {
  title: "Title",
  project: "Project",
  note: "Note",
  content: "Content",
  url: "URL",
  folder: "Folder",
  sourceApp: "Source",
  topics: "Topics",
};

const FIELD_SCORES: Record<SearchField, ScoreConfig> = {
  title: {
    fieldExactMatch: 420,
    phraseBonus: 260,
    exactToken: 92,
    prefixToken: 48,
    partialToken: 24,
    fuzzyToken: 14,
    orderBonus: 88,
    proximityBonus: 54,
    contiguousBonus: 68,
    fuzzyOnlyPenalty: 18,
    weakSingleTokenPenalty: 0,
    longBodyWeakPenalty: 0,
  },
  project: {
    fieldExactMatch: 280,
    phraseBonus: 178,
    exactToken: 58,
    prefixToken: 32,
    partialToken: 18,
    fuzzyToken: 10,
    orderBonus: 52,
    proximityBonus: 28,
    contiguousBonus: 34,
    fuzzyOnlyPenalty: 14,
    weakSingleTokenPenalty: 0,
    longBodyWeakPenalty: 0,
  },
  note: {
    fieldExactMatch: 210,
    phraseBonus: 132,
    exactToken: 42,
    prefixToken: 20,
    partialToken: 12,
    fuzzyToken: 6,
    orderBonus: 28,
    proximityBonus: 18,
    contiguousBonus: 18,
    fuzzyOnlyPenalty: 12,
    weakSingleTokenPenalty: 8,
    longBodyWeakPenalty: 0,
  },
  content: {
    fieldExactMatch: 160,
    phraseBonus: 96,
    exactToken: 26,
    prefixToken: 14,
    partialToken: 8,
    fuzzyToken: 4,
    orderBonus: 18,
    proximityBonus: 12,
    contiguousBonus: 14,
    fuzzyOnlyPenalty: 14,
    weakSingleTokenPenalty: 10,
    longBodyWeakPenalty: 14,
  },
  url: {
    fieldExactMatch: 140,
    phraseBonus: 84,
    exactToken: 24,
    prefixToken: 14,
    partialToken: 10,
    fuzzyToken: 3,
    orderBonus: 16,
    proximityBonus: 10,
    contiguousBonus: 10,
    fuzzyOnlyPenalty: 10,
    weakSingleTokenPenalty: 6,
    longBodyWeakPenalty: 0,
  },
  folder: {
    fieldExactMatch: 100,
    phraseBonus: 54,
    exactToken: 16,
    prefixToken: 10,
    partialToken: 6,
    fuzzyToken: 3,
    orderBonus: 12,
    proximityBonus: 8,
    contiguousBonus: 8,
    fuzzyOnlyPenalty: 9,
    weakSingleTokenPenalty: 6,
    longBodyWeakPenalty: 0,
  },
  sourceApp: {
    fieldExactMatch: 108,
    phraseBonus: 60,
    exactToken: 18,
    prefixToken: 12,
    partialToken: 6,
    fuzzyToken: 3,
    orderBonus: 10,
    proximityBonus: 6,
    contiguousBonus: 6,
    fuzzyOnlyPenalty: 9,
    weakSingleTokenPenalty: 6,
    longBodyWeakPenalty: 0,
  },
  topics: {
    fieldExactMatch: 188,
    phraseBonus: 112,
    exactToken: 34,
    prefixToken: 18,
    partialToken: 10,
    fuzzyToken: 5,
    orderBonus: 20,
    proximityBonus: 14,
    contiguousBonus: 14,
    fuzzyOnlyPenalty: 10,
    weakSingleTokenPenalty: 4,
    longBodyWeakPenalty: 0,
  },
};

const SAFE_STOPWORDS = new Set([
  "a",
  "about",
  "an",
  "find",
  "for",
  "i",
  "it",
  "me",
  "my",
  "saved",
  "show",
  "that",
  "the",
  "thing",
  "this",
]);

const SOURCE_HINTS = new Set([
  "bookmark",
  "bookmarks",
  "brave",
  "browser",
  "chrome",
  "domain",
  "edge",
  "firefox",
  "link",
  "page",
  "site",
  "source",
  "url",
  "web",
  "website",
]);

const normalizeText = (value: string | null | undefined) =>
  (value ?? "")
    .toLowerCase()
    .replace(/['"`]+/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();

const tokenize = (value: string) => normalizeText(value).split(" ").filter(Boolean);

const buildQueryProfile = (rawText: string): QueryProfile => {
  const normalizedText = normalizeText(rawText);
  const allTokens = tokenize(rawText);
  const effectiveTokens = allTokens.filter((token) => !SAFE_STOPWORDS.has(token));
  const tokens = effectiveTokens.length > 0 ? effectiveTokens : allTokens;
  const looksLikeSourceLookup =
    /https?:\/\//i.test(rawText) ||
    /\bwww\./i.test(rawText) ||
    tokens.some((token) => SOURCE_HINTS.has(token));
  const looksLikeTitleOrProject =
    !looksLikeSourceLookup &&
    tokens.length > 0 &&
    tokens.length <= 5 &&
    normalizedText.length <= 48;

  return {
    rawText,
    normalizedText,
    allTokens,
    effectiveTokens: tokens,
    looksLikeTitleOrProject,
    looksLikeSourceLookup,
  };
};

const getFuzzyThreshold = (token: string) => {
  if (token.length >= 8) return 2;
  if (token.length >= 4) return 1;
  return 0;
};

const levenshteinDistance = (left: string, right: string) => {
  if (left === right) return 0;
  if (left.length === 0) return right.length;
  if (right.length === 0) return left.length;

  const previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  const current = new Array(right.length + 1).fill(0);

  for (let row = 1; row <= left.length; row += 1) {
    current[0] = row;
    for (let column = 1; column <= right.length; column += 1) {
      const substitutionCost = left[row - 1] === right[column - 1] ? 0 : 1;
      current[column] = Math.min(
        current[column - 1] + 1,
        previous[column] + 1,
        previous[column - 1] + substitutionCost,
      );
    }
    previous.splice(0, previous.length, ...current);
  }

  return previous[right.length];
};

const classifyTokenMatch = (queryToken: string, fieldToken: string): MatchQuality | null => {
  if (fieldToken === queryToken) {
    return "exact";
  }

  if (queryToken.length >= 2 && fieldToken.startsWith(queryToken)) {
    return "prefix";
  }

  if (
    queryToken.length >= 4 &&
    (fieldToken.includes(queryToken) || queryToken.includes(fieldToken))
  ) {
    return "partial";
  }

  const fuzzyThreshold = getFuzzyThreshold(queryToken);
  if (
    fuzzyThreshold > 0 &&
    Math.abs(fieldToken.length - queryToken.length) <= fuzzyThreshold &&
    levenshteinDistance(queryToken, fieldToken) <= fuzzyThreshold
  ) {
    return "fuzzy";
  }

  return null;
};

const matchPriority = (quality: MatchQuality) => {
  switch (quality) {
    case "exact":
      return 4;
    case "prefix":
      return 3;
    case "partial":
      return 2;
    case "fuzzy":
      return 1;
  }
};

const analyzeField = (
  field: SearchField,
  value: string | null | undefined,
  query: QueryProfile,
): FieldAnalysis => {
  const text = normalizeText(value);
  const tokens = text.split(" ").filter(Boolean);
  const matches: TokenMatch[] = [];
  const usedPositions = new Set<number>();

  for (const queryToken of query.effectiveTokens) {
    let bestMatch: TokenMatch | null = null;

    for (let position = 0; position < tokens.length; position += 1) {
      const fieldToken = tokens[position];
      if (usedPositions.has(position)) {
        continue;
      }

      const quality = classifyTokenMatch(queryToken, fieldToken);
      if (!quality) {
        continue;
      }

      const candidate: TokenMatch = { queryToken, quality, position };
      if (!bestMatch || matchPriority(candidate.quality) > matchPriority(bestMatch.quality)) {
        bestMatch = candidate;
      }
    }

    if (bestMatch) {
      usedPositions.add(bestMatch.position);
      matches.push(bestMatch);
    }
  }

  return {
    field,
    label: FIELD_LABELS[field],
    text,
    tokens,
    exactFieldMatch: text.length > 0 && text === query.normalizedText,
    exactPhraseMatch:
      query.normalizedText.length > 0 && text.length > 0 && text.includes(query.normalizedText),
    matches,
  };
};

const countMatches = (analysis: FieldAnalysis, quality: MatchQuality) =>
  analysis.matches.filter((match) => match.quality === quality).length;

const computeOrderBonus = (analysis: FieldAnalysis, config: ScoreConfig) => {
  if (analysis.matches.length < 2) {
    return 0;
  }

  const positions = analysis.matches.map((match) => match.position);
  let increasingPairs = 0;
  for (let index = 1; index < positions.length; index += 1) {
    if (positions[index] > positions[index - 1]) {
      increasingPairs += 1;
    }
  }

  const orderRatio = increasingPairs / Math.max(1, positions.length - 1);
  const span = Math.max(...positions) - Math.min(...positions) + 1;
  const density = positions.length / span;
  const contiguous =
    orderRatio === 1 &&
    span === positions.length &&
    analysis.matches.every((match) => match.quality !== "fuzzy");

  let bonus = config.orderBonus * orderRatio;
  bonus += config.proximityBonus * density * Math.min(1, positions.length / 3);
  if (contiguous) {
    bonus += config.contiguousBonus;
  }

  return bonus;
};

const applyIntentMultiplier = (
  field: SearchField,
  query: QueryProfile,
  score: number,
) => {
  let multiplier = 1;

  if (query.looksLikeTitleOrProject) {
    if (field === "title") multiplier *= 1.18;
    if (field === "project") multiplier *= 1.12;
    if (field === "content") multiplier *= 0.94;
  }

  if (query.looksLikeSourceLookup) {
    if (field === "url") multiplier *= 1.32;
    if (field === "folder") multiplier *= 1.16;
    if (field === "sourceApp") multiplier *= 1.24;
    if (field === "title") multiplier *= 0.96;
  }

  return score * multiplier;
};

const scoreField = (analysis: FieldAnalysis, query: QueryProfile): RankedFieldScore => {
  const config = FIELD_SCORES[analysis.field];
  const exactCount = countMatches(analysis, "exact");
  const prefixCount = countMatches(analysis, "prefix");
  const partialCount = countMatches(analysis, "partial");
  const fuzzyCount = countMatches(analysis, "fuzzy");
  const matchedTokenCount = analysis.matches.length;

  if (matchedTokenCount === 0 && !analysis.exactPhraseMatch && !analysis.exactFieldMatch) {
    return { label: analysis.label, score: 0 };
  }

  let score = 0;

  if (analysis.exactFieldMatch) score += config.fieldExactMatch;
  if (analysis.exactPhraseMatch) score += config.phraseBonus;
  score += exactCount * config.exactToken;
  score += prefixCount * config.prefixToken;
  score += partialCount * config.partialToken;
  score += fuzzyCount * config.fuzzyToken;
  score += computeOrderBonus(analysis, config);

  if (
    fuzzyCount > 0 &&
    exactCount === 0 &&
    prefixCount === 0 &&
    partialCount === 0 &&
    !analysis.exactPhraseMatch
  ) {
    score -= config.fuzzyOnlyPenalty;
  }

  if (
    matchedTokenCount === 1 &&
    !analysis.exactPhraseMatch &&
    analysis.tokens.length >= 8 &&
    (partialCount > 0 || fuzzyCount > 0)
  ) {
    score -= config.weakSingleTokenPenalty;
  }

  if (
    analysis.field === "content" &&
    matchedTokenCount === 1 &&
    !analysis.exactPhraseMatch &&
    analysis.tokens.length >= 28
  ) {
    score -= config.longBodyWeakPenalty;
  }

  return {
    label: analysis.label,
    score: applyIntentMultiplier(analysis.field, query, score),
  };
};

const buildFieldAnalyses = (
  memory: Memory,
  projects: Project[],
  query: QueryProfile,
): FieldAnalysis[] => {
  const resolvedProjectName =
    projects.find((project) => project.id === memory.projectId)?.name ?? memory.projectName;
  const titleText = [memory.title, memory.resolvedTitle].filter(Boolean).join("\n");
  const contentText = [
    memory.content,
    memory.extractedText,
    memory.summaryText,
    memory.previewText,
    memory.resolvedDescription,
    // v0.2.0 — OCR text from screenshot / imported_image memories. Same
    // weight as content so a phrase matched only via OCR ranks the same
    // as a phrase matched in the body of a text memory.
    memory.ocrText,
  ]
    .filter(Boolean)
    .join("\n");
  const urlText = [
    memory.url,
    memory.canonicalUrl,
    memory.domain,
    memory.resolvedDomain,
    memory.resolvedSiteName,
    memory.memoryType,
  ]
    .filter(Boolean)
    .join(" ");
  const folderText = [memory.bookmarkFolderPath, memory.folderPath].filter(Boolean).join(" ");
  const topicsText = [memory.primaryTopic, ...(memory.topicLabels ?? [])]
    .filter(Boolean)
    .join(" ");

  return [
    analyzeField("title", titleText, query),
    analyzeField("project", resolvedProjectName, query),
    analyzeField("note", memory.note, query),
    analyzeField("content", contentText, query),
    analyzeField("url", urlText, query),
    analyzeField("folder", folderText, query),
    analyzeField("sourceApp", memory.sourceApp, query),
    analyzeField("topics", topicsText, query),
  ];
};

const computeNoisePenalty = (
  fieldScores: RankedFieldScore[],
  query: QueryProfile,
  tokenCoverageRatio: number,
) => {
  const scoreByLabel = new Map(fieldScores.map((score) => [score.label, score.score]));
  const primaryScore =
    (scoreByLabel.get("Title") ?? 0) +
    (scoreByLabel.get("Project") ?? 0) +
    (scoreByLabel.get("Note") ?? 0) +
    (scoreByLabel.get("Content") ?? 0);
  const metadataScore =
    (scoreByLabel.get("URL") ?? 0) +
    (scoreByLabel.get("Folder") ?? 0) +
    (scoreByLabel.get("Source") ?? 0) +
    (scoreByLabel.get("Topics") ?? 0);

  let penalty = 0;

  if (
    primaryScore === 0 &&
    metadataScore > 0 &&
    tokenCoverageRatio < 1 &&
    !query.looksLikeSourceLookup
  ) {
    penalty += 24;
  }

  if (
    query.effectiveTokens.length >= 2 &&
    primaryScore > 0 &&
    primaryScore < 42 &&
    metadataScore === 0
  ) {
    penalty += 10;
  }

  if (
    query.effectiveTokens.length >= 3 &&
    tokenCoverageRatio < 0.67 &&
    !query.looksLikeSourceLookup
  ) {
    penalty += 18;
  }

  return penalty;
};

const computeRecencyTieBreaker = (memory: Memory) => {
  const updatedAt = memory.updatedAt || memory.createdAt;
  const ageHours = (Date.now() - new Date(updatedAt).getTime()) / (1000 * 60 * 60);
  return Math.max(0, 4 - Math.min(4, ageHours / 168));
};

const clamp = (value: number, min = 0, max = 100) =>
  Math.max(min, Math.min(max, value));

const qualitySignal = (memory: Memory) =>
  clamp(memory.qualityScore ?? memory.bookmarkQualityScore ?? 0);

const minimumUsefulScore = (query: QueryProfile) => {
  if (query.effectiveTokens.length >= 3) return 20;
  if (query.effectiveTokens.length === 2) return 9;
  return 7;
};

/*
  Ranking examples this module is optimized for:
  - "pricing strategy" should favor an exact title phrase over a recent content mention.
  - "that thing i saved about chrome docs" should ignore filler and still retrieve the right item.
  - "landng page hooks" should survive a light typo, but still rank below exact matches.
*/
export const scoreMemoryForKeywordQuery = (
  memory: Memory,
  projects: Project[],
  queryText: string,
): KeywordRankingResult => {
  const query = buildQueryProfile(queryText);
  if (!query.normalizedText || query.effectiveTokens.length === 0) {
    return { score: 0, highlights: [] };
  }

  const fieldAnalyses = buildFieldAnalyses(memory, projects, query);
  const fieldScores = fieldAnalyses.map((analysis) =>
    scoreField(analysis, query),
  );
  const scoreByLabel = new Map(fieldScores.map((score) => [score.label, score.score]));
  const matchedTokens = new Set(
    fieldAnalyses.flatMap((analysis) => analysis.matches.map((match) => match.queryToken)),
  );
  const tokenCoverageRatio = matchedTokens.size / query.effectiveTokens.length;
  const coverageBonus =
    matchedTokens.size * 18 +
    tokenCoverageRatio * 24 * Math.min(1, query.effectiveTokens.length / 4);
  const rawTextScore =
    fieldScores.reduce((sum, fieldScore) => sum + fieldScore.score, 0) + coverageBonus;
  const textMatchScore = clamp(rawTextScore / 7);
  const titleMatchBoost = clamp((scoreByLabel.get("Title") ?? 0) / 4.2);
  const topicMatchScore = clamp((scoreByLabel.get("Topics") ?? 0) / 1.8);
  const recencyScore = clamp((computeRecencyTieBreaker(memory) / 4) * 100);
  const qualityScore = qualitySignal(memory);
  // V2 intelligence ranking keeps the final relevance blend easy to reason about:
  // text match leads, title/topic intelligence steer intent, recency/quality break close ties.
  const weightedRelevanceScore =
    textMatchScore * 0.4 +
    titleMatchBoost * 0.2 +
    recencyScore * 0.15 +
    qualityScore * 0.15 +
    topicMatchScore * 0.1;
  const duplicatePenalty =
    memory.sourceType === "bookmark" && memory.isDuplicateOf ? 28 : 0;
  const score =
    weightedRelevanceScore -
    computeNoisePenalty(fieldScores, query, tokenCoverageRatio) / 4 -
    duplicatePenalty;

  if (score < minimumUsefulScore(query)) {
    return { score: 0, highlights: [] };
  }

  return {
    score,
    highlights: fieldScores
      .filter((fieldScore) => fieldScore.score >= 18)
      .sort((left, right) => right.score - left.score)
      .slice(0, 3)
      .map((fieldScore) => fieldScore.label),
  };
};
