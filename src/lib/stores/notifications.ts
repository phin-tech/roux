import { writable, derived, get } from "svelte/store";
import {
  listNotifications,
  notificationsMarkRead as tauriMarkRead,
  notificationsMarkAllRead as tauriMarkAllRead,
  notificationsRemove as tauriRemove,
  notificationsClear as tauriClear,
  notificationsDismissSource as tauriDismissSource,
} from "$lib/tauri";
import type { Notification, NotificationSource } from "$lib/types";
import type { NotificationEvent } from "$lib/types/notifications";

/** Authoritative store of notifications, newest-first (matches Rust `list`). */
export const notifications = writable<Notification[]>([]);

/** Total number of unread notifications (across all sessions + global). */
export const unreadTotal = derived(notifications, ($ns) =>
  $ns.reduce((acc, n) => acc + (n.read ? 0 : 1), 0),
);

/** Per-session unread counts. Notifications with `sessionId === null` land under the special `__global__` key. */
export const unreadBySession = derived(notifications, ($ns) => {
  const map = new Map<string, number>();
  for (const n of $ns) {
    if (n.read) continue;
    const key = n.sessionId ?? "__global__";
    map.set(key, (map.get(key) ?? 0) + 1);
  }
  return map;
});

/** Hydrate the store from the backend. Call once on app start. */
export async function hydrateNotifications(): Promise<void> {
  try {
    const list = await listNotifications();
    notifications.set(list);
  } catch (e) {
    console.error("Failed to hydrate notifications", e);
  }
}

/** Apply a NotificationEvent to the local store. Idempotent for safety. */
export function applyNotificationEvent(event: NotificationEvent): void {
  switch (event.type) {
    case "added": {
      const n = event.notification;
      notifications.update((list) => {
        // Guard against duplicate hydration + event races.
        if (list.some((existing) => existing.id === n.id)) return list;
        return [n, ...list];
      });
      break;
    }
    case "updated": {
      const n = event.notification;
      notifications.update((list) =>
        list.map((existing) => (existing.id === n.id ? n : existing)),
      );
      break;
    }
    case "read": {
      notifications.update((list) =>
        list.map((n) => (n.id === event.id ? { ...n, read: true } : n)),
      );
      break;
    }
    case "readAll": {
      const sid = event.sessionId;
      notifications.update((list) =>
        list.map((n) => {
          if (sid === null) {
            // Rust emits sessionId=null both for "global only" and "all" cases;
            // err on the side of marking everything. The server is the source
            // of truth — a subsequent hydrate will reconcile.
            return { ...n, read: true };
          }
          return n.sessionId === sid ? { ...n, read: true } : n;
        }),
      );
      break;
    }
    case "removed": {
      notifications.update((list) => list.filter((n) => n.id !== event.id));
      break;
    }
    case "cleared": {
      const sid = event.sessionId;
      if (sid === null) {
        // Same caveat as readAll: a re-hydrate reconciles anyway.
        notifications.set([]);
      } else {
        notifications.update((list) => list.filter((n) => n.sessionId !== sid));
      }
      break;
    }
  }
}

// ── Mutation helpers (call the backend; local store is updated via the event) ─────────

export async function markNotificationRead(id: string): Promise<void> {
  await tauriMarkRead(id);
}

export async function markAllNotificationsRead(
  sessionId?: string | null,
): Promise<void> {
  await tauriMarkAllRead(sessionId ?? null, null);
}

export async function removeNotification(id: string): Promise<void> {
  await tauriRemove(id);
}

export async function clearNotifications(
  sessionId?: string | null,
): Promise<void> {
  await tauriClear(sessionId ?? null, null);
}

export async function dismissNotificationSource(
  source: NotificationSource,
): Promise<void> {
  await tauriDismissSource(source);
}

/** Synchronous snapshot, primarily for action handlers that need a lookup. */
export function getNotificationSnapshot(id: string): Notification | undefined {
  return get(notifications).find((n) => n.id === id);
}
