import type { Memory, Project, SearchResult } from "@/domain/types";
import type { SearchProvider } from "@/services/search/SearchProvider";
import { scoreMemoryForKeywordQuery } from "@/services/search/keywordRanking";

export class KeywordSearchProvider implements SearchProvider {
  readonly id = "keyword" as const;

  search({
    memories,
    projects,
    query,
  }: {
    memories: Memory[];
    projects: Project[];
    query: { text: string; projectId?: string | null; limit?: number };
  }): SearchResult[] {
    const text = query.text.trim();
    if (!text) {
      return memories
        .filter((memory) => !query.projectId || memory.projectId === query.projectId)
        .slice()
        .sort(
          (a, b) =>
            new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime(),
        )
        .slice(0, query.limit ?? 8)
        .map((memory) => ({
          memory,
          score: 0,
          highlights: [],
          strategy: "keyword" as const,
          providerId: this.id,
        }));
    }

    return memories
      .filter((memory) => !query.projectId || memory.projectId === query.projectId)
      .map((memory) => {
        const result = scoreMemoryForKeywordQuery(memory, projects, text);
        return {
          memory,
          score: result.score,
          highlights: result.highlights,
          strategy: "keyword" as const,
          providerId: this.id,
        };
      })
      .filter((result) => result.score > 0)
      .sort((left, right) => {
        if (right.score !== left.score) {
          return right.score - left.score;
        }

        return (
          new Date(right.memory.updatedAt || right.memory.createdAt).getTime() -
          new Date(left.memory.updatedAt || left.memory.createdAt).getTime()
        );
      })
      .slice(0, query.limit ?? 24);
  }
}
