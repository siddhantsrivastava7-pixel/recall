import type { Project, ServiceResult } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";

export const createProject = async (name: string, description: string | null) => {
  try {
    const project = await tauriClient.createProject(name, description);
    return { ok: true, data: project } satisfies ServiceResult<Project>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to create project.",
    } satisfies ServiceResult<Project>;
  }
};

export const updateProject = async (
  id: string,
  name: string,
  description: string | null,
) => {
  try {
    const project = await tauriClient.updateProject(id, name, description);
    return { ok: true, data: project } satisfies ServiceResult<Project>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to update project.",
    } satisfies ServiceResult<Project>;
  }
};

export const deleteProject = async (id: string) => {
  try {
    await tauriClient.deleteProject(id);
    return { ok: true } satisfies ServiceResult<void>;
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : "Unable to delete project.",
    } satisfies ServiceResult<void>;
  }
};
