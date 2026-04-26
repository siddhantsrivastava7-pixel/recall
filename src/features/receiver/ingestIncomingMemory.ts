import type { IncomingMobilePushPayload } from "@/features/receiver/validateIncomingMemory";
import { validateIncomingMemoryPayload } from "@/features/receiver/validateIncomingMemory";

export const buildMobilePushRequest = (
  payload: unknown,
): { ok: true; body: IncomingMobilePushPayload } | { ok: false; error: string } => {
  const validation = validateIncomingMemoryPayload(payload);
  if (!validation.ok) return validation;
  return { ok: true, body: validation.value };
};
