import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

vi.mock("$lib/tauri", () => ({
  listNotifications: vi.fn().mockResolvedValue([]),
  notificationsMarkRead: vi.fn().mockResolvedValue(true),
  notificationsMarkAllRead: vi.fn().mockResolvedValue(0),
  notificationsRemove: vi.fn().mockResolvedValue(true),
  notificationsClear: vi.fn().mockResolvedValue(0),
  notificationsDismissSource: vi.fn().mockResolvedValue(0),
}));

import { notifications } from "$lib/stores/notifications";
import { focusedPaneId, resetFocus } from "$lib/panes/focus";
import { resetLayouts, sessionLayouts } from "$lib/panes/layout";
import { sessionState } from "$lib/stores/sessions";
import type { Notification } from "$lib/types";
import { notificationsMarkRead } from "$lib/tauri";
import {
  initNotificationAutoRead,
  stopNotificationAutoRead,
} from "../autoRead";

function waitTick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function makeNotification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: crypto.randomUUID(),
    createdAt: Date.now(),
    level: "info",
    source: { type: "hook", provider: "claude" },
    title: "test",
    subtitle: null,
    body: null,
    sessionId: null,
    read: false,
    actions: [],
    dedupKey: null,
    ...overrides,
  };
}

function focusPaneAction(paneId: string): Notification["actions"][number] {
  return {
    id: "focus",
    label: "Focus pane",
    kind: { type: "focusPane", paneId },
    primary: true,
  };
}

describe("notification auto-read", () => {
  beforeEach(() => {
    stopNotificationAutoRead();
    notifications.set([]);
    resetFocus();
    resetLayouts();
    sessionState.set({ sessions: [], activeSessionId: null });
    vi.mocked(notificationsMarkRead).mockClear();
  });

  afterEach(() => {
    stopNotificationAutoRead();
  });

  it("marks session-scoped and pane-targeted notifications read when focusing a pane", async () => {
    const sessionScoped = makeNotification({ id: "session-scoped", sessionId: "s1" });
    const paneTargeted = makeNotification({
      id: "pane-targeted",
      actions: [focusPaneAction("pane-a")],
    });
    const otherSession = makeNotification({ id: "other-session", sessionId: "s2" });
    const global = makeNotification({ id: "global", sessionId: null });

    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-a" }]]));
    notifications.set([sessionScoped, paneTargeted, otherSession, global]);

    initNotificationAutoRead();
    focusedPaneId.set("pane-a");
    await waitTick();

    expect(vi.mocked(notificationsMarkRead).mock.calls.map(([id]) => id).sort()).toEqual([
      "pane-targeted",
      "session-scoped",
    ]);
  });

  it("marks session-scoped notifications read when selecting a session", async () => {
    const activeSession = makeNotification({ id: "active-session", sessionId: "s1" });
    const otherSession = makeNotification({ id: "other-session", sessionId: "s2" });
    const global = makeNotification({ id: "global", sessionId: null });
    notifications.set([activeSession, otherSession, global]);

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(vi.mocked(notificationsMarkRead).mock.calls.map(([id]) => id)).toEqual([
      "active-session",
    ]);
  });

  it("uses the active session even when focus still points at a previous pane", async () => {
    const activeSession = makeNotification({ id: "active-session", sessionId: "s2" });
    const previousSession = makeNotification({ id: "previous-session", sessionId: "s1" });

    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-a" }]]));
    focusedPaneId.set("pane-a");
    notifications.set([activeSession, previousSession]);

    initNotificationAutoRead();
    vi.mocked(notificationsMarkRead).mockClear();
    sessionState.set({ sessions: [], activeSessionId: "s2" });
    await waitTick();

    expect(vi.mocked(notificationsMarkRead).mock.calls.map(([id]) => id)).toEqual([
      "active-session",
    ]);
  });
});
