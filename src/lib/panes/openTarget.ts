import type { Session } from "$lib/bindings";

/**
 * Inputs for the pure session-open-target resolver.
 *
 * All inputs are read-only snapshots — the resolver has no side effects
 * and does not touch stores, backends, or the filesystem.
 */
export interface SessionOpenTargetInput {
  /** The session id being opened. */
  sessionId: string;
  /**
   * A PTY id explicitly requested by the caller, e.g. a planning run's
   * `run.ptyId`. When set and the PTY is live, it takes priority over the
   * session's `primaryPtyId`.
   */
  requestedPtyId?: string | null;
  /**
   * The session record found in the *active* backend list (via
   * `listSessions`). `null` means the session is not in the active set.
   */
  activeSession: Session | null;
  /**
   * The session record from the local desktop store (the `sessionState`
   * Svelte store). This is a best-effort fallback when the backend list
   * comes up empty — it lets the resolver distinguish "archived" from
   * "truly gone".
   */
  localSession: Session | null;
  /**
   * IDs of PTYs currently alive on the backend (`listAllPtys`).
   * `null` means the inventory could not be read (treated as "no live
   * PTYs").
   */
  livePtyIds: ReadonlySet<string> | null;
}

/**
 * Discriminated union of possible open-target decisions.
 *
 * Callers switch on `kind` and execute the corresponding imperative action:
 *
 * - **attach** — a live PTY exists; restore panes and attach terminal output.
 * - **continue** — the session exists but its PTY is dead; continue/restart.
 * - **restore-then-continue** — the session is not in the active list but
 *   may be archived; restore it, then continue/restart.
 * - **gone** — the session does not exist anywhere; nothing to open.
 */
export type SessionOpenTargetDecision =
  | { kind: "attach"; ptyId: string; session: Session }
  | { kind: "continue"; session: Session }
  | {
      kind: "restore-then-continue";
      sessionId: string;
      localSession: Session | null;
    }
  | { kind: "gone"; sessionId: string };

/**
 * Resolve what to do when the user asks to open a session.
 *
 * Pure function — no side effects, no async, no store access.
 *
 * Decision rules (first match wins):
 * 1. Active session exists → check if the target PTY is live.
 *    - Live → `attach` with that PTY.
 *    - Dead → `continue` (reattach/restart).
 * 2. No active session, but local store has one → `restore-then-continue`.
 * 3. Nothing anywhere → `gone`.
 */
export function resolveSessionOpenTarget(
  input: SessionOpenTargetInput,
): SessionOpenTargetDecision {
  const { sessionId, requestedPtyId, activeSession, localSession, livePtyIds } =
    input;

  if (activeSession) {
    const targetPtyId =
      requestedPtyId ?? activeSession.primaryPtyId ?? sessionId;
    if (_canAttachPty(targetPtyId, livePtyIds)) {
      return { kind: "attach", ptyId: targetPtyId, session: activeSession };
    }
    return { kind: "continue", session: activeSession };
  }

  if (localSession) {
    return { kind: "restore-then-continue", sessionId, localSession };
  }

  return { kind: "gone", sessionId };
}

function _canAttachPty(
  ptyId: string,
  livePtyIds: ReadonlySet<string> | null,
): boolean {
  return livePtyIds?.has(ptyId) === true;
}
