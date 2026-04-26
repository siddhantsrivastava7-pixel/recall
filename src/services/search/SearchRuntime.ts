import type { Memory, Project, SearchQuery, SearchResult } from "@/domain/types";
import type { SearchProvider } from "@/services/search/SearchProvider";
import type { SemanticSearchProvider } from "@/services/search/SemanticSearchProvider";

interface SearchRuntimeOptions {
  keywordProvider: SearchProvider;
  semanticProvider?: SemanticSearchProvider | null;
}

export class SearchRuntime {
  constructor(private readonly options: SearchRuntimeOptions) {}

  search(memories: Memory[], projects: Project[], query: SearchQuery): SearchResult[] {
    return this.options.keywordProvider.search({ memories, projects, query });
  }

  get semanticProvider() {
    return this.options.semanticProvider ?? null;
  }
}
