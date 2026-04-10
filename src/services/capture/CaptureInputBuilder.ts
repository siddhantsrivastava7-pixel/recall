import type { AppContextSnapshot, MemoryInput } from "@/domain/types";

export interface QuickCaptureDraftInput {
  title: string;
  content: string;
  note: string;
  projectId: string;
}

const toOptional = (value: string) => {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

export const buildQuickCaptureInput = (
  draft: QuickCaptureDraftInput,
  context?: AppContextSnapshot | null,
): MemoryInput => ({
  sourceType: "manual",
  title: toOptional(draft.title),
  content: draft.content,
  note: toOptional(draft.note),
  projectId: toOptional(draft.projectId),
  sourceApp: context?.sourceApp ?? null,
  sourceWindow: context?.sourceWindow ?? null,
});
