import { beforeEach, describe, expect, it, vi } from "vitest";
import { writable } from "svelte/store";

import { DEFAULT_SETTINGS } from "$lib/types";

const mocks = vi.hoisted(() => {
  return {
    settingsStore: null as unknown as ReturnType<typeof writable<unknown>>,
    terminalLoadAddon: vi.fn(),
    terminalDispose: vi.fn(),
    webglConstructor: vi.fn(),
    webglDispose: vi.fn(),
    webglOnContextLoss: vi.fn(),
    nextWebglShouldThrow: false,
    lastWebglContextLossHandler: null as (() => void) | null,
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
  Terminal: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    this.loadAddon = mocks.terminalLoadAddon;
    this.dispose = mocks.terminalDispose;
    this.options = {};
    this.buffer = { active: { length: 0 } };
    this.onData = vi.fn().mockReturnValue({ dispose: vi.fn() });
    this.attachCustomKeyEventHandler = vi.fn();
    this.focus = vi.fn();
    this.write = vi.fn();
    this.clear = vi.fn();
    this.reset = vi.fn();
    this.open = vi.fn();
    this.element = null;
  }),
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    this.fit = vi.fn();
    this.proposeDimensions = vi.fn().mockReturnValue(null);
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
    this.onContextLoss = (handler: () => void) => {
      mocks.lastWebglContextLossHandler = handler;
      mocks.webglOnContextLoss(handler);
      return { dispose: vi.fn() };
    };
  }),
}));

const { createXtermTerminalController } = await import("../xtermController");

beforeEach(() => {
  mocks.terminalLoadAddon.mockClear();
  mocks.terminalDispose.mockClear();
  mocks.webglConstructor.mockClear();
  mocks.webglDispose.mockClear();
  mocks.webglOnContextLoss.mockClear();
  mocks.lastWebglContextLossHandler = null;
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
});
