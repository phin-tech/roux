import { render, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExternalToolRun } from "$lib/stores/externalTools";

const webviewMock = vi.hoisted(() => {
  class MockWebview {
    static instances: MockWebview[] = [];

    options: unknown;
    setAutoResize = vi.fn().mockResolvedValue(undefined);
    setPosition = vi.fn().mockResolvedValue(undefined);
    setSize = vi.fn().mockResolvedValue(undefined);
    close = vi.fn().mockResolvedValue(undefined);
    once = vi.fn((event: string, handler: () => void) => {
      if (event === "tauri://created") queueMicrotask(handler);
      return Promise.resolve(() => {});
    });

    constructor(_window: unknown, _label: string, options: unknown) {
      this.options = options;
      MockWebview.instances.push(this);
    }
  }

  return { MockWebview };
});

const tauriMock = vi.hoisted(() => ({
  probeExternalToolUrl: vi.fn(),
}));

const externalToolsMock = vi.hoisted(() => ({
  failExternalToolRun: vi.fn().mockResolvedValue(undefined),
  markExternalToolExited: vi.fn(),
  markExternalToolReady: vi.fn(),
  readExternalToolProcess: vi.fn().mockResolvedValue(null),
  restartExternalToolRun: vi.fn(),
  setExternalToolRunError: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: webviewMock.MockWebview,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ label: "main" })),
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

class ResizeObserverStub {
  observe = vi.fn();
  disconnect = vi.fn();
}

function makeRun(): ExternalToolRun {
  return {
    id: "difit:session-1",
    toolId: "difit",
    toolName: "Difit",
    surface: "web",
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
    tauriMock.probeExternalToolUrl.mockResolvedValue(true);
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("enables native auto-resize and syncs bounds after child webview creation", async () => {
    const { unmount } = render(ExternalToolWebView, { run: makeRun() });

    await waitFor(() => expect(webviewMock.MockWebview.instances).toHaveLength(1));
    const webview = webviewMock.MockWebview.instances[0];

    await waitFor(() => expect(webview.setAutoResize).toHaveBeenCalledWith(true));
    expect(webview.setPosition).toHaveBeenCalled();
    expect(webview.setSize).toHaveBeenCalled();
    expect(externalToolsMock.markExternalToolReady).toHaveBeenCalledWith("difit:session-1");

    unmount();
  });
});
