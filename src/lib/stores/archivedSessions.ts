import { writable, get } from "svelte/store";
import type { Session } from "$lib/types";
import {
  listArchivedSessions,
  listSessions,
  restoreSession as restoreSessionCmd,
  deleteSessionPermanently,
  sessionWorktreeExists,
  removeWorktree,
} from "$lib/tauri";
import { sessionState } from "$lib/stores/sessions";

interface ArchivedState {
  sessions: Session[];
  loaded: boolean;
  worktreeExists: Map<string, boolean>;
}

const initial: ArchivedState = {
  sessions: [],
  loaded: false,
  worktreeExists: new Map(),
};

export const archivedSessionsState = writable<ArchivedState>(initial);

/**
 * Hydrate from the backend. Called by the Sessions Pane on open and after
 * any mutation that affects the archived list. Also batches a
 * `sessionWorktreeExists` lookup per row so restore buttons can render
 * their disabled state without further round-trips.
 */
export async function loadArchivedSessions(): Promise<void> {
  const sessions = await listArchivedSessions();
  const worktreeExists = new Map<string, boolean>();
  await Promise.all(
    sessions.map(async (s) => {
      const exists = await sessionWorktreeExists(s.id).catch(() => false);
      worktreeExists.set(s.id, exists);
    }),
  );
  archivedSessionsState.set({ sessions, loaded: true, worktreeExists });
}

export async function restoreArchivedSession(id: string): Promise<void> {
  await restoreSessionCmd(id);
  archivedSessionsState.update((s) => {
    const worktreeExists = new Map(s.worktreeExists);
    worktreeExists.delete(id);
    return {
      ...s,
      sessions: s.sessions.filter((sess) => sess.id !== id),
      worktreeExists,
    };
  });
  // Re-hydrate the active store so the restored session shows up in the
  // session switcher / Active group without requiring an app restart.
  const sessions = await listSessions();
  sessionState.update((state) => ({ ...state, sessions }));
}

/**
 * Delete the worktree on disk for an archived session, keeping the record.
 * Restore becomes unavailable afterward (the worktree path no longer
 * exists); Delete forever still works. Use this to reclaim disk space
 * without losing the session history entry.
 */
export async function cleanArchivedWorktree(
  id: string,
  repoPath: string,
  worktreePath: string,
): Promise<void> {
  await removeWorktree(repoPath, worktreePath);
  archivedSessionsState.update((s) => {
    const worktreeExists = new Map(s.worktreeExists);
    worktreeExists.set(id, false);
    return { ...s, worktreeExists };
  });
}

export async function removeArchivedSessionForever(id: string): Promise<void> {
  await deleteSessionPermanently(id);
  archivedSessionsState.update((s) => {
    const worktreeExists = new Map(s.worktreeExists);
    worktreeExists.delete(id);
    return {
      ...s,
      sessions: s.sessions.filter((sess) => sess.id !== id),
      worktreeExists,
    };
  });
}

export interface BulkActionFailure {
  id: string;
  error: string;
}

export interface BulkActionResult {
  succeeded: string[];
  failures: BulkActionFailure[];
}

// String(err) renders non-Error rejections as "[object Object]" — preserve
// Error.message and JSON-encode plain objects so the bulk-error banner stays
// useful regardless of where the rejection came from.
function formatErr(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  try {
    return JSON.stringify(err) ?? String(err);
  } catch {
    return String(err);
  }
}

/**
 * Run a per-item async operation across many items, collecting failures
 * instead of bailing on the first error. Sequential (not Promise.all) so a
 * later failure doesn't abandon a partially-completed earlier op, and so
 * the user-visible store stays consistent with each step.
 */
async function runBulk<T>(
  items: readonly T[],
  idOf: (item: T) => string,
  op: (item: T) => Promise<void>,
): Promise<BulkActionResult> {
  const succeeded: string[] = [];
  const failures: BulkActionFailure[] = [];
  for (const item of items) {
    const id = idOf(item);
    try {
      await op(item);
      succeeded.push(id);
    } catch (err) {
      failures.push({ id, error: formatErr(err) });
    }
  }
  return { succeeded, failures };
}

export async function bulkRestoreArchivedSessions(
  ids: readonly string[],
): Promise<BulkActionResult> {
  const result = await runBulk(ids, (id) => id, (id) => restoreSessionCmd(id));
  if (result.succeeded.length > 0) {
    const succeededSet = new Set(result.succeeded);
    archivedSessionsState.update((s) => {
      const worktreeExists = new Map(s.worktreeExists);
      for (const id of succeededSet) worktreeExists.delete(id);
      return {
        ...s,
        sessions: s.sessions.filter((sess) => !succeededSet.has(sess.id)),
        worktreeExists,
      };
    });
    const sessions = await listSessions();
    sessionState.update((state) => ({ ...state, sessions }));
  }
  return result;
}

export async function bulkRemoveArchivedWorktrees(
  entries: readonly { id: string; repoRoot: string; worktreePath: string }[],
): Promise<BulkActionResult> {
  const result = await runBulk(
    entries,
    (e) => e.id,
    (e) => removeWorktree(e.repoRoot, e.worktreePath),
  );
  if (result.succeeded.length > 0) {
    archivedSessionsState.update((s) => {
      const worktreeExists = new Map(s.worktreeExists);
      for (const id of result.succeeded) worktreeExists.set(id, false);
      return { ...s, worktreeExists };
    });
  }
  return result;
}

export async function bulkDeleteArchivedSessionsForever(
  ids: readonly string[],
): Promise<BulkActionResult> {
  const result = await runBulk(
    ids,
    (id) => id,
    (id) => deleteSessionPermanently(id),
  );
  if (result.succeeded.length > 0) {
    const succeededSet = new Set(result.succeeded);
    archivedSessionsState.update((s) => {
      const worktreeExists = new Map(s.worktreeExists);
      for (const id of succeededSet) worktreeExists.delete(id);
      return {
        ...s,
        sessions: s.sessions.filter((sess) => !succeededSet.has(sess.id)),
        worktreeExists,
      };
    });
  }
  return result;
}

/**
 * Called when a previously-active session just moved to the archived list.
 * If the pane has already hydrated, we push the newly archived session in
 * locally so the user doesn't have to close and re-open the pane to see
 * it. If the pane hasn't loaded yet, we skip — the next
 * `loadArchivedSessions` call will pick it up.
 *
 * `worktreeExists` lets the caller explicitly report whether the worktree
 * is still on disk — it's only implicitly `true` when a session has just
 * been archived without worktree cleanup. Pass `false` when the close
 * flow removed the worktree (e.g. `worktreeCleanupOnClose === "always"`)
 * so the History row doesn't show a stale "on disk" badge or a live
 * Restore button.
 */
export function addArchivedSessionFromEvent(
  session: Session,
  worktreeExists: boolean = true,
): void {
  const current = get(archivedSessionsState);
  if (!current.loaded) return;
  if (current.sessions.some((s) => s.id === session.id)) return;
  const nextWorktreeExists = new Map(current.worktreeExists);
  nextWorktreeExists.set(session.id, worktreeExists);
  archivedSessionsState.update((s) => ({
    ...s,
    sessions: [session, ...s.sessions],
    worktreeExists: nextWorktreeExists,
  }));
}

export function clearArchivedSessionsProject(projectId: string): void {
  archivedSessionsState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.projectId === projectId ? { ...s, projectId: null, blueprintId: null } : s
    ),
  }));
}
