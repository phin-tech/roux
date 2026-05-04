// Session branch poller.
//
// `Session.branch` is captured at session creation and never updated by
// itself when the user `git checkout`s inside the pane. That makes the
// status bar and the PR-lookup effect lag behind reality. This poller
// re-reads the branch via the `refresh_session_branch` Tauri command
// on a low-frequency tick. The backend is responsible for cheap
// `git rev-parse` and only writes back when the value changed, so
// the poll is bounded.
//
// The session list itself is the trigger for `refreshSessionPrEffect`:
// when the backend updates the branch, the session-list event flows
// through `activeSession`, and `installSessionPrEffect` re-runs the
// PR lookup with its dedupe key ([id|repo|branch|pinned]).
//
// Frontend-driven (rather than a backend tick) because we need a
// session-list snapshot anyway to know which sessions to poll, and it
// keeps the lifecycle alongside `installPtyInventoryPolling`.

import { get } from "svelte/store";

import { sessionList, sessionState } from "$lib/stores/sessions";
import { refreshSessionBranch } from "$lib/tauri";

const POLL_INTERVAL_MS = 15_000;

let stopPolling: (() => void) | null = null;
let inFlight = false;

/**
 * Re-read the branch of every active (non-archived) git-backed session
 * from the backend. The backend command no-ops when the value is
 * unchanged; when it changes, it updates `session_service` and the
 * frontend store gets the new value via the existing session-list
 * subscription path.
 */
export async function refreshAllSessionBranches(): Promise<void> {
  if (inFlight) return;
  const sessions = get(sessionList);
  if (sessions.length === 0) return;
  inFlight = true;
  try {
    // Sequential, not parallel — the cost is dominated by a single
    // git rev-parse per session, parallelism just creates spawn churn
    // and there's no user-perceptible latency budget at 15s.
    for (const s of sessions) {
      if (s.archived || !s.isGitRepo) continue;
      try {
        const next = await refreshSessionBranch(s.id);
        if (next && next !== s.branch) {
          // Mirror into the frontend store so reactive consumers
          // pick up the change without waiting for a list refetch.
          sessionState.update((state) => ({
            ...state,
            sessions: state.sessions.map((r) =>
              r.id === s.id ? { ...r, branch: next } : r,
            ),
          }));
        }
      } catch {
        // Individual git failures (worktree disappeared, etc.) shouldn't
        // poison the whole tick.
      }
    }
  } finally {
    inFlight = false;
  }
}

export function installSessionBranchPoller(
  intervalMs = POLL_INTERVAL_MS,
): () => void {
  if (stopPolling) return stopPolling;

  void refreshAllSessionBranches();
  const timer = setInterval(() => {
    void refreshAllSessionBranches();
  }, intervalMs);

  stopPolling = () => {
    clearInterval(timer);
    stopPolling = null;
  };
  return stopPolling;
}

export function _resetSessionBranchPollerForTests(): void {
  stopPolling?.();
  stopPolling = null;
  inFlight = false;
}
