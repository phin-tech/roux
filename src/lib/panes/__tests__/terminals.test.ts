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
    getPromptSnapshot: vi.fn().mockReturnValue(null),
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
    // Mutable so individual tests can simulate an armed leader tree
    // without re-mocking the whole module.
    keymapValue: { treePath: [] as string[] },
  };
});

vi.mock("../instances", () => ({
  getInstance: mocks.getInstance,
  getAttachedPtyId: vi.fn((pane: { ptyId?: string } | null | undefined) => pane?.ptyId ?? null),
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
      run(mocks.keymapValue);
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
    set: vi.fn(),
  },
}));

vi.mock("../ptyOutputBus", () => ({
  emitPtyOutput: vi.fn(),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
}));

import { connectPaneTerminal } from "../terminals";
import { registry as commandRegistry } from "$lib/commands";

describe("terminals", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // The mocked registry Map is module-scoped, so set() entries from
    // earlier tests leak into later ones. clearAllMocks doesn't touch
    // Map contents — clear it explicitly to keep tests order-independent.
    (commandRegistry as unknown as Map<string, unknown>).clear();
    mocks.keymapValue.treePath = [];
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

  describe("allowKeyboardEvent", () => {
    // The xterm `allowKeyboardEvent` callback is passed to
    // ensureTerminalController as part of the options object on the
    // very first init call. Re-running connectPaneTerminal between
    // assertions and pulling the callback off of mock.calls keeps each
    // case self-contained.
    type AllowKeyboardEvent = (event: KeyboardEvent) => boolean;

    async function captureAllowKeyboardEvent(): Promise<AllowKeyboardEvent> {
      mocks.ensureTerminalController.mockReset().mockReturnValue(mocks.controller);
      await connectPaneTerminal("pane-1");
      const calls = mocks.ensureTerminalController.mock.calls as unknown as Array<
        [string, { allowKeyboardEvent: AllowKeyboardEvent }]
      >;
      const opts = calls[0]?.[1];
      if (!opts?.allowKeyboardEvent) throw new Error("allowKeyboardEvent not registered");
      return opts.allowKeyboardEvent;
    }

    function makeKeyEvent(init: { key: string; defaultPrevented?: boolean }): KeyboardEvent {
      const event = new KeyboardEvent("keydown", { key: init.key, cancelable: true });
      if (init.defaultPrevented) event.preventDefault();
      return event;
    }

    it("returns true for non-keydown events without consulting the keymap", async () => {
      const allow = await captureAllowKeyboardEvent();
      const event = new KeyboardEvent("keyup", { key: "a" });
      expect(allow(event)).toBe(true);
    });

    it("forwards Escape to xterm even when defaultPrevented (App.svelte focus-blur fix)", async () => {
      // App.svelte:266 calls preventDefault on Escape unconditionally to
      // keep WebKit from blurring xterm's hidden textarea. A previous
      // blanket `defaultPrevented → false` short-circuit caused xterm
      // to refuse Escape entirely, breaking Claude TUI cancel and vim.
      const { resolveKey } = await import("$lib/keymap/resolve");
      vi.mocked(resolveKey).mockReturnValueOnce({ kind: "passthrough" });
      const allow = await captureAllowKeyboardEvent();
      const event = makeKeyEvent({ key: "Escape", defaultPrevented: true });
      expect(allow(event)).toBe(true);
    });

    it("rejects an editor-toggle chord that App.svelte already handled (no double-fire)", async () => {
      const { resolveKey } = await import("$lib/keymap/resolve");
      const { registry } = await import("$lib/commands");
      const execute = vi.fn();
      (registry as unknown as Map<string, { execute: () => void }>).set(
        "pane.open-multiline-editor",
        { execute },
      );
      vi.mocked(resolveKey).mockReturnValueOnce({
        kind: "chord",
        action: { kind: "command", id: "pane.open-multiline-editor" },
      } as never);
      const allow = await captureAllowKeyboardEvent();
      const event = makeKeyEvent({ key: "e", defaultPrevented: true });
      expect(allow(event)).toBe(false);
      expect(execute).not.toHaveBeenCalled();
    });

    it("fires the editor-toggle chord exactly once when App.svelte did not handle it first", async () => {
      const { resolveKey } = await import("$lib/keymap/resolve");
      const execute = vi.fn();
      (commandRegistry as unknown as Map<string, { execute: () => void }>).set(
        "pane.open-multiline-editor",
        { execute },
      );
      vi.mocked(resolveKey).mockReturnValueOnce({
        kind: "chord",
        action: { kind: "command", id: "pane.open-multiline-editor" },
      } as never);
      const allow = await captureAllowKeyboardEvent();
      const event = makeKeyEvent({ key: "e" });
      const preventDefault = vi.spyOn(event, "preventDefault");
      const stopPropagation = vi.spyOn(event, "stopPropagation");

      expect(allow(event)).toBe(false);
      expect(preventDefault).toHaveBeenCalledTimes(1);
      expect(stopPropagation).toHaveBeenCalledTimes(1);
      expect(execute).toHaveBeenCalledTimes(1);
    });

    it("blocks unbound keys while a leader tree is armed (App.svelte preventDefault'd them)", async () => {
      // Regression: a `defaultPrevented`-only-on-chord guard let
      // resolve.ts §1e's `none` resolutions through, leaking unbound
      // characters to the PTY mid-chord while the tree stayed armed.
      const { resolveKey } = await import("$lib/keymap/resolve");
      mocks.keymapValue.treePath = ["leader"];
      vi.mocked(resolveKey).mockReturnValueOnce({ kind: "none" } as never);
      const allow = await captureAllowKeyboardEvent();
      const event = makeKeyEvent({ key: "z", defaultPrevented: true });
      expect(allow(event)).toBe(false);
    });
  });
});
