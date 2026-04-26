import type { Memory, Project, SearchQuery, SearchResult } from "@/domain/types";
import type { EmbeddingGenerator } from "@/services/search/EmbeddingGenerator";
import type { VectorIndex } from "@/services/search/VectorIndex";

export interface SemanticSearchContext {
  memories: Memory[];
  projects: Project[];
  query: SearchQuery;
  embeddingGenerator: EmbeddingGenerator;
  vectorIndex: VectorIndex;
}

export interface SemanticSearchProvider {
  id: "semantic";
  label: string;
  search(context: SemanticSearchContext): Promise<SearchResult[]>;
}
