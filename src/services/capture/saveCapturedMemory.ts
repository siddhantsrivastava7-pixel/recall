import type { MemoryInput } from "@/domain/types";
import type { Memory } from "@/domain/types";
import { createMemory } from "@/services/memories";
import {
  markCaptureFailure,
  markDbWriteComplete,
  startCaptureTrace,
  type CaptureLatencyThresholds,
  type CaptureTraceOrigin,
} from "@/services/capture/captureTelemetry";

interface SaveCapturedMemoryOptions {
  origin?: CaptureTraceOrigin;
  latencyThresholds?: Partial<CaptureLatencyThresholds>;
}

interface SaveCapturedMemoryResult {
  ok: boolean;
  error?: string;
  traceId: string;
}

interface SaveCapturedMemorySuccessResult extends SaveCapturedMemoryResult {
  ok: true;
  memory: Memory;
}

interface SaveCapturedMemoryFailureResult extends SaveCapturedMemoryResult {
  ok: false;
}

export const saveCapturedMemory = async (
  input: MemoryInput,
  options: SaveCapturedMemoryOptions = {},
): Promise<SaveCapturedMemorySuccessResult | SaveCapturedMemoryFailureResult> => {
  const traceId = startCaptureTrace({
    origin: options.origin ?? "manual",
    sourceType: input.sourceType ?? "manual",
    latencyThresholds: options.latencyThresholds,
  });

  if (!input.content.trim()) {
    const error = "Content is required.";
    markCaptureFailure(traceId, error);
    return { ok: false, error, traceId };
  }

  const result = await createMemory(input);
  if (!result.ok || !result.data) {
    const error = result.error ?? "Unable to save memory.";
    markCaptureFailure(traceId, error);
    return { ok: false, error, traceId };
  }

  const memory = result.data;
  markDbWriteComplete(traceId, memory.id);

  return {
    ok: true,
    memory,
    traceId,
  };
};
