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
import { notificationsMarkRead, notificationsRemove } from "$lib/tauri";
import {
  initNotificationAutoRead,
  stopNotificationAutoRead,
} from "../autoRead";

function waitTick(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
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
    vi.mocked(notificationsMarkRead).mockReset();
    vi.mocked(notificationsMarkRead).mockResolvedValue(true);
    vi.mocked(notificationsRemove).mockReset();
    vi.mocked(notificationsRemove).mockResolvedValue(true);
  });

  afterEach(() => {
    stopNotificationAutoRead();
  });

  it("marks session-scoped and pane-targeted notifications read when focusing a pane", async () => {
    const sessionScoped = makeNotification({
      id: "session-scoped",
      sessionId: "s1",
    });
    const paneTargeted = makeNotification({
      id: "pane-targeted",
      actions: [focusPaneAction("pane-a")],
    });
    const otherSession = makeNotification({
      id: "other-session",
      sessionId: "s2",
    });
    const global = makeNotification({ id: "global", sessionId: null });

    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-a" }]]));
    notifications.set([sessionScoped, paneTargeted, otherSession, global]);

    initNotificationAutoRead();
    focusedPaneId.set("pane-a");
    await waitTick();

    expect(
      vi
        .mocked(notificationsMarkRead)
        .mock.calls.map(([id]) => id)
        .sort(),
    ).toEqual(["pane-targeted", "session-scoped"]);
  });

  it("marks session-scoped notifications read when selecting a session", async () => {
    const activeSession = makeNotification({
      id: "active-session",
      sessionId: "s1",
    });
    const otherSession = makeNotification({
      id: "other-session",
      sessionId: "s2",
    });
    const global = makeNotification({ id: "global", sessionId: null });
    notifications.set([activeSession, otherSession, global]);

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(
      vi.mocked(notificationsMarkRead).mock.calls.map(([id]) => id),
    ).toEqual(["active-session"]);
  });

  it("uses the active session even when focus still points at a previous pane", async () => {
    const activeSession = makeNotification({
      id: "active-session",
      sessionId: "s2",
    });
    const previousSession = makeNotification({
      id: "previous-session",
      sessionId: "s1",
    });
    const previousPane = makeNotification({
      id: "previous-pane",
      actions: [focusPaneAction("pane-a")],
    });

    sessionLayouts.set(new Map([["s1", { kind: "leaf", paneId: "pane-a" }]]));
    focusedPaneId.set("pane-a");
    notifications.set([activeSession, previousSession, previousPane]);
    sessionState.set({ sessions: [], activeSessionId: "s2" });

    initNotificationAutoRead();
    await waitTick();

    expect(
      vi.mocked(notificationsMarkRead).mock.calls.map(([id]) => id),
    ).toEqual(["active-session"]);
  });

  it("does not mark pane-targeted notifications read before pane layouts hydrate", async () => {
    const paneTargeted = makeNotification({
      id: "pane-targeted",
      actions: [focusPaneAction("pane-a")],
    });

    focusedPaneId.set("pane-a");
    notifications.set([paneTargeted]);

    initNotificationAutoRead();
    await waitTick();

    expect(notificationsMarkRead).not.toHaveBeenCalled();
  });

  it("auto-removes completion-session notifications when the matching session becomes active", async () => {
    const completion = makeNotification({
      id: "completion-1",
      sessionId: "s1",
      dedupKey: "completion:session:s1",
      level: "success",
    });
    notifications.set([completion]);

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(notificationsRemove).toHaveBeenCalledWith("completion-1");
    expect(notificationsMarkRead).not.toHaveBeenCalled();
  });

  it("does not auto-remove completion-session notifications for other sessions", async () => {
    const completion = makeNotification({
      id: "completion-2",
      sessionId: "s2",
      dedupKey: "completion:session:s2",
      level: "success",
    });
    notifications.set([completion]);

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(notificationsRemove).not.toHaveBeenCalled();
  });

  it("does not auto-remove completion:pane fallback notifications", async () => {
    const fallback = makeNotification({
      id: "fallback-pane",
      sessionId: "s1",
      dedupKey: "completion:pane:pane-1",
      level: "success",
    });
    notifications.set([fallback]);

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(notificationsRemove).not.toHaveBeenCalled();
    // It still gets marked read via the normal path.
    expect(notificationsMarkRead).toHaveBeenCalledWith("fallback-pane");
  });

  it("limits concurrent auto-remove requests and drains remaining matches", async () => {
    const blockers: Array<ReturnType<typeof deferred<boolean>>> = [];
    vi.mocked(notificationsRemove).mockImplementation(() => {
      const blocker = deferred<boolean>();
      blockers.push(blocker);
      return blocker.promise;
    });
    notifications.set(
      Array.from({ length: 10 }, (_, index) =>
        makeNotification({
          id: `completion-${index}`,
          sessionId: "s1",
          dedupKey: "completion:session:s1",
          level: "success",
        }),
      ),
    );

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(notificationsRemove).toHaveBeenCalledTimes(8);

    for (const blocker of blockers.slice(0, 8)) {
      blocker.resolve(true);
    }
    await waitTick();
    await waitTick();

    expect(notificationsRemove).toHaveBeenCalledTimes(10);
  });

  it("limits concurrent auto-read requests and drains remaining matches", async () => {
    const blockers: Array<ReturnType<typeof deferred<boolean>>> = [];
    vi.mocked(notificationsMarkRead).mockImplementation(() => {
      const blocker = deferred<boolean>();
      blockers.push(blocker);
      return blocker.promise;
    });
    notifications.set(
      Array.from({ length: 10 }, (_, index) =>
        makeNotification({ id: `notification-${index}`, sessionId: "s1" }),
      ),
    );

    initNotificationAutoRead();
    sessionState.set({ sessions: [], activeSessionId: "s1" });
    await waitTick();

    expect(notificationsMarkRead).toHaveBeenCalledTimes(8);

    for (const blocker of blockers.slice(0, 8)) {
      blocker.resolve(true);
    }
    await waitTick();
    await waitTick();

    expect(notificationsMarkRead).toHaveBeenCalledTimes(10);
  });
});
