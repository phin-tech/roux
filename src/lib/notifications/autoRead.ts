import { get } from "svelte/store";
import { activeSessionId } from "$lib/stores/sessions";
import { focusedPaneId } from "$lib/panes/focus";
import { collectLeafIds, sessionLayouts } from "$lib/panes/layout";
import {
  markNotificationRead,
  notifications,
} from "$lib/stores/notifications";
import type { Notification } from "$lib/types";
import { logError } from "$lib/logging";

let unsubscribeActiveSession: (() => void) | null = null;
let unsubscribeFocusedPane: (() => void) | null = null;
let unsubscribeNotifications: (() => void) | null = null;
let unsubscribeLayouts: (() => void) | null = null;
const pendingReadIds = new Set<string>();
const MAX_AUTO_READ_CONCURRENCY = 8;
let inFlightAutoReads = 0;

export function initNotificationAutoRead(): void {
  if (unsubscribeActiveSession) return;

  const refresh = () => {
    markRelevantNotificationsRead();
  };

  unsubscribeActiveSession = activeSessionId.subscribe(refresh);
  unsubscribeFocusedPane = focusedPaneId.subscribe(refresh);
  unsubscribeNotifications = notifications.subscribe(refresh);
  unsubscribeLayouts = sessionLayouts.subscribe(refresh);
}

export function stopNotificationAutoRead(): void {
  unsubscribeActiveSession?.();
  unsubscribeFocusedPane?.();
  unsubscribeNotifications?.();
  unsubscribeLayouts?.();
  unsubscribeActiveSession = null;
  unsubscribeFocusedPane = null;
  unsubscribeNotifications = null;
  unsubscribeLayouts = null;
  pendingReadIds.clear();
  inFlightAutoReads = 0;
}

function markRelevantNotificationsRead(): void {
  const focusedPane = get(focusedPaneId);
  const activeSession = get(activeSessionId);
  const focusedPaneSession = focusedPane ? findSessionForPane(focusedPane) : null;
  const snapshot = get(notifications);
  const unreadIds = new Set(snapshot.filter((n) => !n.read).map((n) => n.id));
  for (const id of pendingReadIds) {
    if (!unreadIds.has(id)) pendingReadIds.delete(id);
  }

  for (const notification of snapshot) {
    if (notification.read || pendingReadIds.has(notification.id)) continue;
    if (!matchesNavigationTarget(notification, activeSession, focusedPaneSession, focusedPane)) {
      continue;
    }
    if (inFlightAutoReads >= MAX_AUTO_READ_CONCURRENCY) break;
    pendingReadIds.add(notification.id);
    inFlightAutoReads += 1;
    let failed = false;
    void markNotificationRead(notification.id)
      .catch((e) => {
        failed = true;
        pendingReadIds.delete(notification.id);
        logError("notification auto-read failed", e);
      })
      .finally(() => {
        inFlightAutoReads = Math.max(0, inFlightAutoReads - 1);
        if (!failed && unsubscribeActiveSession) {
          queueMicrotask(markRelevantNotificationsRead);
        }
      });
  }
}

function matchesNavigationTarget(
  notification: Notification,
  activeSession: string | null,
  focusedPaneSession: string | null,
  paneId: string | null,
): boolean {
  if (activeSession && notification.sessionId === activeSession) return true;
  const focusedPaneMatchesActiveSession =
    !activeSession || focusedPaneSession === activeSession;
  if (
    focusedPaneMatchesActiveSession &&
    focusedPaneSession &&
    notification.sessionId === focusedPaneSession
  ) {
    return true;
  }
  if (!focusedPaneMatchesActiveSession || !focusedPaneSession || !paneId) {
    return false;
  }
  return notification.actions.some((action) => {
    const kind = action.kind;
    return kind.type === "focusPane" && kind.paneId === paneId;
  });
}

function findSessionForPane(paneId: string): string | null {
  for (const [sessionId, layout] of get(sessionLayouts)) {
    if (collectLeafIds(layout).includes(paneId)) return sessionId;
  }
  return null;
}
