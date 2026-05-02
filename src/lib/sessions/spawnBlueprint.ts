import type { Project, Session, SessionBlueprint } from "$lib/types";
import {
  createSessionShell,
  setSessionProject as tauriSetSessionProject,
} from "$lib/tauri";
import { addSession } from "$lib/stores/sessions";
import { initSessionWithProfile } from "$lib/panes/actions";
import type { SpawnProfileRef } from "$lib/panes/profiles";

export interface SpawnBlueprintOptions {
  /**
   * Blueprint id to record on the spawned session. Defaults to `bp.id`.
   * Pass `null` for spawns whose blueprint is NOT being persisted on the
   * project (the "spawn now, don't save as template" path) so the session
   * doesn't carry a dangling reference.
   */
  blueprintId?: string | null;
}

/**
 * Spawn a session from a blueprint, tagging it with the given project.
 * Centralizes the per-blueprint dance (resolve profile, create shell, attach
 * pane, run profile) so callers in NewProjectDialog, SessionTabs, and the
 * command palette all go through the same path.
 */
export async function spawnBlueprintForProject(
  project: Project,
  bp: SessionBlueprint,
  opts: SpawnBlueprintOptions = {},
): Promise<Session> {
  const { resolveProfileRef } = await import("$lib/panes/profiles");
  const { runProfileInPane } = await import("$lib/panes/profileRunner");
  const profileRef: SpawnProfileRef = { kind: "registered", id: bp.spawnProfile };
  const profile = resolveProfileRef(profileRef);
  const nonoProfile = bp.nonoProfile ?? profile?.nonoProfile ?? undefined;
  const nonoAllowDirs =
    bp.nonoAllowDirs && bp.nonoAllowDirs.length > 0
      ? bp.nonoAllowDirs
      : profile?.nonoAllowDirs ?? undefined;

  const blueprintId = opts.blueprintId === undefined ? bp.id : opts.blueprintId;

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
      blueprintId,
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
  return newSession;
}
