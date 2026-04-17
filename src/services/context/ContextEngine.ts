import type { Memory, Project, SearchSuggestion } from "@/domain/types";
import {
  getMemoryDisplayDomain,
  getMemoryDisplayPreview,
  getMemoryDisplayTitle,
  normalizeDisplayText,
} from "@/domain/formatters";

const DAY_MS = 24 * 60 * 60 * 1000;
const MAX_CONTEXT_TERMS = 12;

const STOPWORDS = new Set([
  "a",
  "about",
  "an",
  "and",
  "app",
  "browser",
  "co",
  "com",
  "context",
  "dev",
  "for",
  "from",
  "http",
  "https",
  "i",
  "in",
  "inbox",
  "io",
  "it",
  "link",
  "me",
  "my",
  "net",
  "of",
  "on",
  "or",
  "org",
  "page",
  "project",
  "recall",
  "saved",
  "show",
  "site",
  "that",
  "the",
  "thing",
  "this",
  "to",
  "url",
  "with",
  "www",
]);

const BROAD_RELATION_DOMAINS = new Set([
  "docs.google.com",
  "github.com",
  "google.com",
  "mobile.twitter.com",
  "twitter.com",
  "x.com",
  "youtu.be",
  "youtube.com",
]);

export interface SessionContextInput {
  recentQueries: string[];
  recentlyOpenedMemoryIds: string[];
  recentCaptureIds: string[];
  activeProjectId?: string | "all" | null;
}

export interface SessionContext {
  topicWeights: Map<string, number>;
  domainWeights: Map<string, number>;
  recentQueries: string[];
  recentlyOpenedMemoryIds: string[];
  recentCaptureIds: string[];
  activeProjectId: string | "all";
}

export interface ContextualMemory {
  memory: Memory;
  score: number;
  reason: string;
}

export interface RecallFeed {
  usefulAgainNow: ContextualMemory[];
  relatedFromEarlier: ContextualMemory[];
  youMightAlsoNeed: ContextualMemory[];
  projectRelevant: ContextualMemory[];
}

const clamp = (value: number, min = 0, max = 100) =>
  Math.max(min, Math.min(max, value));

const tokenize = (value: string | null | undefined) =>
  normalizeDisplayText(value)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .split(" ")
    .filter((token) => token.length > 1 && !STOPWORDS.has(token));

const unique = <T>(values: T[]) => Array.from(new Set(values));

const memoryTopics = (memory: Memory) =>
  unique([memory.primaryTopic, ...(memory.topicLabels ?? [])]
    .filter(Boolean)
    .flatMap((topic) => tokenize(topic)));

const memoryDomains = (memory: Memory) =>
  unique(
    [
      getMemoryDisplayDomain(memory),
      memory.domain,
      memory.resolvedDomain,
      memory.sourceApp,
    ]
      .filter(Boolean)
      .flatMap((domain) => tokenize(domain)),
  );

const memoryDomainValues = (memory: Memory) =>
  unique(
    [getMemoryDisplayDomain(memory), memory.domain, memory.resolvedDomain, memory.sourceApp]
      .filter(Boolean)
      .map((domain) => normalizeDisplayText(domain).toLowerCase().replace(/^www\./, ""))
      .filter(Boolean),
  );

const qualityScore = (memory: Memory) =>
  clamp(memory.qualityScore ?? memory.bookmarkQualityScore ?? 0);

const recencyScore = (iso: string | null | undefined) => {
  const timestamp = new Date(iso ?? "").getTime();
  if (!Number.isFinite(timestamp)) return 0;
  const ageDays = (Date.now() - timestamp) / DAY_MS;
  return clamp(100 - ageDays * 4);
};

const forgottenScore = (memory: Memory) => {
  const lastOpened = memory.lastOpenedAt ?? memory.updatedAt ?? memory.createdAt;
  const ageDays = (Date.now() - new Date(lastOpened).getTime()) / DAY_MS;
  return clamp(ageDays * 3.5);
};

const overlapScore = (tokens: string[], weights: Map<string, number>) =>
  tokens.reduce((sum, token) => sum + (weights.get(token) ?? 0), 0);

const addWeightedTokens = (
  weights: Map<string, number>,
  tokens: string[],
  amount: number,
) => {
  for (const token of tokens) {
    weights.set(token, (weights.get(token) ?? 0) + amount);
  }
};

const topWeightedTokens = (weights: Map<string, number>) =>
  Array.from(weights.entries())
    .sort((left, right) => right[1] - left[1])
    .slice(0, MAX_CONTEXT_TERMS)
    .map(([token]) => token);

