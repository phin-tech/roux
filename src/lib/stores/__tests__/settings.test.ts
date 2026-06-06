import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { DEFAULT_SETTINGS } from "$lib/types";

const tauriMock = vi.hoisted(() => ({
  getSettings: vi.fn(),
  onSettingsChanged: vi.fn(),
  updateSettings: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/tauri", () => tauriMock);
vi.mock("$lib/stores/worktrunkDetection", () => ({
  refreshWorktrunkDetection: vi.fn(),
}));

import {
  setDefaultAgentProfile,
  setStartupTarget,
  settings,
} from "../settings";

describe("settings actions", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauriMock.updateSettings.mockClear();
    settings.set({ ...DEFAULT_SETTINGS });
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("updates the global default agent without mutating Kanban workflow settings", async () => {
    setDefaultAgentProfile("codex");

    expect(get(settings).defaultAgentProfile).toBe("codex");
    expect(get(settings).kanban?.workflow?.id).toBe("default");

    await vi.runAllTimersAsync();
    expect(tauriMock.updateSettings).toHaveBeenCalledTimes(1);
    expect(tauriMock.updateSettings.mock.calls[0][0].defaultAgentProfile).toBe(
      "codex",
    );
    expect(tauriMock.updateSettings.mock.calls[0][0].kanban?.workflow?.id).toBe(
      "default",
    );
  });

  it("updates startup target and legacy Kanban launch state in one draft", async () => {
    settings.set({
      ...DEFAULT_SETTINGS,
      startupTarget: "kanbanWide",
      kanban: { ...DEFAULT_SETTINGS.kanban, startupSidebar: "kanban" },
    });

    setStartupTarget("restore");

    expect(get(settings).startupTarget).toBe("restore");
    expect(get(settings).startupExternalToolId).toBeNull();
    expect(get(settings).kanban?.startupSidebar).toBe("restore");

    await vi.runAllTimersAsync();
    expect(tauriMock.updateSettings).toHaveBeenCalledTimes(1);
    expect(tauriMock.updateSettings.mock.calls[0][0].startupTarget).toBe(
      "restore",
    );
    expect(
      tauriMock.updateSettings.mock.calls[0][0].kanban?.startupSidebar,
    ).toBe("restore");
  });
});
