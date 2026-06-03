import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  notifications,
  unreadTotal,
  unreadBySession,
  applyNotificationEvent,
} from "../notifications";
import type { Notification } from "$lib/types";

function makeNotification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: crypto.randomUUID(),
    createdAt: Date.now(),
    level: "info",
    source: { type: "cli" },
    title: "test",
    subtitle: null,
    body: null,
    sessionId: null,
    read: false,
    actions: [],
    ...overrides,
  };
}

describe("notifications store", () => {
  beforeEach(() => {
    notifications.set([]);
  });

  describe("applyNotificationEvent", () => {
    it("added prepends to the store newest-first", () => {
      const a = makeNotification({ title: "first" });
      const b = makeNotification({ title: "second" });
      applyNotificationEvent({ type: "added", notification: a });
      applyNotificationEvent({ type: "added", notification: b });
      const list = get(notifications);
      expect(list.map((n) => n.title)).toEqual(["second", "first"]);
    });

    it("added dedupes by id (guards hydrate + event race)", () => {
      const n = makeNotification({ title: "dup" });
      applyNotificationEvent({ type: "added", notification: n });
      applyNotificationEvent({ type: "added", notification: n });
      expect(get(notifications)).toHaveLength(1);
    });

    it("updated replaces by id and preserves order", () => {
      const a = makeNotification({ title: "a" });
      const b = makeNotification({ title: "b" });
      applyNotificationEvent({ type: "added", notification: a });
      applyNotificationEvent({ type: "added", notification: b });
      const bUpdated: Notification = { ...b, title: "b-updated" };
      applyNotificationEvent({ type: "updated", notification: bUpdated });
      const list = get(notifications);
      // b was newest, should still be at the top
      expect(list[0].title).toBe("b-updated");
      expect(list[1].title).toBe("a");
    });

    it("read marks a single notification read", () => {
      const a = makeNotification();
      applyNotificationEvent({ type: "added", notification: a });
      applyNotificationEvent({ type: "read", id: a.id });
      expect(get(notifications)[0].read).toBe(true);
    });

    it("readAll with sessionId marks only that session", () => {
      const global = makeNotification({ sessionId: null });
      const s1 = makeNotification({ sessionId: "s1" });
      const s2 = makeNotification({ sessionId: "s2" });
      applyNotificationEvent({ type: "added", notification: global });
      applyNotificationEvent({ type: "added", notification: s1 });
      applyNotificationEvent({ type: "added", notification: s2 });
      applyNotificationEvent({ type: "readAll", sessionId: "s1" });
      const list = get(notifications);
      expect(list.find((n) => n.id === global.id)?.read).toBe(false);
      expect(list.find((n) => n.id === s1.id)?.read).toBe(true);
      expect(list.find((n) => n.id === s2.id)?.read).toBe(false);
    });

    it("removed drops the notification", () => {
      const a = makeNotification();
      const b = makeNotification();
      applyNotificationEvent({ type: "added", notification: a });
      applyNotificationEvent({ type: "added", notification: b });
      applyNotificationEvent({ type: "removed", id: a.id });
      const list = get(notifications);
      expect(list).toHaveLength(1);
      expect(list[0].id).toBe(b.id);
    });

    it("cleared with sessionId only clears that session", () => {
      const global = makeNotification({ sessionId: null });
      const s1 = makeNotification({ sessionId: "s1" });
      const s2 = makeNotification({ sessionId: "s2" });
      applyNotificationEvent({ type: "added", notification: global });
      applyNotificationEvent({ type: "added", notification: s1 });
      applyNotificationEvent({ type: "added", notification: s2 });
      applyNotificationEvent({ type: "cleared", sessionId: "s1" });
      const list = get(notifications);
      expect(list).toHaveLength(2);
      expect(list.find((n) => n.id === s1.id)).toBeUndefined();
    });

    it("cleared with null sessionId clears everything", () => {
      applyNotificationEvent({
        type: "added",
        notification: makeNotification(),
      });
      applyNotificationEvent({
        type: "added",
        notification: makeNotification(),
      });
      applyNotificationEvent({ type: "cleared", sessionId: null });
      expect(get(notifications)).toHaveLength(0);
    });
  });

  describe("derived stores", () => {
    it("unreadTotal counts unread notifications only", () => {
      const a = makeNotification();
      const b = makeNotification({ read: true });
      const c = makeNotification();
      notifications.set([a, b, c]);
      expect(get(unreadTotal)).toBe(2);
    });

    it("unreadBySession groups by sessionId with global bucket", () => {
      const global = makeNotification({ sessionId: null });
      const s1a = makeNotification({ sessionId: "s1" });
      const s1b = makeNotification({ sessionId: "s1" });
      const s2 = makeNotification({ sessionId: "s2" });
      const s1Read = makeNotification({ sessionId: "s1", read: true });
      notifications.set([global, s1a, s1b, s2, s1Read]);
      const map = get(unreadBySession);
      expect(map.get("__global__")).toBe(1);
      expect(map.get("s1")).toBe(2);
      expect(map.get("s2")).toBe(1);
      // Read notifications shouldn't contribute
      expect([...map.values()].reduce((a, b) => a + b, 0)).toBe(4);
    });

    it("unreadBySession omits empty buckets", () => {
      notifications.set([]);
      expect(get(unreadBySession).size).toBe(0);
    });
  });
});
