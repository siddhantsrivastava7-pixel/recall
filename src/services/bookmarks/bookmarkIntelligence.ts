import type { Memory, Project } from "@/domain/types";

const DAY_MS = 24 * 60 * 60 * 1000;

const normalize = (value: string | null | undefined) =>
  (value ?? "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();

const tokenize = (value: string | null | undefined) =>
  normalize(value)
    .split(" ")
    .filter((token) => token.length >= 3);

const bookmarkSortKey = (memory: Memory) =>
  new Date(memory.updatedAt || memory.lastEnrichedAt || memory.createdAt).getTime();

export const getNonDuplicateBookmarks = (memories: Memory[]) =>
  memories.filter((memory) => memory.sourceType === "bookmark" && !memory.isDuplicateOf);

export const getRecentBookmarks = (memories: Memory[], limit = 5) =>
  getNonDuplicateBookmarks(memories)
    .slice()
    .sort((left, right) => bookmarkSortKey(right) - bookmarkSortKey(left))
    .slice(0, limit);

export const getUsefulForgottenBookmarks = (memories: Memory[], limit = 5) => {
  const cutoff = Date.now() - 7 * DAY_MS;

  return getNonDuplicateBookmarks(memories)
    .filter((memory) => (memory.bookmarkQualityScore ?? 0) >= 46)
    .filter((memory) => new Date(memory.updatedAt || memory.createdAt).getTime() <= cutoff)
    .slice()
    .sort((left, right) => {
      const qualityDelta =
        (right.bookmarkQualityScore ?? 0) - (left.bookmarkQualityScore ?? 0);
      if (qualityDelta !== 0) return qualityDelta;
      return new Date(left.updatedAt || left.createdAt).getTime() -
        new Date(right.updatedAt || right.createdAt).getTime();
    })
    .slice(0, limit);
};

export interface BookmarkDomainInsight {
  domain: string;
  count: number;
  averageQuality: number;
  latestMemory: Memory;
}

export const getTopBookmarkDomains = (
  memories: Memory[],
  limit = 5,
): BookmarkDomainInsight[] => {
  const byDomain = new Map<string, Memory[]>();

  for (const memory of getNonDuplicateBookmarks(memories)) {
    const domain = memory.resolvedDomain ?? memory.domain;
    if (!domain) continue;
    const bucket = byDomain.get(domain) ?? [];
    bucket.push(memory);
    byDomain.set(domain, bucket);
  }

  return Array.from(byDomain.entries())
    .map(([domain, bucket]) => ({
      domain,
      count: bucket.length,
      averageQuality:
        bucket.reduce((sum, memory) => sum + (memory.bookmarkQualityScore ?? 0), 0) /
        bucket.length,
      latestMemory: bucket
        .slice()
        .sort((left, right) => bookmarkSortKey(right) - bookmarkSortKey(left))[0],
    }))
    .sort((left, right) => {
      if (right.count !== left.count) return right.count - left.count;
      return right.averageQuality - left.averageQuality;
    })
    .slice(0, limit);
};

export const getBookmarksRelatedToActiveProject = (
  memories: Memory[],
  projects: Project[],
  activeProjectId: string | "all",
  limit = 4,
) => {
  if (activeProjectId === "all") return [];

  const project = projects.find((candidate) => candidate.id === activeProjectId);
  if (!project) return [];

  const projectTokens = Array.from(
    new Set([
      ...tokenize(project.name),
      ...tokenize(project.description),
    ]),
  );
  if (projectTokens.length === 0) return [];

  return getNonDuplicateBookmarks(memories)
    .map((memory) => {
      const titleTokens = new Set(
        tokenize([memory.title, memory.resolvedTitle].filter(Boolean).join(" ")),
      );
      const descriptionTokens = new Set(tokenize(memory.resolvedDescription));
      const topicTokens = new Set(tokenize((memory.topicLabels ?? []).join(" ")));
      const folderTokens = new Set(tokenize(memory.bookmarkFolderPath));
      const domainTokens = new Set(tokenize(memory.resolvedDomain));

      const strongMatches = projectTokens.filter(
        (token) =>
          titleTokens.has(token) || descriptionTokens.has(token) || topicTokens.has(token),
      ).length;
      const contextualMatches = projectTokens.filter(
        (token) => folderTokens.has(token) || domainTokens.has(token),
      ).length;
      const relationScore = strongMatches * 2 + contextualMatches;

      return { memory, relationScore, strongMatches };
    })
    .filter((candidate) => candidate.strongMatches > 0 || candidate.relationScore >= 3)
    .sort((left, right) => {
      if (right.relationScore !== left.relationScore) {
        return right.relationScore - left.relationScore;
      }
      return (right.memory.bookmarkQualityScore ?? 0) - (left.memory.bookmarkQualityScore ?? 0);
    })
    .slice(0, limit)
    .map((candidate) => candidate.memory);
};
