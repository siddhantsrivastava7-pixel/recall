export interface EmbeddingDocument {
  id: string;
  memoryId: string;
  text: string;
  metadata?: Record<string, string | number | boolean | null>;
}

export interface EmbeddingVector {
  id: string;
  memoryId: string;
  values: number[];
  dimensions: number;
  model: string;
}

export interface EmbeddingGenerator {
  id: string;
  generate(documents: EmbeddingDocument[]): Promise<EmbeddingVector[]>;
}
