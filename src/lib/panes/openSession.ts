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
import { resolveSessionOpenTarget, type SessionOpenTargetDecision } from "./openTarget";
import type { Session } from "$lib/bindings";

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
 * Fall back to the local store copy when backend lookup + archive
 * restore both come up empty. This handles the case where the session
 * was registered client-side but hasn't been persisted yet.
 */
function _fallbackOrGone(
  sessionId: string,
  requestedPtyId: string | null | undefined,
  localSession: Session | null,
  livePtyIds: ReadonlySet<string> | null,
): SessionOpenTargetDecision {
  if (localSession) {
    return resolveSessionOpenTarget({
      sessionId,
      requestedPtyId,
      activeSession: localSession,
      localSession,
      livePtyIds,
    });
  }
  return { kind: "gone", sessionId };
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
  // ── Fast path: already registered locally with a live pane ──────────
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

  // ── Look up the session ─────────────────────────────────────────────
  const sessions = await listSessions();
  const activeSession = sessions.find((s) => s.id === sessionId) ?? null;

  // ── Read live PTY inventory ─────────────────────────────────────────
  let livePtyIds: Set<string> | null = null;
  try {
    livePtyIds = new Set((await listAllPtys()).map((pty) => pty.id));
  } catch (e) {
    livePtyIds = null;
    log(
      `openSessionById(${sessionId}): unable to read live PTY inventory: ${e}`,
    );
  }

  // ── Resolve the open target ─────────────────────────────────────────
  let decision = resolveSessionOpenTarget({
    sessionId,
    requestedPtyId: options.ptyId,
    activeSession,
    localSession: existing,
    livePtyIds,
  });

  let wasRestoredFromArchive = false;

  // ── Handle restore-then-continue ────────────────────────────────────
  //
  // This covers two cases:
  // 1. The resolver explicitly said "restore-then-continue" (local session
  //    exists but no active backend record — likely archived).
  // 2. The resolver said "gone" (no session anywhere), but the session may
  //    still exist in the archive without a local store copy.
  if (decision.kind === "restore-then-continue" || decision.kind === "gone") {
    const hadLocalSession = decision.kind === "restore-then-continue"
      ? decision.localSession
      : null;
    try {
      await restoreArchivedSession(sessionId);
      const restoredSessions = await listSessions();
      const restored =
        restoredSessions.find((s) => s.id === sessionId) ?? null;
      if (restored) {
        wasRestoredFromArchive = true;
        // Re-resolve with the freshly restored session. Its PTY is
        // guaranteed to be dead (archived sessions have no live PTY),
        // so the resolver will return "continue".
        decision = resolveSessionOpenTarget({
          sessionId,
          requestedPtyId: options.ptyId,
          activeSession: restored,
          localSession: hadLocalSession,
          livePtyIds,
        });
      } else {
        // Archive restore didn't find it, but we may have a local copy.
        // Fall back to the local store copy (e.g. the session was
        // registered client-side but hasn't been persisted yet).
        decision = _fallbackOrGone(sessionId, options.ptyId, hadLocalSession, livePtyIds);
      }
    } catch (e) {
      log(`openSessionById(${sessionId}): archived restore failed: ${e}`);
      decision = _fallbackOrGone(sessionId, options.ptyId, hadLocalSession, livePtyIds);
    }
  }

  if (decision.kind === "gone") {
    return "gone";
  }

  // After the archive-restore block above, the decision is guaranteed to be
  // "attach" or "continue" (restore-then-continue was re-resolved, gone was
  // returned). TypeScript can't narrow through re-assignments, so we guard.
  if (decision.kind !== "attach" && decision.kind !== "continue") {
    return "gone";
  }

  let session: Session = decision.session;

  // ── Execute ─────────────────────────────────────────────────────────
  if (wasRestoredFromArchive) {
    // Full reattach with continue intent — rehydrates pane layout and
    // replays the profile with `--continue`/`resume --last`.
    const { reattachSession } = await import("$lib/sessions/reconnect");
    session = await reattachSession(session);
    addSession(session);
    setActiveSession(session.id);
    return "opened";
  }

  // xterm-heavy modules are imported lazily, mirroring the launch restore path.
  const [{ initTerminal, attachPtyListeners }, { attachPtyToPane }] =
    await Promise.all([import("./terminals"), import("./attach")]);

  const persisted = await loadPaneState(session.id);
  await restoreSessionPanes(session, persisted, {
    initTerminal,
    attachPtyListeners,
    attachLivePtyToPane: attachPtyToPane,
    livePtyIds,
    primaryPtyId:
      decision.kind === "attach" ? decision.ptyId : session.primaryPtyId ?? session.id,
  });

  if (decision.kind === "continue") {
    // PTY is dead — restart the shell with continue flags so the
    // provider picks up the last session.
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
  if (decision.kind === "attach") {
    updateSessionStatus(session.id, "idle");
  }
  setActiveSession(session.id);
  return "opened";
}
