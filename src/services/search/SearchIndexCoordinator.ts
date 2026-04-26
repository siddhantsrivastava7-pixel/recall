import type { Memory } from "@/domain/types";
import type { EmbeddingDocument, EmbeddingGenerator } from "@/services/search/EmbeddingGenerator";
import type { VectorIndex } from "@/services/search/VectorIndex";

interface SearchIndexCoordinatorDependencies {
  embeddingGenerator: EmbeddingGenerator;
  vectorIndex: VectorIndex;
}

export class SearchIndexCoordinator {
  constructor(private readonly dependencies: SearchIndexCoordinatorDependencies) {}

  async reindexMemories(memories: Memory[]) {
    const documents: EmbeddingDocument[] = memories.map((memory) => ({
      id: memory.id,
      memoryId: memory.id,
      text: [memory.title, memory.content, memory.note, memory.projectName]
        .filter(Boolean)
        .join("\n\n"),
      metadata: {
        projectId: memory.projectId,
        sourceApp: memory.sourceApp,
      },
    }));

    const vectors = await this.dependencies.embeddingGenerator.generate(documents);
    await this.dependencies.vectorIndex.upsert(
      vectors.map((vector) => ({
        id: vector.id,
        memoryId: vector.memoryId,
        values: vector.values,
      })),
    );
  }

  async removeMemories(memoryIds: string[]) {
    await this.dependencies.vectorIndex.deleteByMemoryIds(memoryIds);
  }
}
