export interface IncomingMobileMemory {
  id: string;
  title?: string;
  content?: string;
  url?: string;
  source?: string;
  note?: string;
  memoryType?: string;
  createdAt: number;
  imageUri?: string;
  previewText?: string;
}

export interface IncomingMobilePushPayload {
  memory: IncomingMobileMemory;
}

export const validateIncomingMemoryPayload = (
  payload: unknown,
): { ok: true; value: IncomingMobilePushPayload } | { ok: false; error: string } => {
  if (!payload || typeof payload !== "object" || !("memory" in payload)) {
    return { ok: false, error: "Payload must include memory." };
  }

  const memory = (payload as { memory?: unknown }).memory;
  if (!memory || typeof memory !== "object") {
    return { ok: false, error: "memory must be an object." };
  }

  const candidate = memory as Partial<IncomingMobileMemory>;
  if (typeof candidate.id !== "string" || candidate.id.trim().length === 0) {
    return { ok: false, error: "memory.id is required." };
  }

  if (typeof candidate.createdAt !== "number" || candidate.createdAt <= 0) {
    return { ok: false, error: "memory.createdAt must be a positive number." };
  }

  const hasBody =
    isMeaningfulString(candidate.content) ||
    isMeaningfulString(candidate.url) ||
    isMeaningfulString(candidate.previewText);

  if (!hasBody) {
    return { ok: false, error: "memory must include content, url, or previewText." };
  }

  return {
    ok: true,
    value: {
      memory: {
        id: candidate.id,
        title: optionalString(candidate.title),
        content: optionalString(candidate.content),
        url: optionalString(candidate.url),
        source: optionalString(candidate.source),
        note: optionalString(candidate.note),
        memoryType: optionalString(candidate.memoryType),
        createdAt: candidate.createdAt,
        imageUri: optionalString(candidate.imageUri),
        previewText: optionalString(candidate.previewText),
      },
    },
  };
};

const isMeaningfulString = (value: unknown) =>
  typeof value === "string" && value.trim().length > 0;

const optionalString = (value: unknown) =>
  typeof value === "string" && value.trim().length > 0 ? value.trim() : undefined;
