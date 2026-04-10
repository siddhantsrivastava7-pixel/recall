import type { Memory, Project, SearchQuery, SearchResult } from "@/domain/types";

export interface SearchProvider {
  id: "keyword";
  search(args: {
    memories: Memory[];
    projects: Project[];
    query: SearchQuery;
  }): SearchResult[];
}
