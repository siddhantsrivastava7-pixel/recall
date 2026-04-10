export interface VectorIndexRecord {
  id: string;
  memoryId: string;
  values: number[];
  contentPreview?: string;
  metadata?: Record<string, string | number | boolean | null>;
}

export interface VectorSearchQuery {
  values: number[];
  limit: number;
  projectId?: string | null;
}

export interface VectorSearchMatch {
  recordId: string;
  memoryId: string;
  score: number;
  excerpt?: string;
}

export interface VectorIndex {
  id: string;
  dimensions: number;
  upsert(records: VectorIndexRecord[]): Promise<void>;
  search(query: VectorSearchQuery): Promise<VectorSearchMatch[]>;
  deleteByMemoryIds(memoryIds: string[]): Promise<void>;
  reset(): Promise<void>;
}