export const buildSessionContext = (
  memories: Memory[],
  input: SessionContextInput,
): SessionContext => {
  const topicWeights = new Map<string, number>();
  const domainWeights = new Map<string, number>();
  const memoryById = new Map(memories.map((memory) => [memory.id, memory]));

  input.recentQueries.slice(0, 8).forEach((query, index) => {
    addWeightedTokens(topicWeights, tokenize(query), Math.max(1, 8 - index));
  });

  input.recentlyOpenedMemoryIds.slice(0, 12).forEach((id, index) => {
    const memory = memoryById.get(id);
    if (!memory) return;
    addWeightedTokens(topicWeights, memoryTopics(memory), Math.max(1, 7 - index * 0.45));
    addWeightedTokens(domainWeights, memoryDomains(memory), Math.max(1, 5 - index * 0.35));
  });

  input.recentCaptureIds.slice(0, 8).forEach((id, index) => {
    const memory = memoryById.get(id);
    if (!memory) return;
    addWeightedTokens(topicWeights, memoryTopics(memory), Math.max(1, 6 - index * 0.4));
    addWeightedTokens(domainWeights, memoryDomains(memory), Math.max(1, 4 - index * 0.3));
  });

  return {
    topicWeights,
    domainWeights,
    recentQueries: input.recentQueries,
    recentlyOpenedMemoryIds: input.recentlyOpenedMemoryIds,
    recentCaptureIds: input.recentCaptureIds,
    activeProjectId: input.activeProjectId ?? "all",
  };
};

const relationReason = (
  topicOverlap: number,
  domainOverlap: number,
  projectBoost: number,
) => {
  if (domainOverlap > topicOverlap) return "Same domain";
  if (topicOverlap > 0) return "Related topic";
  if (projectBoost > 0) return "Project context";
  return "Useful again";
};

const sharedTokenCount = (left: string[], right: string[]) => {
  const rightSet = new Set(right);
  return unique(left).filter((token) => rightSet.has(token)).length;
};

const relatedEvidenceScore = (current: Memory, candidate: Memory) => {
  const topicOverlap = sharedTokenCount(memoryTopics(current), memoryTopics(candidate));
  const currentDomains = memoryDomainValues(current);
  const candidateDomains = new Set(memoryDomainValues(candidate));
  const domainOverlap = currentDomains.filter(
    (domain) => candidateDomains.has(domain) && !BROAD_RELATION_DOMAINS.has(domain),
  ).length;
  const currentTextTokens = tokenize(
    `${getMemoryDisplayTitle(current)} ${getMemoryDisplayPreview(current, 180)}`,
  ).filter((token) => !/^\d+$/.test(token));
  const candidateTextTokens = tokenize(
    `${getMemoryDisplayTitle(candidate)} ${getMemoryDisplayPreview(candidate, 180)}`,
  ).filter((token) => !/^\d+$/.test(token));
  const textOverlap = sharedTokenCount(currentTextTokens, candidateTextTokens);

  return topicOverlap * 3 + domainOverlap * 2 + Math.max(0, textOverlap - 1);
};

export const scoreMemoryForContext = (
  memory: Memory,
  context: SessionContext,
  options: { preferForgotten?: boolean; projectId?: string | "all" } = {},
): ContextualMemory => {
  const topicOverlap = overlapScore(memoryTopics(memory), context.topicWeights);
  const domainOverlap = overlapScore(memoryDomains(memory), context.domainWeights);
  const projectId = options.projectId ?? context.activeProjectId;
  const projectBoost = projectId !== "all" && memory.projectId === projectId ? 18 : 0;
  const recentlyOpenedPenalty = context.recentlyOpenedMemoryIds.slice(0, 3).includes(memory.id)
    ? 18
    : 0;
  const duplicatePenalty = memory.isDuplicateOf ? 24 : 0;
  const forgotten = options.preferForgotten ? forgottenScore(memory) * 0.3 : 0;

  const score =
    topicOverlap * 7 +
    domainOverlap * 5 +
    qualityScore(memory) * 0.34 +
    recencyScore(memory.enrichedAt ?? memory.updatedAt ?? memory.createdAt) * 0.16 +
    forgotten +
    projectBoost -
    recentlyOpenedPenalty -
    duplicatePenalty;

  return {
    memory,
    score,
    reason: relationReason(topicOverlap, domainOverlap, projectBoost),
  };
};

