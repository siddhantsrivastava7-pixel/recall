import type { Memory, Project, SearchQuery } from "@/domain/types";
import { KeywordSearchProvider } from "@/services/search/KeywordSearchProvider";
import { SearchRuntime } from "@/services/search/SearchRuntime";

export const recallSearchRuntime = new SearchRuntime({
  keywordProvider: new KeywordSearchProvider(),
});

export const searchMemories = (
  memories: Memory[],
  projects: Project[],
  query: SearchQuery,
) => recallSearchRuntime.search(memories, projects, query);
