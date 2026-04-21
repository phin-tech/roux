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
  archivedSessionsState.update((s) => ({
    ...s,
    sessions: s.sessions.filter((sess) => sess.id !== id),
  }));
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
export async function cleanArchivedWorktree(id: string, worktreePath: string): Promise<void> {
  await removeWorktree(worktreePath);
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

/**
 * Called when a previously-active session just moved to the archived list.
 * If the pane has already hydrated, we push the newly archived session in
 * locally so the user doesn't have to close and re-open the pane to see
 * it. If the pane hasn't loaded yet, we skip — the next
 * `loadArchivedSessions` call will pick it up.
 */
export function addArchivedSessionFromEvent(session: Session): void {
  const current = get(archivedSessionsState);
  if (!current.loaded) return;
  if (current.sessions.some((s) => s.id === session.id)) return;
  const worktreeExists = new Map(current.worktreeExists);
  worktreeExists.set(session.id, true);
  archivedSessionsState.update((s) => ({
    ...s,
    sessions: [session, ...s.sessions],
    worktreeExists,
  }));
}
