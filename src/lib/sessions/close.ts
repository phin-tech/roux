import { get } from "svelte/store";
import { removeSession } from "$lib/stores/sessions";
import { addArchivedSessionFromEvent } from "$lib/stores/archivedSessions";
import { closeSessionPanes, detachSessionPanes } from "$lib/panes/actions";
import { flushPaneState } from "$lib/panes/persistence";
import {
  killSession,
  removeWorktree,
  deleteSessionPermanently,
} from "$lib/tauri";
import { settings } from "$lib/stores/settings";
import {
  sessionAgentStatus,
  computeEffectiveSessionStatus,
} from "$lib/panes/agentState";
import { activePlanningRunByItem, workItems } from "$lib/stores/workItems";
import type { Session } from "$lib/types";

/**
 * How to close a session. With the sessions-history pane, "archive" is the
 * only interactive choice — the session's record is soft-deleted and its
 * worktree is kept on disk. Users who want the worktree gone use the
 * Clean worktree action from the History pane (safer: no ambiguity about
 * what's being destroyed since the session is already archived).
 *
 * `delete-forever` is available for programmatic callers only; it does
 * **not** touch the worktree either — use Clean worktree first, then
 * Delete forever from History, if both are desired.
 */
export type CloseAction = "archive" | "delete-forever";

interface CloseOpts {
  /** Skip the interactive confirm. Used for quit-flow, spawn-rollback, etc. */
  force?: boolean;
  /** Defaults to `archive`. `delete-forever` skips the confirm unconditionally. */
  action?: CloseAction;
  /** Work-item cards normally detach on close so their terminal can be reopened. */
  preserveWorkItemBoundSession?: boolean;
}

export async function closeSession(
  session: Session,
  opts?: CloseOpts,
): Promise<boolean> {
  const s = get(settings);
  const force = opts?.force ?? false;
  const action: CloseAction = opts?.action ?? "archive";
  const preserveWorkItemBoundSession =
    opts?.preserveWorkItemBoundSession ?? true;

  // Thinking/generating confirm — preserved from the old flow. Always the
  // same one-confirm prompt; no secondary destructive confirm stacked on
  // top (that was the bug that made "OK, OK" delete worktrees).
  const effective = computeEffectiveSessionStatus(
    session.status,
    get(sessionAgentStatus).get(session.id) ?? null,
  );
  if (
    !force &&
    action === "archive" &&
    s.confirmOnClose &&
    (effective === "thinking" || effective === "generating")
  ) {
    const confirmed = window.confirm(
      `"${session.name}" is currently ${effective}. Close it?`,
    );
    if (!confirmed) return false;
  }

  // Persist the live layout before disposing panes. Once closeSessionPanes()
  // runs, the layout and pane records are gone, so a later quit/debounce
  // flush has nothing left to serialize for restore.
  //
  // Scoped to this session id specifically: at launch, every restored
  // session has a transient primary-only layout in `sessionLayouts` until
  // the user clicks Continue. A blanket flush here would write that stub
  // over each session's rich persisted layout, losing their split panes.
  await flushPaneState(session.id);

  const isWorkItemBoundSession = get(workItems).some(
    (item) => item.sessionId === session.id,
  );
  const isPlanningRunSession = [...get(activePlanningRunByItem).values()].some(
    (run) => run.sessionId === session.id,
  );
  if (
    action === "archive" &&
    preserveWorkItemBoundSession &&
    (isWorkItemBoundSession || isPlanningRunSession)
  ) {
    detachSessionPanes(session.id);
    removeSession(session.id);
    return true;
  }

  // Dispose panes / terminals regardless of action.
  closeSessionPanes(session.id);

  if (action === "delete-forever") {
    await deleteSessionPermanently(session.id);
    removeSession(session.id);
    return true;
  }

  // Archive path: soft-delete the record, worktree stays on disk.
  await killSession(session.id);

  // Honor the legacy always-cleanup setting for users who explicitly
  // opted in. `prompt` no longer prompts — worktree removal is a
  // post-archive action from the History pane, not a close-time gotcha.
  // Track whether the worktree was actually removed so the History pane
  // reflects the right "on disk" / "gone" state without a re-hydrate.
  let worktreeStillOnDisk = session.isWorktree;
  if (session.isWorktree) {
    const mode =
      s.worktreeCleanupOnClose ??
      (s.cleanupWorktreesOnClose ? "always" : "prompt");
    if (mode === "always") {
      try {
        await removeWorktree(session.repoRoot, session.worktreePath);
        worktreeStillOnDisk = false;
      } catch {
        // If removal failed we still archive the session, but leave the
        // worktree flagged as on disk so the user can retry from the
        // History pane (Clean worktree).
      }
    }
  }

  // Remove from the active store and push into the archived store so an
  // already-open Sessions Pane reflects the new history row immediately.
  removeSession(session.id);
  const endedAt = Math.floor(Date.now() / 1000);
  addArchivedSessionFromEvent(
    {
      ...session,
      archived: true,
      endedAt,
      primaryPtyId: null,
      status: "disconnected",
    },
    worktreeStillOnDisk,
  );
  return true;
}
