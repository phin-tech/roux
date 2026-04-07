import { writable } from "svelte/store";
import type { Project } from "../types";
import {
  listProjects as tauriListProjects,
  createProject as tauriCreateProject,
  removeProject as tauriRemoveProject,
  renameProject as tauriRenameProject,
} from "../tauri";

export const projects = writable<Project[]>([]);

export async function loadProjects(): Promise<void> {
  const list = await tauriListProjects();
  projects.set(list);
}

export async function createProject(name: string): Promise<Project> {
  const project = await tauriCreateProject(name);
  projects.update((ps) => [...ps, project]);
  return project;
}

export async function removeProject(id: string): Promise<void> {
  await tauriRemoveProject(id);
  projects.update((ps) => ps.filter((p) => p.id !== id));
}

export async function renameProject(id: string, name: string): Promise<void> {
  await tauriRenameProject(id, name);
  projects.update((ps) =>
    ps.map((p) => (p.id === id ? { ...p, name } : p))
  );
}
