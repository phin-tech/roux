import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Notification } from "$lib/types";
import { notifications } from "$lib/stores/notifications";
import { dispatchNotificationAction } from "../dispatchAction";
import {
  notificationsMarkRead,
  openPathInFinder,
} from "$lib/tauri";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

vi.mock("$lib/tauri", () => ({
  notificationsDismissSource: vi.fn(),
  notificationsMarkRead: vi.fn(),
  notificationsRemove: vi.fn(),
  openPathInFinder: vi.fn(),
}));

function makeNotification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "notification-1",
    createdAt: 1,
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

describe("dispatchNotificationAction", () => {
  beforeEach(() => {
    notifications.set([makeNotification()]);
    vi.mocked(notificationsMarkRead).mockReset().mockResolvedValue(true);
    vi.mocked(openPathInFinder).mockReset().mockResolvedValue(undefined);
  });

  it("passes raw filesystem paths through to the native path opener", async () => {
    await dispatchNotificationAction("notification-1", {
      id: "open-path",
      label: "Open",
      primary: true,
      kind: { type: "openPath", path: "/repo/file.txt" },
    });

    expect(openPathInFinder).toHaveBeenCalledWith("/repo/file.txt");
    expect(notificationsMarkRead).toHaveBeenCalledWith("notification-1");
  });

  it("normalizes file URLs before using the native path opener", async () => {
    await dispatchNotificationAction("notification-1", {
      id: "open-path",
      label: "Open",
      primary: true,
      kind: { type: "openPath", path: "file:///repo/My%20File.txt" },
    });

    expect(openPathInFinder).toHaveBeenCalledWith("/repo/My File.txt");
  });

  it("normalizes Windows file URLs without a leading slash before the drive", async () => {
    await dispatchNotificationAction("notification-1", {
      id: "open-path",
      label: "Open",
      primary: true,
      kind: { type: "openPath", path: "file:///C:/Users/Sam/file.txt" },
    });

    expect(openPathInFinder).toHaveBeenCalledWith("C:/Users/Sam/file.txt");
  });
});
