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

  it("recreates a ready native webview when the component remounts", async () => {
    const readyRun = { ...makeRun(), status: "ready" as const };

    const first = render(ExternalToolWebView, { run: readyRun });
    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const closed = webviewMock.MockWebview.instances[0];
    first.unmount();
    expect(closed.close).toHaveBeenCalledOnce();

    const second = render(ExternalToolWebView, { run: readyRun });
    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(2));
    expect(webviewMock.MockWebview.instances[1].options).toMatchObject({
      url: "http://127.0.0.1:4966",
    });
    expect(tauriMock.probeExternalToolUrl).not.toHaveBeenCalled();

    second.unmount();
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

  it("ignores stale startup probe failures after a run relaunch", async () => {
    let rejectProbe: (err: Error) => void = () => {};
    tauriMock.probeExternalToolUrl.mockReturnValueOnce(
      new Promise<boolean>((_, reject) => {
        rejectProbe = reject;
      }),
    );
    const replacement = {
      ...makeRun(),
      runtimeId: "process-2",
      launchedAtMs: 200,
      rendered: {
        ...makeRun().rendered!,
        port: 4967,
        url: "http://127.0.0.1:4967",
      },
    };
    const { rerender, unmount } = render(ExternalToolWebView, { run: makeRun() });
    await waitFor(() => expect(tauriMock.probeExternalToolUrl).toHaveBeenCalled());

    await rerender({ run: replacement });
    rejectProbe(new Error("stale probe"));

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    expect(externalToolsMock.failExternalToolRun).not.toHaveBeenCalled();

    unmount();
  });

  it("adds the measured main-view toolbar inset to native webview bounds", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      if (this.hasAttribute("data-main-view-toolbar")) {
        return {
          x: 0,
          y: 0,
          left: 0,
          top: 0,
          right: 800,
          bottom: 36,
          width: 800,
          height: 36,
          toJSON: () => ({}),
        } as DOMRect;
      }
      return {
        x: 12,
        y: 44,
        left: 12,
        top: 44,
        right: 812,
        bottom: 644,
        width: 800,
        height: 600,
        toJSON: () => ({}),
      } as DOMRect;
    });
    const root = document.createElement("div");
    root.setAttribute("data-main-view-root", "");
    const toolbar = document.createElement("div");
    toolbar.setAttribute("data-main-view-toolbar", "");
    const target = document.createElement("div");
    root.append(toolbar, target);
    document.body.appendChild(root);

    const { unmount } = render(ExternalToolWebView, {
      target,
      props: { run: makeRun() },
    });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    expect(webviewMock.MockWebview.instances[0].options).toMatchObject({ y: 80 });

    unmount();
    root.remove();
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
