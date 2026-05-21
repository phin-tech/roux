import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { writable } from "svelte/store";

import { DEFAULT_SETTINGS } from "$lib/types";

const mocks = vi.hoisted(() => {
  return {
    settingsStore: null as unknown as ReturnType<typeof writable<unknown>>,
    terminalLoadAddon: vi.fn(),
    terminalDispose: vi.fn(),
    terminalOpen: vi.fn(),
    fitAddonFit: vi.fn(),
    fitAddonProposeDimensions: vi.fn(),
    webglConstructor: vi.fn(),
    webglDispose: vi.fn(),
    webglOnContextLoss: vi.fn(),
    webglClearTextureAtlas: vi.fn(),
    webglOnAddTextureAtlasCanvas: vi.fn(),
    contextLossSubDispose: vi.fn(),
    atlasAddSubDispose: vi.fn(),
    nextWebglShouldThrow: false,
    lastWebglContextLossHandler: null as (() => void) | null,
    lastWebglAtlasAddHandler: null as (() => void) | null,
  };
});

// hoisted block can't import the writable factory cleanly across all setups;
// initialize the store after hoist using a top-level statement that runs
// before the dynamic import of the module under test.
const { writable: makeStore } = await import("svelte/store");
mocks.settingsStore = makeStore({ ...DEFAULT_SETTINGS });

vi.mock("$lib/stores/settings", () => ({
  get settings() {
    return mocks.settingsStore;
  },
}));

vi.mock("$lib/stores/userTerminalThemes", async () => {
  const { writable } = await import("svelte/store");
  return { userTerminalThemes: writable([]) };
});

vi.mock("$lib/themes", () => ({
  resolveTerminalTheme: () => ({
    background: "#000",
    foreground: "#fff",
    cursor: "#fff",
    selectionBackground: "#444",
    ansi: {
      black: "#000", red: "#f00", green: "#0f0", yellow: "#ff0",
      blue: "#00f", magenta: "#f0f", cyan: "#0ff", white: "#fff",
      brightBlack: "#888", brightRed: "#f88", brightGreen: "#8f8", brightYellow: "#ff8",
      brightBlue: "#88f", brightMagenta: "#f8f", brightCyan: "#8ff", brightWhite: "#fff",
    },
  }),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

vi.mock("../xtermWatchDecorations", () => ({
  installXtermWatchDecorations: vi.fn(),
}));

vi.mock("../promptSnapshot", () => ({
  readPromptSnapshot: vi.fn().mockReturnValue(null),
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn().mockImplementation(function (this: Record<string, unknown>, options: Record<string, unknown>) {
    this.loadAddon = mocks.terminalLoadAddon;
    this.dispose = mocks.terminalDispose;
    this.options = { ...options };
    this.buffer = { active: { length: 0 } };
    this.onData = vi.fn().mockReturnValue({ dispose: vi.fn() });
    this.attachCustomKeyEventHandler = vi.fn();
    this.focus = vi.fn();
    this.input = vi.fn();
    this.paste = vi.fn();
    this.write = vi.fn();
    this.clear = vi.fn();
    this.reset = vi.fn();
    this.clearSelection = vi.fn();
    this.scrollToBottom = vi.fn();
    this.open = mocks.terminalOpen;
    this.element = null;
  }),
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    this.fit = mocks.fitAddonFit;
    this.proposeDimensions = mocks.fitAddonProposeDimensions;
  }),
}));

vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: vi.fn().mockImplementation(function () {}),
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    mocks.webglConstructor();
    if (mocks.nextWebglShouldThrow) {
      throw new Error("WebGL unavailable");
    }
    this.dispose = mocks.webglDispose;
    this.clearTextureAtlas = mocks.webglClearTextureAtlas;
    this.onContextLoss = (handler: () => void) => {
      mocks.lastWebglContextLossHandler = handler;
      mocks.webglOnContextLoss(handler);
      return { dispose: mocks.contextLossSubDispose };
    };
    this.onAddTextureAtlasCanvas = (handler: () => void) => {
      mocks.lastWebglAtlasAddHandler = handler;
      mocks.webglOnAddTextureAtlasCanvas(handler);
      return { dispose: mocks.atlasAddSubDispose };
    };
  }),
}));

const { createXtermTerminalController } = await import("../xtermController");

beforeEach(() => {
  mocks.terminalLoadAddon.mockClear();
  mocks.terminalDispose.mockClear();
  mocks.terminalOpen.mockClear();
  mocks.fitAddonFit.mockClear();
  mocks.fitAddonProposeDimensions.mockReset().mockReturnValue(null);
  mocks.webglConstructor.mockClear();
  mocks.webglDispose.mockClear();
  mocks.webglOnContextLoss.mockClear();
  mocks.webglClearTextureAtlas.mockClear();
  mocks.webglOnAddTextureAtlasCanvas.mockClear();
  mocks.contextLossSubDispose.mockClear();
  mocks.atlasAddSubDispose.mockClear();
  mocks.lastWebglContextLossHandler = null;
  mocks.lastWebglAtlasAddHandler = null;
  mocks.nextWebglShouldThrow = false;
  mocks.settingsStore.set({ ...DEFAULT_SETTINGS });
});

