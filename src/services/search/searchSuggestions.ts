import type { Memory, SearchSuggestion } from "@/domain/types";
import {
  getMemoryDisplayDomain,
  getMemoryDisplayTitle,
  normalizeDisplayText,
} from "@/domain/formatters";

const MAX_SUGGESTIONS = 3;

const STOPWORDS = new Set([
  "a",
  "about",
  "an",
  "and",
  "find",
  "for",
  "from",
  "i",
  "in",
  "it",
  "me",
  "my",
  "of",
  "on",
  "or",
  "saved",
  "show",
  "that",
  "the",
  "thing",
  "this",
  "to",
  "with",
]);

const normalizeTokenText = (value: string | null | undefined) =>
  normalizeDisplayText(value)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();

const tokenize = (value: string | null | undefined) =>
  normalizeTokenText(value)
    .split(" ")
    .filter((token) => token.length > 1 && !STOPWORDS.has(token));

const clamp = (value: number, min = 0, max = 100) =>
  Math.max(min, Math.min(max, value));

const recencyScore = (memory: Memory) => {
  const timestamp = new Date(memory.updatedAt || memory.createdAt).getTime();
  if (!Number.isFinite(timestamp)) return 0;
  const ageHours = (Date.now() - timestamp) / (1000 * 60 * 60);
  return clamp(100 - ageHours / 3.36);
};

const qualityScore = (memory: Memory) =>
  clamp(memory.qualityScore ?? memory.bookmarkQualityScore ?? 0);

const matchTokens = (queryTokens: string[], candidateTokens: string[]) => {
  if (queryTokens.length === 0 || candidateTokens.length === 0) return [];
  const matches = new Set<string>();

  for (const queryToken of queryTokens) {
    for (const candidateToken of candidateTokens) {
      if (
        candidateToken === queryToken ||
        candidateToken.startsWith(queryToken) ||
        (queryToken.length >= 4 && candidateToken.includes(queryToken))
      ) {
        matches.add(candidateToken);
      }
    }
  }

  return Array.from(matches);
};

const suggestionReason = (memory: Memory, matchedTopics: string[]) => {
  if (matchedTopics.length > 0) {
    return `Topic: ${matchedTopics.slice(0, 2).join(", ")}`;
  }

  const domain = getMemoryDisplayDomain(memory);
  if (domain) return domain;

  return memory.sourceType === "bookmark" ? "Related bookmark" : "Related memory";
};

export const getSearchSuggestions = (
  memories: Memory[],
  queryText: string,
): SearchSuggestion[] => {
  const queryTokens = Array.from(new Set(tokenize(queryText)));
  if (queryTokens.length === 0) return [];

  return memories
    .map((memory) => {
      const topicLabels = [
        memory.primaryTopic,
        ...(memory.topicLabels ?? []),
      ].filter(Boolean) as string[];
      const topicTokens = tokenize(topicLabels.join(" "));
      const titleTokens = tokenize(getMemoryDisplayTitle(memory));
      const domainTokens = tokenize(
        [
          getMemoryDisplayDomain(memory),
          memory.domain,
          memory.resolvedDomain,
          memory.resolvedSiteName,
          memory.memoryType,
        ]
          .filter(Boolean)
          .join(" "),
      );
      const matchedTopicTokens = matchTokens(queryTokens, topicTokens);
      const matchedTopicLabels = topicLabels.filter(
        (label) => matchTokens(queryTokens, tokenize(label)).length > 0,
      );
      const matchedContextTokens = matchTokens(queryTokens, [
        ...titleTokens,
        ...domainTokens,
      ]);
      const topicMatchScore =
        matchedTopicTokens.length > 0
          ? clamp((matchedTopicTokens.length / queryTokens.length) * 100)
          : 0;
      const contextScore =
        matchedContextTokens.length > 0
          ? clamp((matchedContextTokens.length / queryTokens.length) * 100)
          : 0;

      // Suggestions are intentionally topic-led; title/domain only help break ties.
      const score =
        topicMatchScore * 0.48 +
        qualityScore(memory) * 0.24 +
        recencyScore(memory) * 0.18 +
        contextScore * 0.1;

      return {
        memory,
        score,
        reason: suggestionReason(memory, matchedTopicLabels),
        matchedTopics: matchedTopicLabels,
      };
    })
    .filter((suggestion) => suggestion.score >= 24 && suggestion.matchedTopics.length > 0)
    .sort((left, right) => {
      if (right.score !== left.score) return right.score - left.score;
      return (
        new Date(right.memory.updatedAt || right.memory.createdAt).getTime() -
        new Date(left.memory.updatedAt || left.memory.createdAt).getTime()
      );
    })
    .slice(0, MAX_SUGGESTIONS);
};
