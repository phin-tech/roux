import { get } from "svelte/store";
import { agentStates, type AgentState } from "./agentState";
import { paneInstances } from "./instances";
import { resolveProfileRef } from "./profiles";
import { notificationsPush } from "$lib/tauri";
import { log, logError } from "$lib/logging";

/**
 * Per-pane completion notifications.
 *
 * Subscribes to the agentStates store, tracks previous status per pane,
 * and pushes a single notification whenever a pane transitions
 * `generating → idle`. Matches the spec:
 *
 * > Notification service receives pane-status-update events and fires
 * > per-pane on `generating → idle` transitions. Two panes finishing in
 * > the same session produces two notifications.
 *
 * Window-focus suppression and OS-notification fan-out happen on the Rust
 * side inside `notifications::NotificationManager::push`, so we only need
 * to detect the transition and hand off.
 */

type Status = AgentState["status"];

/**
 * Last-observed status for every pane we've seen at least once, keyed by
 * pane id. Populated by the agentStates subscription so we can compare
 * the new value against the previous one. We intentionally keep entries
 * around after a pane closes — the cleanup happens in `disposeAgentState`
 * via the dispose hook, which also removes the pane's entry here.
 */
const lastStatusByPane = new Map<string, Status>();

/**
 * Keep `lastStatusByPane` in sync with pane disposal so a brand-new pane
 * that happens to reuse an id (the generated-pane-id collision case) does
 * not inherit the previous pane's status and drop its first transition.
 */
export function forgetLastStatus(paneId: string): void {
  lastStatusByPane.delete(paneId);
}

let unsubscribe: (() => void) | null = null;

/**
 * Start listening for transitions. Idempotent — calling twice keeps one
 * live subscription. Call once at app startup (App.svelte) so the wiring
 * is active for the whole session.
 */
export function initAgentNotifications(): void {
  if (unsubscribe) return;
  unsubscribe = agentStates.subscribe((states) => {
    for (const [paneId, state] of states) {
      const prev = lastStatusByPane.get(paneId);
      lastStatusByPane.set(paneId, state.status);

      // Only fire on an actual transition from generating to idle. The
      // first time we see a pane (prev === undefined) is intentionally
      // ignored so a brand-new "idle" doesn't masquerade as completion.
      if (prev === "generating" && state.status === "idle") {
        void fireCompletionNotification(paneId, state);
      }
    }

    // Drop entries for panes that no longer have an agentState — keeps
    // the map bounded and matches the dispose semantics of the store.
    for (const paneId of [...lastStatusByPane.keys()]) {
      if (!states.has(paneId)) lastStatusByPane.delete(paneId);
    }
  });
}

/** Test-only. Tears down the subscription and clears internal state. */
export function stopAgentNotifications(): void {
  if (unsubscribe) {
    unsubscribe();
    unsubscribe = null;
  }
  lastStatusByPane.clear();
}

async function fireCompletionNotification(
  paneId: string,
  state: AgentState,
): Promise<void> {
  const instance = get(paneInstances).get(paneId);
  const profile = resolveProfileRef(instance?.spawnProfileRef);
  const title = deriveTitle(instance?.name, profile?.name, state.provider);
  const body = `${capitalize(state.provider)} finished generating.`;

  try {
    await notificationsPush({
      level: "success",
      source: { type: "hook", provider: state.provider },
      title,
      subtitle: null,
      body,
      sessionId: null,
      actions: [
        {
          id: "focus",
          label: "Focus pane",
          kind: { type: "focusPane", paneId },
          primary: true,
        },
        {
          id: "dismiss",
          label: "Dismiss",
          kind: { type: "dismiss" },
          primary: false,
        },
      ],
    });
    log(`agentNotifications: fired generating→idle notification for pane ${paneId}`);
  } catch (e) {
    logError("agentNotifications: notificationsPush failed", e);
  }
}

function deriveTitle(
  paneName: string | undefined,
  profileName: string | undefined,
  provider: string,
): string {
  if (paneName) return `${paneName} is idle`;
  if (profileName) return `${profileName} is idle`;
  return `${capitalize(provider)} is idle`;
}

function capitalize(s: string): string {
  return s.length === 0 ? s : s.charAt(0).toUpperCase() + s.slice(1);
}
