import { get } from "svelte/store";
import { removeSession } from "$lib/stores/sessions";
import { removeSessionPanes } from "$lib/stores/panes";
import { killSession, removeWorktree } from "$lib/tauri";
import { closeAuxiliaryPanes } from "$lib/panes/actions";
import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
import { settings } from "$lib/stores/settings";
import type { Session } from "$lib/types";

export async function closeSession(session: Session): Promise<boolean> {
  const s = get(settings);

  if (
    s.confirmOnClose &&
    (session.status === "thinking" || session.status === "generating")
  ) {
    const confirmed = window.confirm(
      `"${session.name}" is currently ${session.status}. Close it?`
    );
    if (!confirmed) return false;
  }

  await closeAuxiliaryPanes(session.id);
  await disposeClaudeTerminal(session.id);
  await killSession(session.id);

  if (session.isWorktree) {
    if (s.cleanupWorktreesOnClose) {
      await removeWorktree(session.worktreePath).catch(() => {});
    } else {
      const remove = window.confirm(
        `Also remove the worktree at ${session.worktreePath}?`
      );
      if (remove) {
        await removeWorktree(session.worktreePath).catch(() => {});
      }
    }
  }

  removeSessionPanes(session.id);
  removeSession(session.id);
  return true;
}
