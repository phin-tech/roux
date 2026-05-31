import { get, writable } from "svelte/store";
import { beforeEach, describe, expect, it, vi } from "vitest";

const queryMock = vi.hoisted(() => ({
  activeSession: null as { id: string } | null,
}));

vi.mock("$lib/queries", () => ({
  queries: {
    activeSession: () => queryMock.activeSession,
    activeSessionId: () => queryMock.activeSession?.id ?? null,
  },
}));

vi.mock("$lib/bindings", () => ({
  commands: {},
}));

vi.mock("$lib/tauri", () => ({
  openInEditor: vi.fn(),
  notificationsPush: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

vi.mock("$lib/logging", () => ({
  logError: vi.fn(),
}));

vi.mock("$lib/themes", () => ({
  THEME_DEFINITIONS: [],
  MATCH_GUI_TERMINAL_THEME_ID: "auto",
  getAllTerminalThemeDefinitions: vi.fn(() => []),
}));

vi.mock("$lib/stores/userTerminalThemes", () => ({
  userTerminalThemes: writable([]),
  loadUserTerminalThemes: vi.fn(),
}));

vi.mock("$lib/stores/mainView", () => ({
  toggleMainView: vi.fn(),
}));

import { registry } from "../registry";
import { registerUiCommands } from "../ui";
import { toggleMainView } from "$lib/stores/mainView";
import {
  closePrStatusDetails,
  prStatusDetailsOpen,
} from "$lib/stores/prStatusDetails";

describe("ui commands", () => {
  beforeEach(() => {
    registry.unregister("ui.toggle-board");
    registry.unregister("ui.toggle-pr-status-details");
    queryMock.activeSession = null;
    vi.clearAllMocks();
    closePrStatusDetails();
  });

  it("registers a board main-view toggle command", () => {
    registerUiCommands();

    const command = registry.get("ui.toggle-board");
    expect(command?.label).toBe("Toggle Board Main View");
    expect(command?.category).toBe("App");

    command?.execute?.();
    expect(toggleMainView).toHaveBeenCalledWith({ kind: "board" });
  });

  it("registers a bindable PR status details toggle command", () => {
    queryMock.activeSession = { id: "s1" };
    registerUiCommands();

    const command = registry.get("ui.toggle-pr-status-details");
    expect(command?.label).toBe("Toggle PR Status Details");
    expect(command?.category).toBe("Watches");
    expect(command?.available?.()).toBe(true);

    command?.execute?.();
    expect(get(prStatusDetailsOpen)).toBe(true);

    command?.execute?.();
    expect(get(prStatusDetailsOpen)).toBe(false);
  });

  it("hides the PR status details toggle command without an active session", () => {
    registerUiCommands();

    expect(registry.get("ui.toggle-pr-status-details")?.available?.()).toBe(false);
    expect(registry.getAvailable().map((cmd) => cmd.id)).not.toContain(
      "ui.toggle-pr-status-details",
    );
  });
});
