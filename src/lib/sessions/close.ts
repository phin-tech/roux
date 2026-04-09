import { get } from "svelte/store";
import { removeSession } from "$lib/stores/sessions";
import { closeSessionPanes } from "$lib/panes/actions";
import { killSession, removeWorktree } from "$lib/tauri";
import { settings } from "$lib/stores/settings";
import type { Session } from "$lib/types";

export async function closeSession(session: Session, opts?: { force?: boolean }): Promise<boolean> {
  const s = get(settings);
  const force = opts?.force ?? false;

  if (
    !force &&
    s.confirmOnClose &&
    (session.status === "thinking" || session.status === "generating")
  ) {
    const confirmed = window.confirm(
      `"${session.name}" is currently ${session.status}. Close it?`
    );
    if (!confirmed) return false;
  }

  // closeSessionPanes disposes all instances (terminals, listeners) and removes the layout
  closeSessionPanes(session.id);
  await killSession(session.id);

  if (session.isWorktree) {
    if (s.cleanupWorktreesOnClose) {
      await removeWorktree(session.worktreePath).catch(() => {});
    } else if (!force) {
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
