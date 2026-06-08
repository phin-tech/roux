import { get } from "svelte/store";
import {
  sessionState,
  addSession,
  setActiveSession,
  updateSessionStatus,
} from "$lib/stores/sessions";
import { listSessions, listAllPtys } from "$lib/tauri";
import { restoreArchivedSession } from "$lib/stores/archivedSessions";
import { loadPaneState } from "./persistence";
import { restoreSessionPanes } from "./restore";
import { collectLeafIds, sessionLayouts } from "./layout";
import { getAttachedPtyId, getInstance } from "./instances";
import { log } from "$lib/logging";

export type OpenSessionResult = "focused" | "opened" | "gone";

export interface OpenSessionOptions {
  ptyId?: string | null;
}

function hasAttachedSessionPane(sessionId: string, ptyId = sessionId): boolean {
  const layout = get(sessionLayouts).get(sessionId);
  if (!layout) return false;
  return collectLeafIds(layout).some((paneId) => {
    const pane = getInstance(paneId);
    return pane ? getAttachedPtyId(pane) === ptyId : false;
  });
}

/**
 * Open (render + focus) a session that may not yet be registered in the
 * desktop.
 *
 * Used by the board's "Open terminal" action. A daemon-dispatched session runs
 * headless in the daemon's PTY manager and never enters the desktop
 * `sessionList` (there is no session-events bridge), so the desktop has to
 * look it up and attach on demand. This reattaches to the already-running PTY
 * via the same restore path used on launch — no respawn, no profile replay.
 *
 * - "focused": already registered locally; just made active.
 * - "opened":  looked up from the backend, attached, and made active.
 * - "gone":    the session no longer exists (its PTY/session was closed).
 */
export async function openSessionById(
  sessionId: string,
  options: OpenSessionOptions = {},
): Promise<OpenSessionResult> {
  const existing =
    get(sessionState).sessions.find((s) => s.id === sessionId) ?? null;
  const initialPtyId = options.ptyId || existing?.primaryPtyId || sessionId;
  if (
    existing &&
    existing.status !== "disconnected" &&
    hasAttachedSessionPane(sessionId, initialPtyId)
  ) {
    setActiveSession(sessionId);
    return "focused";
  }

  const sessions = await listSessions();
  let session = sessions.find((s) => s.id === sessionId) ?? null;
  let restoredArchived = false;

  // The session may be absent from the active list because it was
  // archived.  Try to revive it.  This also covers the case where a
  // stale local copy in `existing` outlives the backend record.
  if (!session) {
    try {
      await restoreArchivedSession(sessionId);
      const restoredSessions = await listSessions();
      session = restoredSessions.find((s) => s.id === sessionId) ?? null;
      if (session) restoredArchived = true;
    } catch (e) {
      log(`openSessionById(${sessionId}): archived restore failed: ${e}`);
    }
  }

  // Fall back to the local store copy when backend lookup + archive
  // restore both come up empty (e.g. the session was registered
  // client-side but hasn't been persisted yet).
  if (!session) {
    session = existing ?? null;
  }

  if (!session) {
    return "gone";
  }

  const primaryPtyId = options.ptyId || session.primaryPtyId || session.id;

  // Reconnect the session *before* adding it to the store so the UI
  // never renders it in a disconnected state (which would show the
  // SessionPicker instead of the live terminal).
  if (restoredArchived) {
    const { reattachSession } = await import("$lib/sessions/reconnect");
    session = await reattachSession(session);
    addSession(session);
    setActiveSession(session.id);
    return "opened";
  }

  // xterm-heavy modules are imported lazily, mirroring the launch restore path.
  const [{ initTerminal, attachPtyListeners }, { attachPtyToPane }] =
    await Promise.all([import("./terminals"), import("./attach")]);

  let livePtyIds: Set<string> | null = null;
  try {
    livePtyIds = new Set((await listAllPtys()).map((pty) => pty.id));
  } catch (e) {
    livePtyIds = null;
    log(
      `openSessionById(${sessionId}): unable to read live PTY inventory: ${e}`,
    );
  }

  const persisted = await loadPaneState(session.id);
  await restoreSessionPanes(session, persisted, {
    initTerminal,
    attachPtyListeners,
    attachLivePtyToPane: attachPtyToPane,
    livePtyIds,
    primaryPtyId,
  });

  const attached = hasAttachedSessionPane(session.id, primaryPtyId);
  if (!attached) {
    const { continueSessionShell } = await import("$lib/sessions/reconnect");
    try {
      session = await continueSessionShell(session);
    } catch (e) {
      log(
        `openSessionById(${sessionId}): reconnect failed, session stays disconnected: ${e}`,
      );
    }
  }

  // Add the session to the store *after* reconnect so the initial render
  // sees a live / idle session, never a disconnected one.
  addSession(session);
  if (attached) {
    updateSessionStatus(session.id, "idle");
  }
  setActiveSession(session.id);
  return "opened";
}
