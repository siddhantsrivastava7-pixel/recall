import { create } from "zustand";
import type { Project } from "@/domain/types";
import { tauriClient } from "@/services/api/tauri-client";

interface ProjectStoreState {
  projects: Project[];
  activeProjectId: string | "all";
  hydrate: (projects: Project[]) => void;
  setActiveProject: (projectId: string | "all") => void;
  create: (name: string, description: string | null) => Promise<{ ok: boolean; error?: string }>;
  update: (id: string, name: string, description: string | null) => Promise<{ ok: boolean; error?: string }>;
  remove: (id: string) => Promise<{ ok: boolean; error?: string }>;
}

export const useProjectStore = create<ProjectStoreState>((set) => ({
  projects: [],
  activeProjectId: "all",

  hydrate(projects) { set({ projects }); },
  setActiveProject(activeProjectId) { set({ activeProjectId }); },

  async create(name, description) {
    try {
      const project = await tauriClient.createProject(name, description);
      set(state => ({ projects: [project, ...state.projects] }));
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Failed to create." };
    }
  },

  async update(id, name, description) {
    try {
      const project = await tauriClient.updateProject(id, name, description);
      set(state => ({ projects: state.projects.map(p => p.id === id ? project : p) }));
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Failed to update." };
    }
  },

  async remove(id) {
    try {
      await tauriClient.deleteProject(id);
      set(state => ({
        projects: state.projects.filter(p => p.id !== id),
        activeProjectId: state.activeProjectId === id ? "all" : state.activeProjectId,
      }));
      return { ok: true };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "Failed to delete." };
    }
  },
}));