describe("XtermTerminalController renderer setup", () => {
  it("loads WebglAddon when gpuAcceleration is 'auto'", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    createXtermTerminalController();

    expect(mocks.webglConstructor).toHaveBeenCalledTimes(1);
    expect(mocks.webglOnContextLoss).toHaveBeenCalledTimes(1);
  });

  it("attaches without scheduling its own fit", () => {
    const raf = vi.fn();
    vi.stubGlobal("requestAnimationFrame", raf);
    const controller = createXtermTerminalController();
    const container = document.createElement("div");

    controller.attach(container);

    expect(mocks.terminalOpen).toHaveBeenCalledWith(container);
    expect(raf).not.toHaveBeenCalled();
    expect(mocks.fitAddonFit).not.toHaveBeenCalled();

    // dispose() cancels the throttled atlas-refresh timer attach() schedules,
    // so it cannot leak into a later test.
    controller.dispose();
  });

  it("clears the WebGL texture atlas after a successful fit", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    mocks.fitAddonProposeDimensions.mockReturnValue({ cols: 120, rows: 30 });

    const controller = createXtermTerminalController();
    const dims = controller.fit();

    expect(dims).toEqual({ cols: 120, rows: 30 });
    expect(mocks.fitAddonFit).toHaveBeenCalledTimes(1);
    expect(mocks.webglClearTextureAtlas).toHaveBeenCalledTimes(1);
  });

  it("does not clear the WebGL texture atlas when fit cannot propose dimensions", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    mocks.fitAddonProposeDimensions.mockReturnValue(null);

    const controller = createXtermTerminalController();
    const dims = controller.fit();

    expect(dims).toBeNull();
    expect(mocks.fitAddonFit).toHaveBeenCalledTimes(1);
    expect(mocks.webglClearTextureAtlas).not.toHaveBeenCalled();
  });

  it("loads WebglAddon when gpuAcceleration is 'on'", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "on" });

    createXtermTerminalController();

    expect(mocks.webglConstructor).toHaveBeenCalledTimes(1);
  });

  it("does NOT load WebglAddon when gpuAcceleration is 'off'", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "off" });

    createXtermTerminalController();

    expect(mocks.webglConstructor).not.toHaveBeenCalled();
  });

  it("treats missing gpuAcceleration as 'auto'", () => {
    const settings: Record<string, unknown> = { ...DEFAULT_SETTINGS };
    delete settings.gpuAcceleration;
    mocks.settingsStore.set(settings);

    createXtermTerminalController();

    expect(mocks.webglConstructor).toHaveBeenCalledTimes(1);
  });

  it("survives WebglAddon construction failure", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    mocks.nextWebglShouldThrow = true;

    expect(() => createXtermTerminalController()).not.toThrow();
    expect(mocks.webglConstructor).toHaveBeenCalledTimes(1);
    expect(mocks.webglOnContextLoss).not.toHaveBeenCalled();
  });

  it("disposes the WebglAddon when context is lost", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    createXtermTerminalController();
    expect(mocks.lastWebglContextLossHandler).not.toBeNull();

    mocks.lastWebglContextLossHandler?.();

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
  });

  it("disposes the WebglAddon on controller dispose", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    const controller = createXtermTerminalController();
    controller.dispose();

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
    expect(mocks.terminalDispose).toHaveBeenCalledTimes(1);
  });

  it("does not double-dispose the WebglAddon if context loss preceded controller dispose", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    const controller = createXtermTerminalController();
    mocks.lastWebglContextLossHandler?.();
    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);

    controller.dispose();

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
    expect(mocks.terminalDispose).toHaveBeenCalledTimes(1);
  });

  it("disposes the onContextLoss subscription when the controller is disposed", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    const controller = createXtermTerminalController();
    expect(mocks.contextLossSubDispose).not.toHaveBeenCalled();

    controller.dispose();

    expect(mocks.contextLossSubDispose).toHaveBeenCalledTimes(1);
  });

  it("ignores a context-loss event that fires after the controller is disposed", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    const controller = createXtermTerminalController();
    controller.dispose();
    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);

    // Simulate a stray late event despite the subscription being disposed.
    mocks.lastWebglContextLossHandler?.();

    // The handler's guard (`this.webglAddon !== webgl`) drops the late event,
    // so no second dispose() call lands on the WebGL addon.
    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
  });

  it("only disposes the WebglAddon once if onContextLoss fires twice", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    createXtermTerminalController();
    mocks.lastWebglContextLossHandler?.();
    mocks.lastWebglContextLossHandler?.();

    expect(mocks.webglDispose).toHaveBeenCalledTimes(1);
  });

  it("forwards viewport preparation calls to xterm", async () => {
    const { Terminal } = await import("@xterm/xterm");
    const controller = createXtermTerminalController();
    const terminal = vi.mocked(Terminal).mock.results.at(-1)?.value as {
      clearSelection: ReturnType<typeof vi.fn>;
      scrollToBottom: ReturnType<typeof vi.fn>;
    };

    controller.clearSelection();
    controller.scrollToBottom();

    expect(terminal.clearSelection).toHaveBeenCalledTimes(1);
    expect(terminal.scrollToBottom).toHaveBeenCalledTimes(1);
  });

  it("forwards input and paste calls through xterm", async () => {
    const { Terminal } = await import("@xterm/xterm");
    const controller = createXtermTerminalController();
    const terminal = vi.mocked(Terminal).mock.results.at(-1)?.value as {
      input: ReturnType<typeof vi.fn>;
      paste: ReturnType<typeof vi.fn>;
    };

    controller.input("\r");
    controller.paste("echo hi");

    expect(terminal.input).toHaveBeenCalledWith("\r", undefined);
    expect(terminal.paste).toHaveBeenCalledWith("echo hi");
  });

  it("gates keyboard events without disabling terminal protocol responses", async () => {
    const { Terminal } = await import("@xterm/xterm");
    const controller = createXtermTerminalController();
    const terminal = vi.mocked(Terminal).mock.results.at(-1)?.value as {
      options: Record<string, unknown>;
      attachCustomKeyEventHandler: ReturnType<typeof vi.fn>;
    };
    const keyHandler = terminal.attachCustomKeyEventHandler.mock.calls[0]?.[0] as
      | ((event: KeyboardEvent) => boolean)
      | undefined;
    expect(keyHandler).toBeDefined();
    expect(terminal.options.disableStdin).toBe(false);

    const event = new KeyboardEvent("keydown", { key: "a" });
    controller.setInputEnabled(false);
    expect(keyHandler!(event)).toBe(false);
    expect(terminal.options.disableStdin).toBe(false);

    controller.setInputEnabled(true);
    expect(keyHandler!(event)).toBe(true);
  });
});

