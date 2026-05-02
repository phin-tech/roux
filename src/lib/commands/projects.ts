import { get } from "svelte/store";
import { registry } from "./registry";
import { projects, removeProject } from "$lib/stores/projects";
import { spawnBlueprintForProject } from "$lib/sessions/spawnBlueprint";
import {
  openNewProjectDialog,
  openEditProjectDialog,
} from "$lib/stores/newProjectDialog";

export function registerProjectCommands(): void {
  // App.svelte intercepts this id and opens the dialog in create mode.
  registry.register({
    id: "project.new",
    label: "New Project",
    category: "Projects",
    execute: () => openNewProjectDialog(),
  });

  registry.register({
    id: "project.edit",
    label: "Edit Project",
    category: "Projects",
    available: () => get(projects).length > 0,
    inputPlaceholder: "Pick a project to edit...",
    getItems: () =>
      get(projects).map((p) => ({
        id: p.id,
        label: p.name,
        description: `${p.repoRoots?.length ?? 0} repo(s) · ${
          p.sessionBlueprints?.length ?? 0
        } session(s)`,
        action: () => openEditProjectDialog(p),
      })),
  });

  registry.register({
    id: "project.delete",
    label: "Delete Project",
    category: "Projects",
    available: () => get(projects).length > 0,
    inputPlaceholder: "Pick a project to delete...",
    getItems: () =>
      get(projects).map((p) => ({
        id: p.id,
        label: p.name,
        description: "Removes the project (sessions stay, just untagged)",
        action: async () => {
          await removeProject(p.id);
        },
      })),
  });

  registry.register({
    id: "project.spawn-blueprint",
    label: "Spawn Project Session",
    category: "Projects",
    available: () =>
      get(projects).some((p) => (p.sessionBlueprints?.length ?? 0) > 0),
    inputPlaceholder: "Pick a project, then a session blueprint...",
    getItems: () =>
      get(projects)
        .filter((p) => (p.sessionBlueprints?.length ?? 0) > 0)
        .flatMap((p) =>
          (p.sessionBlueprints ?? []).map((bp) => ({
            id: `${p.id}::${bp.id}`,
            label: `${p.name} · ${bp.name}`,
            description: bp.branch ? `branch ${bp.branch}` : bp.repoRoot,
            action: () => {
              void spawnBlueprintForProject(p, bp).catch((error) => {
                console.error("Failed to spawn project blueprint session", {
                  projectId: p.id,
                  blueprintId: bp.id,
                  error,
                });
              });
            },
          })),
        ),
  });
}
