import { get } from "svelte/store";
import { removeSession } from "$lib/stores/sessions";
import { closeSessionPanes } from "$lib/panes/actions";
import { killSession, removeWorktree } from "$lib/tauri";
import { settings } from "$lib/stores/settings";
import {
  sessionAgentStatus,
  computeEffectiveSessionStatus,
} from "$lib/panes/agentState";
import type { Session } from "$lib/types";

export async function closeSession(session: Session, opts?: { force?: boolean }): Promise<boolean> {
  const s = get(settings);
  const force = opts?.force ?? false;

  // Use the unified effective status so that a session whose secondary
  // pane is actively generating still trips the confirm prompt even if
  // the legacy Session.status field is stale.
  const effective = computeEffectiveSessionStatus(
    session.status,
    get(sessionAgentStatus).get(session.id) ?? null,
  );

  if (
    !force &&
    s.confirmOnClose &&
    (effective === "thinking" || effective === "generating")
  ) {
    const confirmed = window.confirm(
      `"${session.name}" is currently ${effective}. Close it?`,
    );
    if (!confirmed) return false;
  }

  // closeSessionPanes disposes all instances (terminals, listeners) and removes the layout
  closeSessionPanes(session.id);
  await killSession(session.id);

  if (session.isWorktree) {
    // Prefer the new three-state enum; fall back to the legacy boolean for
    // settings files written before the migration ran.
    const mode =
      s.worktreeCleanupOnClose ?? (s.cleanupWorktreesOnClose ? "always" : "prompt");
    if (mode === "always") {
      await removeWorktree(session.worktreePath).catch(() => {});
    } else if (mode === "prompt" && !force) {
      const remove = window.confirm(
        `Also remove the worktree at ${session.worktreePath}?`
      );
      if (remove) {
        await removeWorktree(session.worktreePath).catch(() => {});
      }
    }
  }

  removeSession(session.id);
  return true;
}