describe("XtermTerminalController WebGL atlas refresh", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("subscribes to onAddTextureAtlasCanvas when WebGL is active", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });

    createXtermTerminalController();

    expect(mocks.webglOnAddTextureAtlasCanvas).toHaveBeenCalledTimes(1);
  });

  it("does not subscribe to onAddTextureAtlasCanvas when gpuAcceleration is 'off'", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "off" });

    createXtermTerminalController();

    expect(mocks.webglOnAddTextureAtlasCanvas).not.toHaveBeenCalled();
  });

  it("clears the texture atlas on a throttled delay after a page is added", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    createXtermTerminalController();

    mocks.lastWebglAtlasAddHandler?.();
    // Throttled: nothing happens synchronously on the page-add event.
    expect(mocks.webglClearTextureAtlas).not.toHaveBeenCalled();

    vi.advanceTimersByTime(250);
    expect(mocks.webglClearTextureAtlas).toHaveBeenCalledTimes(1);
  });

  it("coalesces a burst of page-add events into a single clear", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    createXtermTerminalController();

    for (let i = 0; i < 5; i++) {
      mocks.lastWebglAtlasAddHandler?.();
    }

    vi.advanceTimersByTime(250);
    expect(mocks.webglClearTextureAtlas).toHaveBeenCalledTimes(1);
  });

  it("schedules a throttled atlas refresh on attach", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    const controller = createXtermTerminalController();

    controller.attach(document.createElement("div"));
    expect(mocks.webglClearTextureAtlas).not.toHaveBeenCalled();

    vi.advanceTimersByTime(250);
    expect(mocks.webglClearTextureAtlas).toHaveBeenCalledTimes(1);
  });

  it("cancels a pending atlas refresh when the controller is disposed", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    const controller = createXtermTerminalController();

    mocks.lastWebglAtlasAddHandler?.();
    controller.dispose();

    expect(() => vi.advanceTimersByTime(250)).not.toThrow();
    expect(mocks.webglClearTextureAtlas).not.toHaveBeenCalled();
  });

  it("disposes the onAddTextureAtlasCanvas subscription on dispose", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    const controller = createXtermTerminalController();

    expect(mocks.atlasAddSubDispose).not.toHaveBeenCalled();

    controller.dispose();

    expect(mocks.atlasAddSubDispose).toHaveBeenCalledTimes(1);
  });

  it("ignores a page-add event that fires after the controller is disposed", () => {
    mocks.settingsStore.set({ ...DEFAULT_SETTINGS, gpuAcceleration: "auto" });
    const controller = createXtermTerminalController();
    controller.dispose();

    expect(() => mocks.lastWebglAtlasAddHandler?.()).not.toThrow();
    vi.advanceTimersByTime(250);
    expect(mocks.webglClearTextureAtlas).not.toHaveBeenCalled();
  });
});
