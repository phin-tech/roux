import { derived, get } from "svelte/store";
import {
  pendingDecisionByItem,
  latestRunByItem,
  workItems,
} from "$lib/stores/workItems";
import type { WorkItemDecision } from "$lib/types/workItems";
import { activeSessionId } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";
import { notificationsPush, notificationsRemove } from "$lib/tauri";
import { log, logError } from "$lib/logging";

/**
 * Surfaces a work item that is blocked on a human decision.
 *
 * When a run gets a pending decision the store marks it `blocked` and the
 * board paints the card — but that is only visible if you are looking at the
 * board. This service mirrors `agentNotifications` to push an `attention`
 * notification on a new pending decision and remove it once the decision is
 * resolved or times out, so a blocked item surfaces even when you are inside a
 * terminal pane or the window is backgrounded.
 *
 * Gated on the global `notificationsEnabled` flag (a dedicated, persisted
 * toggle would require a Rust `RouxSettings` field; deferred). Window-focus
 * suppression and OS fan-out happen Rust-side in `NotificationManager::push`;
 * we add session-level suppression to match `agentNotifications`.
 */

interface TrackedDecision {
  decisionId: string;
  /** Id of the pushed notification, filled in after the push resolves. */
  notificationId: string | null;
}

/** The decision we last notified about, per work item id. */
const trackedByItem = new Map<string, TrackedDecision>();

let unsubscribe: (() => void) | null = null;

/**
 * Start listening for blocked work items. Idempotent — calling twice keeps one
 * live subscription. Call once at app startup (App.svelte).
 */
export function initWorkItemNotifications(): void {
  if (unsubscribe) return;
  const source = derived(
    [pendingDecisionByItem, activeSessionId, settings],
    ([$pending, $activeSessionId, $settings]) => ({
      pending: $pending,
      activeSessionId: $activeSessionId,
      notificationsEnabled: $settings.notificationsEnabled,
    }),
  );
  unsubscribe = source.subscribe((state) => {
    // Dismiss notifications for items whose decision resolved, timed out, or
    // was replaced by a different decision.
    for (const [itemId, tracked] of [...trackedByItem]) {
      const current = state.pending.get(itemId);
      if (!current || current.id !== tracked.decisionId) {
        trackedByItem.delete(itemId);
        void dismissDecisionNotification(tracked);
      }
    }

    // Fire for newly-pending decisions.
    for (const [itemId, decision] of state.pending) {
      const tracked = trackedByItem.get(itemId);
      if (tracked && tracked.decisionId === decision.id) continue;
      const run = get(latestRunByItem).get(itemId);
      const sessionId = run?.sessionId ?? null;
      if (!state.notificationsEnabled) continue;
      if (sessionId && state.activeSessionId === sessionId) continue;
      // Mark synchronously only after suppression checks pass so a later
      // settings/focus change can still notify for the same pending decision.
      const entry: TrackedDecision = {
        decisionId: decision.id,
        notificationId: null,
      };
      trackedByItem.set(itemId, entry);
      void fireDecisionNotification(itemId, decision, entry);
    }
  });
}

/** Test-only. Tears down the subscription and clears internal state. */
export function stopWorkItemNotifications(): void {
  if (unsubscribe) {
    unsubscribe();
    unsubscribe = null;
  }
  trackedByItem.clear();
}

async function fireDecisionNotification(
  itemId: string,
  decision: WorkItemDecision,
  entry: TrackedDecision,
): Promise<void> {
  const run = get(latestRunByItem).get(itemId);
  const sessionId = run?.sessionId ?? null;

  const item = get(workItems).find((i) => i.id === itemId);
  const title = item?.title
    ? `Decision needed: ${item.title}`
    : "Decision needed";

  const actions = sessionId
    ? [
        {
          id: "focus",
          label: "Open session",
          kind: { type: "focusSession" as const, sessionId },
          primary: true,
        },
        {
          id: "dismiss",
          label: "Dismiss",
          kind: { type: "dismiss" as const },
          primary: false,
        },
      ]
    : [
        {
          id: "dismiss",
          label: "Dismiss",
          kind: { type: "dismiss" as const },
          primary: true,
        },
      ];

  try {
    const notification = await notificationsPush({
      level: "attention",
      source: { type: "internal" },
      title,
      subtitle: null,
      body: decision.question,
      sessionId,
      actions,
      dedupKey: `work-item-decision:${decision.id}`,
    });
    // If the decision resolved while we were pushing, the dismiss path already
    // dropped this entry — remove the orphaned notification instead of leaking.
    if (trackedByItem.get(itemId) === entry) {
      entry.notificationId = notification.id;
    } else {
      void notificationsRemove(notification.id);
    }
    log(`workItemNotifications: fired blocked notification for item ${itemId}`);
  } catch (e) {
    if (trackedByItem.get(itemId) === entry) {
      trackedByItem.delete(itemId);
    }
    logError("workItemNotifications: notificationsPush failed", e);
  }
}

async function dismissDecisionNotification(
  tracked: TrackedDecision,
): Promise<void> {
  if (!tracked.notificationId) return;
  try {
    await notificationsRemove(tracked.notificationId);
    log("workItemNotifications: dismissed resolved decision notification");
  } catch (e) {
    logError("workItemNotifications: notificationsRemove failed", e);
  }
}