export const getContextualSearchSuggestions = (
  memories: Memory[],
  queryText: string,
  context: SessionContext,
  limit = 3,
): SearchSuggestion[] => {
  const queryTokens = tokenize(queryText);
  if (queryTokens.length === 0) return [];
  const queryWeights = new Map(context.topicWeights);
  addWeightedTokens(queryWeights, queryTokens, 10);

  return memories
    .map((memory) => {
      const contextual = scoreMemoryForContext(memory, {
        ...context,
        topicWeights: queryWeights,
      });
      const matchedTopics = (memory.topicLabels ?? []).filter((topic) =>
        tokenize(topic).some((token) => queryTokens.includes(token) || queryWeights.has(token)),
      );

      return {
        memory,
        score: contextual.score,
        reason: matchedTopics[0] ? `Topic: ${matchedTopics[0]}` : contextual.reason,
        matchedTopics,
      };
    })
    .filter((suggestion) => suggestion.score >= 28 && suggestion.matchedTopics.length > 0)
    .sort((left, right) => right.score - left.score)
    .slice(0, limit);
};

export const getRelatedMemories = (
  current: Memory,
  memories: Memory[],
  context: SessionContext,
  limit = 4,
): ContextualMemory[] => {
  const relatedContext: SessionContext = {
    ...context,
    topicWeights: new Map(context.topicWeights),
    domainWeights: new Map(context.domainWeights),
  };
  addWeightedTokens(relatedContext.topicWeights, [
    ...memoryTopics(current),
    ...tokenize(getMemoryDisplayTitle(current)),
    ...tokenize(getMemoryDisplayPreview(current, 180)),
  ], 8);
  addWeightedTokens(relatedContext.domainWeights, memoryDomains(current), 8);

  return memories
    .filter((memory) => memory.id !== current.id)
    .map((memory) => scoreMemoryForContext(memory, relatedContext, {
      projectId: "all",
    }))
    .filter((item) => item.score >= 24 && relatedEvidenceScore(current, item.memory) >= 2)
    .sort((left, right) => right.score - left.score)
    .slice(0, limit);
};

export const getProjectRelevantMemories = (
  memories: Memory[],
  projects: Project[],
  activeProjectId: string | "all",
  limit = 4,
): ContextualMemory[] => {
  if (activeProjectId === "all") return [];
  const project = projects.find((candidate) => candidate.id === activeProjectId);
  if (!project) return [];
  const topicWeights = new Map<string, number>();
  addWeightedTokens(topicWeights, tokenize(`${project.name} ${project.description ?? ""}`), 10);
  const context = buildSessionContext(memories, {
    activeProjectId,
    recentQueries: [],
    recentlyOpenedMemoryIds: [],
    recentCaptureIds: [],
  });
  context.topicWeights = topicWeights;

  return memories
    .filter((memory) => memory.projectId !== activeProjectId)
    .map((memory) => scoreMemoryForContext(memory, context, { projectId: activeProjectId }))
    .filter((item) => item.score >= 22)
    .sort((left, right) => right.score - left.score)
    .slice(0, limit);
};

export const getRecallFeed = (
  memories: Memory[],
  projects: Project[],
  context: SessionContext,
): RecallFeed => {
  const candidates = memories.filter((memory) => !memory.isDuplicateOf);
  const scored = candidates
    .map((memory) => scoreMemoryForContext(memory, context, { preferForgotten: true }))
    .sort((left, right) => right.score - left.score);
  const openedIds = new Set(context.recentlyOpenedMemoryIds);
  const recentCaptureIds = new Set(context.recentCaptureIds);

  const usefulAgainNow = scored
    .filter((item) => item.score >= 26 && !openedIds.has(item.memory.id))
    .slice(0, 4);
  const relatedFromEarlier = scored
    .filter((item) => {
      const openedAt = item.memory.lastOpenedAt;
      return openedAt && Date.now() - new Date(openedAt).getTime() > 3 * DAY_MS;
    })
    .slice(0, 4);
  const youMightAlsoNeed = scored
    .filter((item) => !recentCaptureIds.has(item.memory.id))
    .slice(0, 4);
  const projectRelevant = getProjectRelevantMemories(
    memories,
    projects,
    context.activeProjectId,
    4,
  );

  return {
    usefulAgainNow,
    relatedFromEarlier,
    youMightAlsoNeed,
    projectRelevant,
  };
};

export const summarizeSessionContext = (context: SessionContext) => ({
  topics: topWeightedTokens(context.topicWeights),
  domains: topWeightedTokens(context.domainWeights),
});
