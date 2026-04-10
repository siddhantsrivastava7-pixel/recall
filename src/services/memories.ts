import type { MemoryInput, ServiceResult } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";

export const createMemory = async (input: MemoryInput) => {
  try {
    const memory = await tauriClient.createMemory(input);
    return { ok: true, data: memory } satisfies ServiceResult<typeof memory>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to save memory.",
    } satisfies ServiceResult<never>;
  }
};

export const updateMemory = async (id: string, input: MemoryInput) => {
  try {
    const memory = await tauriClient.updateMemory(id, input);
    return { ok: true, data: memory } satisfies ServiceResult<typeof memory>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to update memory.",
    } satisfies ServiceResult<never>;
  }
};

export const deleteMemory = async (id: string) => {
  try {
    await tauriClient.deleteMemory(id);
    return { ok: true } satisfies ServiceResult<void>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to delete memory.",
    } satisfies ServiceResult<void>;
  }
};

export const duplicateMemory = async (id: string) => {
  try {
    const memory = await tauriClient.duplicateMemory(id);
    return { ok: true, data: memory } satisfies ServiceResult<typeof memory>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to duplicate memory.",
    } satisfies ServiceResult<never>;
  }
};
