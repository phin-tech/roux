import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const controller = {
  clear: vi.fn(),
  reset: vi.fn(),
};

vi.mock("$lib/tauri", () => ({
  killPty: vi.fn().mockResolvedValue(undefined),
  spawnTask: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/terminals", () => ({
  connectPaneTerminal: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/panes/terminalRuntime", () => ({
  getTerminalController: vi.fn(() => controller),
  clearPaneOutputChannel: vi.fn(),
  disposePaneTerminalRuntime: vi.fn(),
}));

import { killPty, spawnTask } from "$lib/tauri";
import { connectPaneTerminal } from "$lib/panes/terminals";
import { clearPaneOutputChannel } from "$lib/panes/terminalRuntime";
import {
  createPane,
  getInstance,
  paneInstances,
  resetInstances,
  updateInstance,
} from "../instances";
import { rerunCommandPane } from "../commandPaneRuntime";

describe("commandPaneRuntime", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    controller.clear.mockReset();
    controller.reset.mockReset();
    resetInstances();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-04-15T13:00:00Z"));
  });

  it("reruns a command pane through the runtime boundary", async () => {
    const paneId = createPane({
      id: "cmd-1",
      type: "command",
      ptyId: "pty-old",
      command: "npm test",
      workingDir: "/repo",
    });
    const unlistenA = vi.fn();
    const unlistenB = vi.fn();
    const timer = setInterval(() => {}, 1000);

    updateInstance(paneId, {
      commandStatus: "running",
      commandStartedAt: 1,
      elapsedTimer: timer,
      unlisteners: [unlistenA, unlistenB],
    });

    const callOrder: string[] = [];
    vi.mocked(connectPaneTerminal).mockImplementation(async () => {
      callOrder.push("attach");
    });
    vi.mocked(spawnTask).mockImplementation(async () => {
      callOrder.push("spawn");
    });

    await rerunCommandPane(paneId, "session-1");

    expect(killPty).toHaveBeenCalledWith("pty-old");
    expect(unlistenA).toHaveBeenCalledTimes(1);
    expect(unlistenB).toHaveBeenCalledTimes(1);
    expect(clearPaneOutputChannel).toHaveBeenCalledWith(paneId);
    expect(controller.clear).toHaveBeenCalledTimes(1);
    expect(controller.reset).toHaveBeenCalledTimes(1);
    expect(callOrder).toEqual(["attach", "spawn"]);

    const inst = getInstance(paneId)!;
    expect(inst.ptyId).toBe("cmd-1-1776258000000");
    expect(inst.commandStatus).toBe("running");
    expect(inst.commandExitCode).toBeNull();
    expect(inst.commandStartedAt).toBe(1776258000000);
    expect(inst.unlisteners).toEqual([]);
    expect(inst.elapsedTimer).not.toBeNull();
    expect(spawnTask).toHaveBeenCalledWith(
      "cmd-1-1776258000000",
      "npm test",
      "/repo",
      "session-1",
      paneId,
    );
  });

  it("updates completion state from the attached exit callback", async () => {
    const paneId = createPane({
      id: "cmd-1",
      type: "command",
      ptyId: "pty-old",
      command: "npm test",
      workingDir: "/repo",
    });

    await rerunCommandPane(paneId, "session-1");

    const onExit = vi.mocked(connectPaneTerminal).mock.calls[0]?.[1];
    expect(onExit).toBeTypeOf("function");

    onExit?.({ code: 7 } as { code: number });

    const inst = get(paneInstances).get(paneId)!;
    expect(inst.commandStatus).toBe("error");
    expect(inst.commandExitCode).toBe(7);
    expect(inst.elapsedTimer).toBeNull();
  });

  it("is a no-op when the pane is missing command metadata", async () => {
    const paneId = createPane({
      id: "cmd-1",
      type: "command",
      ptyId: "pty-old",
    });

    await rerunCommandPane(paneId, "session-1");

    expect(killPty).not.toHaveBeenCalled();
    expect(connectPaneTerminal).not.toHaveBeenCalled();
    expect(spawnTask).not.toHaveBeenCalled();
  });
});
