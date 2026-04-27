import { openUrl } from "@tauri-apps/plugin-opener";
import { openPathInFinder } from "$lib/tauri";
import { setActiveSession, sessionState } from "$lib/stores/sessions";
import { setLogicalFocus } from "$lib/panes/focus";
import { paneInstances } from "$lib/panes/instances";
import { sessionLayouts, collectLeafIds } from "$lib/panes/layout";
import { get } from "svelte/store";
import { log } from "$lib/logging";
import {
  dismissNotificationSource,
  markNotificationRead,
  removeNotification,
  getNotificationSnapshot,
} from "$lib/stores/notifications";
import type { NotificationAction } from "$lib/types";

/** Find which session owns a pane by walking layouts. Returns null if none. */
function findSessionForPane(paneId: string): string | null {
  for (const [sessionId, layout] of get(sessionLayouts)) {
    for (const leafId of collectLeafIds(layout)) {
      if (leafId === paneId) return sessionId;
    }
  }
  return null;
}

/**
 * Wait until a pane instance is registered, then set logical focus on it.
 * Needed when switching sessions: panes mount asynchronously after
 * `setActiveSession`, so an immediate `setLogicalFocus` would no-op
 * because the instance isn't in `paneInstances` yet. Gives up after
 * `timeoutMs` to avoid hanging if the pane never mounts.
 */
function focusPaneWhenMounted(paneId: string, timeoutMs = 2000): void {
  if (get(paneInstances).has(paneId)) {
    setLogicalFocus(paneId);
    return;
  }
  let done = false;
  const unsubscribe = paneInstances.subscribe((instances) => {
    if (done || !instances.has(paneId)) return;
    done = true;
    setLogicalFocus(paneId);
    // Defer unsubscribe so Svelte's own subscription bookkeeping settles.
    queueMicrotask(() => unsubscribe());
  });
  setTimeout(() => {
    if (done) return;
    done = true;
    unsubscribe();
    log(`focusPane: pane ${paneId} never mounted within ${timeoutMs}ms`);
  }, timeoutMs);
}

/**
 * Execute a notification action. The notification id is needed so that
 * source-scoped dismissals and read-marking can be done off the stored
 * snapshot rather than re-passed from the caller.
 */
export async function dispatchNotificationAction(
  notificationId: string,
  action: NotificationAction,
): Promise<void> {
  const notification = getNotificationSnapshot(notificationId);
  const kind = action.kind;

  switch (kind.type) {
    case "focusSession": {
      setActiveSession(kind.sessionId);
      await markNotificationRead(notificationId);
      break;
    }
    case "focusPane": {
      // Walk all session layouts so a notification's Focus-pane action
      // works even when the target pane lives in a different session than
      // the one currently active. Without this, cross-session notifications
      // silently no-op (the pane's instance isn't registered until its
      // owning session is active).
      const owningSessionId = findSessionForPane(kind.paneId);
      if (!owningSessionId) {
        log(`focusPane: pane ${kind.paneId} not found in any session`);
      } else {
        if (owningSessionId !== get(sessionState).activeSessionId) {
          setActiveSession(owningSessionId);
        }
        focusPaneWhenMounted(kind.paneId);
      }
      await markNotificationRead(notificationId);
      break;
    }
    case "openUrl": {
      await openUrl(kind.url);
      await markNotificationRead(notificationId);
      break;
    }
    case "openPath": {
      await openPathInFinder(kind.path);
      await markNotificationRead(notificationId);
      break;
    }
    case "runCommand": {
      // TODO: wire to the frontend command registry once a notification
      // source actually emits runCommand actions.
      log(`notification.runCommand: command=${kind.commandId} (not yet wired)`);
      break;
    }
    case "retryWatch": {
      // TODO: call the backend watch-retry command once it exists. The
      // button currently records intent but is a no-op.
      log(`notification.retryWatch: watch=${kind.watchId} (not yet wired)`);
      break;
    }
    case "dismiss": {
      await removeNotification(notificationId);
      break;
    }
    case "dismissSource": {
      if (notification) {
        await dismissNotificationSource(notification.source);
      }
      break;
    }
    case "markRead": {
      await markNotificationRead(notificationId);
      break;
    }
  }
}
