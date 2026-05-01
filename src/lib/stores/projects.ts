import { writable, get } from "svelte/store";
import type { Project, ProjectUpdate } from "../types";
import {
  listProjects as tauriListProjects,
  createProject as tauriCreateProject,
  removeProject as tauriRemoveProject,
  renameProject as tauriRenameProject,
  updateProject as tauriUpdateProject,
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

/**
 * Create a project then immediately apply the rich fields (repo roots,
 * context paths, session blueprints) from the new-project dialog. Returns
 * the fully-populated Project after the patch round-trips.
 */
export async function createProjectFull(
  name: string,
  patch: Omit<ProjectUpdate, "name">,
): Promise<Project> {
  const created = await tauriCreateProject(name);
  const hasPatch =
    patch.repoRoots !== undefined ||
    patch.contextPaths !== undefined ||
    patch.sessionBlueprints !== undefined ||
    patch.projectPrompt !== undefined;
  const final = hasPatch ? await tauriUpdateProject(created.id, patch) : created;
  projects.update((ps) => [...ps, final]);
  return final;
}

export async function updateProject(
  id: string,
  patch: ProjectUpdate,
): Promise<Project> {
  const updated = await tauriUpdateProject(id, patch);
  projects.update((ps) => ps.map((p) => (p.id === id ? updated : p)));
  return updated;
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

/** Synchronously look up a project by id from the store snapshot.
 *  Returns null when no project matches — most callers want to no-op
 *  rather than block PTY spawn on a missing project record. */
export function getProjectById(id: string | null | undefined): Project | null {
  if (!id) return null;
  return get(projects).find((p) => p.id === id) ?? null;
}

/** Convenience accessor for the spawn-time project prompt. Returns ""
 *  when the project has no prompt or doesn't exist, so callers can
 *  pass the value straight through to `runProfileInPane`. */
export function getProjectPrompt(id: string | null | undefined): string {
  return getProjectById(id)?.projectPrompt ?? "";
}
