import { openUrl } from "@tauri-apps/plugin-opener";
import { setActiveSession } from "$lib/stores/sessions";
import { setLogicalFocus } from "$lib/panes/focus";
import { paneInstances } from "$lib/panes/instances";
import { get } from "svelte/store";
import { sessionState } from "$lib/stores/sessions";
import { log } from "$lib/logging";
import {
  dismissNotificationSource,
  markNotificationRead,
  removeNotification,
  getNotificationSnapshot,
} from "$lib/stores/notifications";
import type { NotificationAction } from "$lib/types";

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
      // Find which session owns this pane (by walking paneInstances — the
      // instance itself doesn't carry a sessionId, so we scan sessions and
      // match on known pane ids via the layout. For Phase 2 the simpler
      // approach is to just set logical focus and hope the current session
      // contains it; if not, the caller should have used focusSession.
      const instances = get(paneInstances);
      if (instances.has(kind.paneId)) {
        setLogicalFocus(kind.paneId);
      } else {
        log(`focusPane: pane ${kind.paneId} not found in current instances`);
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
      // openUrl handles file:// paths on all platforms.
      const path = kind.path.startsWith("file://")
        ? kind.path
        : `file://${kind.path}`;
      await openUrl(path);
      await markNotificationRead(notificationId);
      break;
    }
    case "runCommand": {
      // Frontend command registry wiring. Phase 2 defers this because
      // there's no current caller; stub + log so it's visible.
      log(
        `notification.runCommand: command=${kind.commandId} (not yet wired — Phase 3)`,
      );
      break;
    }
    case "retryWatch": {
      // RetryWatch backend command lands with the hook-bridge work;
      // stub + log for Phase 2. The button still appears and records intent.
      log(
        `notification.retryWatch: watch=${kind.watchId} (not yet wired — Phase 3)`,
      );
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
  // Silence unused-var warning if session is unused in the current branch.
  void sessionState;
}
