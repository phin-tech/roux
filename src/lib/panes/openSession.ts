import { get } from "svelte/store";
import {
  sessionState,
  addSession,
  setActiveSession,
} from "$lib/stores/sessions";
import { listSessions, listAllPtys } from "$lib/tauri";
import { loadPaneState } from "./persistence";
import { restoreSessionPanes } from "./restore";
import { log } from "$lib/logging";

export type OpenSessionResult = "focused" | "opened" | "gone";

/**
 * Open (render + focus) a session that may not yet be registered in the
 * desktop.
 *
 * Used by the board's "Open terminal" action. A daemon-dispatched session runs
 * headless in the daemon's PTY manager and never enters the desktop
 * `sessionList` (there is no session-events bridge), so the desktop has to
 * look it up and attach on demand. This reattaches to the already-running PTY
 * via the same restore path used on launch — no respawn, no profile replay
 * (the primary PTY id equals the session id).
 *
 * - "focused": already registered locally; just made active.
 * - "opened":  looked up from the backend, attached, and made active.
 * - "gone":    the session no longer exists (its PTY/session was closed).
 */
export async function openSessionById(
  sessionId: string,
): Promise<OpenSessionResult> {
  if (get(sessionState).sessions.some((s) => s.id === sessionId)) {
    setActiveSession(sessionId);
    return "focused";
  }

  const sessions = await listSessions();
  const session = sessions.find((s) => s.id === sessionId);
  if (!session) {
    log(`openSessionById(${sessionId}): session not found — likely closed`);
    return "gone";
  }

  addSession(session);

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
  });
  setActiveSession(session.id);
  return "opened";
}
