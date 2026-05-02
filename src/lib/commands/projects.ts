import { get } from "svelte/store";
import { registry } from "./registry";
import { projects, removeProject } from "$lib/stores/projects";
import { addSession } from "$lib/stores/sessions";
import { initSessionWithProfile } from "$lib/panes/actions";
import { createSessionShell, setSessionProject as tauriSetSessionProject } from "$lib/tauri";
import type { Project, SessionBlueprint } from "$lib/types";
import {
  openNewProjectDialog,
  openEditProjectDialog,
} from "$lib/stores/newProjectDialog";
import type { SpawnProfileRef } from "$lib/panes/profiles";

async function spawnBlueprint(project: Project, bp: SessionBlueprint): Promise<void> {
  const { resolveProfileRef } = await import("$lib/panes/profiles");
  const { runProfileInPane } = await import("$lib/panes/profileRunner");
  const profileRef: SpawnProfileRef = { kind: "registered", id: bp.spawnProfile };
  const profile = resolveProfileRef(profileRef);
  const nonoProfile = bp.nonoProfile ?? profile?.nonoProfile ?? undefined;
  const nonoAllowDirs =
    bp.nonoAllowDirs && bp.nonoAllowDirs.length > 0
      ? bp.nonoAllowDirs
      : profile?.nonoAllowDirs ?? undefined;

  const newSession = await createSessionShell(
    bp.repoRoot,
    bp.name,
    bp.worktreePath ?? null,
    bp.branch ?? null,
    {
      nonoProfile,
      nonoAllowDirs,
      profile: bp.spawnProfile,
      base: bp.base ?? null,
      fetchFirst: bp.fetchFirst ?? false,
      projectId: project.id,
      blueprintId: bp.id,
    },
  );
  addSession(newSession);
  // Defensive: backend should already have stamped project_id (we passed it
  // through CreateShellOpts), but the frontend store mirror may have an
  // older snapshot if it raced. Re-issuing set_session_project is idempotent.
  // Best-effort: don't let a failure here abort the rest of session init.
  try {
    await tauriSetSessionProject(newSession.id, project.id);
  } catch (error) {
    console.warn("Failed to defensively sync session project", {
      sessionId: newSession.id,
      projectId: project.id,
      error,
    });
  }

  const mainPaneId = initSessionWithProfile(newSession.id, profileRef, {
    nonoProfile,
    nonoAllowDirs,
  });
  const { connectPaneTerminal } = await import("$lib/panes/terminals");
  await connectPaneTerminal(mainPaneId);
  if (profile) {
    await runProfileInPane(newSession.id, profile, {
      appendSystemPrompt: project.projectPrompt ?? "",
    });
  }
}

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
              void spawnBlueprint(p, bp).catch((error) => {
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
