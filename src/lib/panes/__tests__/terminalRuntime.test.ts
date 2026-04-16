import { get } from "svelte/store";
import { beforeEach, describe, expect, it, vi } from "vitest";

const factory = vi.fn();

import {
  clearPaneOutputChannel,
  disposePaneTerminalRuntime,
  ensureTerminalController,
  getPaneOutputChannel,
  getTerminalController,
  registerTerminalControllerFactory,
  resetPaneTerminalRuntimes,
  setPaneOutputChannel,
  terminalRuntimeVersion,
} from "../terminalRuntime";

describe("terminalRuntime", () => {
  beforeEach(() => {
    resetPaneTerminalRuntimes();
    factory.mockReset();
    factory.mockImplementation(() => ({
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
    }));
    registerTerminalControllerFactory((options) => factory(options));
  });

  it("stores an output channel without eagerly constructing a terminal controller", () => {
    const outputChannel = { id: "out-1" } as never;

    setPaneOutputChannel("pane-1", outputChannel);

    expect(getPaneOutputChannel("pane-1")).toBe(outputChannel);
    expect(getTerminalController("pane-1")).toBeNull();
    expect(factory).not.toHaveBeenCalled();
  });

  it("creates the controller lazily and preserves any existing output channel", () => {
    const outputChannel = { id: "out-1" } as never;
    setPaneOutputChannel("pane-1", outputChannel);
    const beforeVersion = get(terminalRuntimeVersion);

    const controller = ensureTerminalController("pane-1", {
      allowKeyboardEvent: vi.fn(),
    });

    expect(controller).toBe(getTerminalController("pane-1"));
    expect(getPaneOutputChannel("pane-1")).toBe(outputChannel);
    expect(factory).toHaveBeenCalledTimes(1);
    expect(get(terminalRuntimeVersion)).toBe(beforeVersion + 1);
  });

  it("clears channels and disposes controllers independently", () => {
    const outputChannel = { id: "out-1" } as never;
    setPaneOutputChannel("pane-1", outputChannel);
    const controller = ensureTerminalController("pane-1");
    const beforeDisposeVersion = get(terminalRuntimeVersion);

    clearPaneOutputChannel("pane-1");
    expect(getPaneOutputChannel("pane-1")).toBeNull();

    disposePaneTerminalRuntime("pane-1");
    expect(controller.dispose).toHaveBeenCalledTimes(1);
    expect(getTerminalController("pane-1")).toBeNull();
    expect(getPaneOutputChannel("pane-1")).toBeNull();
    expect(get(terminalRuntimeVersion)).toBe(beforeDisposeVersion + 1);
  });

  it("does not change the runtime version for output-channel-only updates", () => {
    const beforeVersion = get(terminalRuntimeVersion);

    setPaneOutputChannel("pane-1", { id: "out-1" } as never);
    clearPaneOutputChannel("pane-1");

    expect(get(terminalRuntimeVersion)).toBe(beforeVersion);
  });
});
