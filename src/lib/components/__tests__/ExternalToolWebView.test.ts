import { render, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExternalToolRun } from "$lib/stores/externalTools";

const webviewMock = vi.hoisted(() => {
  class MockWebview {
    static instances: MockWebview[] = [];
    static getByLabel = vi.fn(async (label: string) => {
      return MockWebview.instances.find((instance) => instance.label === label) ?? null;
    });

    label: string;
    options: unknown;
    setPosition = vi.fn().mockResolvedValue(undefined);
    setSize = vi.fn().mockResolvedValue(undefined);
    hide = vi.fn().mockResolvedValue(undefined);
    close = vi.fn().mockResolvedValue(undefined);
    once = vi.fn((event: string, handler: () => void) => {
      if (event === "tauri://created") queueMicrotask(handler);
      return Promise.resolve(() => {});
    });

    constructor(_window: unknown, label: string, options: unknown) {
      this.label = label;
      this.options = options;
      MockWebview.instances.push(this);
    }
  }

  return { MockWebview };
});

const tauriMock = vi.hoisted(() => ({
  probeExternalToolUrl: vi.fn(),
}));

const windowMock = vi.hoisted(() => {
  const state = {
    resizeHandler: null as (() => void) | null,
    scaleHandler: null as (() => void) | null,
    unlistenResize: vi.fn(),
    unlistenScale: vi.fn(),
    onResized: vi.fn(async (handler: () => void) => {
      state.resizeHandler = handler;
      return state.unlistenResize;
    }),
    onScaleChanged: vi.fn(async (handler: () => void) => {
      state.scaleHandler = handler;
      return state.unlistenScale;
    }),
  };
  return state;
});

const externalToolsMock = vi.hoisted(() => ({
  failExternalToolRun: vi.fn().mockResolvedValue(undefined),
  markExternalToolExited: vi.fn(),
  markExternalToolReady: vi.fn(),
  readExternalToolProcess: vi.fn().mockResolvedValue(null),
  registerExternalToolViewCloser: vi.fn((_runId: string, _closeView: () => void) => () => {}),
  restartExternalToolRun: vi.fn(),
  setExternalToolRunError: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: webviewMock.MockWebview,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    label: "main",
    onResized: windowMock.onResized,
    onScaleChanged: windowMock.onScaleChanged,
  })),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
  LogicalSize: class LogicalSize {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
}));

vi.mock("$lib/tauri", () => ({
  probeExternalToolUrl: tauriMock.probeExternalToolUrl,
}));

vi.mock("$lib/stores/externalTools", () => externalToolsMock);

import ExternalToolWebView from "../ExternalToolWebView.svelte";

let frameCallbacks: FrameRequestCallback[] = [];

class ResizeObserverStub {
  observe = vi.fn();
  disconnect = vi.fn();
}

function flushAnimationFrames(): void {
  const callbacks = frameCallbacks.splice(0);
  for (const callback of callbacks) callback(0);
}

function makeRun(): ExternalToolRun {
  return {
    id: "difit:session-1",
    toolId: "difit",
    toolName: "Difit",
    surface: "web",
    webEmbedder: "webview",
    sessionId: "session-1",
    runtimeId: "process-1",
    runtimeGeneration: null,
    rendered: {
      command: "difit --port 4966",
      cwd: "/repo",
      port: 4966,
      url: "http://127.0.0.1:4966",
    },
    status: "starting",
    error: null,
    exitCode: null,
    logsOpen: false,
    launchedAtMs: 100,
  };
}

describe("ExternalToolWebView", () => {
  beforeEach(() => {
    webviewMock.MockWebview.instances = [];
    webviewMock.MockWebview.getByLabel.mockClear();
    windowMock.resizeHandler = null;
    windowMock.scaleHandler = null;
    frameCallbacks = [];
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
      x: 12,
      y: 44,
      left: 12,
      top: 44,
      right: 812,
      bottom: 644,
      width: 800,
      height: 600,
      toJSON: () => ({}),
    } as DOMRect);
    tauriMock.probeExternalToolUrl.mockResolvedValue(true);
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frameCallbacks.push(callback);
      return frameCallbacks.length;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("marks the child webview ready and syncs bounds after creation", async () => {
    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const webview = webviewMock.MockWebview.instances[0];

    expect(webview.setPosition).toHaveBeenCalled();
    expect(webview.setSize).toHaveBeenCalled();
    expect(externalToolsMock.markExternalToolReady).toHaveBeenCalledWith("difit:session-1");

    unmount();
  });

  it("registers a closer that closes the child webview", async () => {
    const closeView = { current: null as (() => void) | null };
    const unregister = vi.fn();
    externalToolsMock.registerExternalToolViewCloser.mockImplementationOnce((_, nextCloseView) => {
      closeView.current = nextCloseView;
      return unregister;
    });
    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const webview = webviewMock.MockWebview.instances[0];
    const registeredCloseView = closeView.current;
    if (!registeredCloseView) throw new Error("Expected webview closer to be registered");
    registeredCloseView();

    expect(webview.close).toHaveBeenCalledOnce();

    unmount();
    expect(unregister).toHaveBeenCalledOnce();
  });

  it("cleans up the runtime when startup probing fails", async () => {
    tauriMock.probeExternalToolUrl.mockRejectedValueOnce(new Error("probe denied"));

    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() =>
      expect(externalToolsMock.failExternalToolRun).toHaveBeenCalledWith(
        "difit:session-1",
        "process-1",
        "Failed to check http://127.0.0.1:4966: probe denied",
      ),
    );
    expect(externalToolsMock.setExternalToolRunError).not.toHaveBeenCalled();

    unmount();
  });

  it("resyncs child webview bounds when the parent window resizes", async () => {
    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const webview = webviewMock.MockWebview.instances[0];
    await waitFor(() => expect(webview.setSize).toHaveBeenCalled());
    flushAnimationFrames();
    const sizeCalls = webview.setSize.mock.calls.length;

    windowMock.resizeHandler?.();
    flushAnimationFrames();

    await waitFor(() => expect(webview.setSize.mock.calls.length).toBeGreaterThan(sizeCalls));

    unmount();
    expect(windowMock.unlistenResize).toHaveBeenCalled();
    expect(windowMock.unlistenScale).toHaveBeenCalled();
  });

  it("removes the window resize listener if registration resolves after destroy", async () => {
    let resolveResizeListener: (unlisten: typeof windowMock.unlistenResize) => void = () => {};
    windowMock.onResized.mockImplementationOnce(async (handler: () => void) => {
      windowMock.resizeHandler = handler;
      return new Promise<typeof windowMock.unlistenResize>((resolve) => {
        resolveResizeListener = resolve;
      });
    });

    const { unmount } = render(ExternalToolWebView, { run: makeRun() });
    await waitFor(() => expect(windowMock.onResized).toHaveBeenCalled());

    unmount();
    resolveResizeListener(windowMock.unlistenResize);

    await waitFor(() => expect(windowMock.unlistenResize).toHaveBeenCalled());
  });

  it("resyncs child webview bounds when the parent window scale changes", async () => {
    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const webview = webviewMock.MockWebview.instances[0];
    await waitFor(() => expect(webview.setSize).toHaveBeenCalled());
    flushAnimationFrames();
    const sizeCalls = webview.setSize.mock.calls.length;

    windowMock.scaleHandler?.();
    flushAnimationFrames();

    await waitFor(() => expect(webview.setSize.mock.calls.length).toBeGreaterThan(sizeCalls));

    unmount();
  });
});
