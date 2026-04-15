import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const controller = {
    attach: vi.fn(),
    detach: vi.fn(),
    dispose: vi.fn(),
    clear: vi.fn(),
    reset: vi.fn(),
    fit: vi.fn().mockReturnValue(null),
    setInputEnabled: vi.fn(),
    onInput: vi.fn().mockReturnValue(() => {}),
    write: vi.fn(),
    focus: vi.fn(),
    setTheme: vi.fn(),
    setCustomKeyHandler: vi.fn(),
  };

  return {
    controller,
    getInstance: vi.fn(),
    ensureTerminalController: vi.fn(() => controller),
    getTerminalController: vi.fn(() => controller),
    getPaneOutputChannel: vi.fn(() => null),
    setPaneOutputChannel: vi.fn(),
    attachPtyOutput: vi.fn().mockResolvedValue(undefined),
    createPtyOutputChannel: vi.fn(() => ({ id: "channel" })),
  };
});

vi.mock("../instances", () => ({
  getInstance: mocks.getInstance,
}));

vi.mock("../terminalRuntime", () => ({
  ensureTerminalController: mocks.ensureTerminalController,
  getTerminalController: mocks.getTerminalController,
  getPaneOutputChannel: mocks.getPaneOutputChannel,
  setPaneOutputChannel: mocks.setPaneOutputChannel,
}));

vi.mock("$lib/tauri", () => ({
  attachPtyOutput: mocks.attachPtyOutput,
  createPtyOutputChannel: mocks.createPtyOutputChannel,
  onSessionExit: vi.fn().mockResolvedValue(() => {}),
  writeToSession: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/keymap/store", () => ({
  keymapState: {
    subscribe: (run: (value: unknown) => void) => {
      run({});
      return () => {};
    },
  },
}));

vi.mock("$lib/keymap/resolve", () => ({
  resolveKey: vi.fn(() => ({ kind: "none" })),
}));

vi.mock("$lib/commands", () => ({
  registry: new Map(),
}));

vi.mock("../focus", () => ({
  focusedPaneId: {
    subscribe: (run: (value: unknown) => void) => {
      run(null);
      return () => {};
    },
  },
}));

vi.mock("../ptyOutputBus", () => ({
  emitPtyOutput: vi.fn(),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
}));

import { connectPaneTerminal } from "../terminals";

describe("terminals", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.controller.onInput.mockReset().mockReturnValue(() => {});
    mocks.controller.setInputEnabled.mockReset();
    mocks.ensureTerminalController.mockReset().mockReturnValue(mocks.controller);
    mocks.getTerminalController.mockReset().mockReturnValue(null as never);
    mocks.getPaneOutputChannel.mockReset().mockReturnValue(null);
    mocks.setPaneOutputChannel.mockReset();
    mocks.attachPtyOutput.mockReset().mockResolvedValue(undefined);
    mocks.createPtyOutputChannel.mockReset().mockReturnValue({ id: "channel" });
    mocks.getInstance.mockReset().mockReturnValue({
      id: "pane-1",
      type: "shell",
      ptyId: "pty-1",
      unlisteners: [],
    });
  });

  it("connects a pane terminal by creating the controller before attaching PTY output", async () => {
    const callOrder: string[] = [];
    mocks.ensureTerminalController.mockImplementation(() => {
      callOrder.push("init");
      return mocks.controller;
    });
    mocks.attachPtyOutput.mockImplementation(async () => {
      callOrder.push("attach");
    });

    await connectPaneTerminal("pane-1");

    expect(callOrder).toEqual(["init", "attach"]);
    expect(mocks.controller.onInput).toHaveBeenCalledTimes(1);
    expect(mocks.controller.setInputEnabled).toHaveBeenCalledTimes(1);
    expect(mocks.createPtyOutputChannel).toHaveBeenCalledTimes(1);
    expect(mocks.setPaneOutputChannel).toHaveBeenCalledWith("pane-1", { id: "channel" });
    expect(mocks.attachPtyOutput).toHaveBeenCalledWith("pty-1", { id: "channel" });
  });
});
