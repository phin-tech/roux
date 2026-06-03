import { render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  externalToolRuns,
  type ExternalToolRun,
} from "$lib/stores/externalTools";

const tauriMock = vi.hoisted(() => ({
  daemonProcessKill: vi.fn().mockResolvedValue(undefined),
  daemonProcessOutput: vi.fn().mockResolvedValue(null),
  killPty: vi.fn().mockResolvedValue(undefined),
  launchExternalTool: vi.fn(),
  probeExternalToolUrl: vi.fn(),
}));

const windowMock = vi.hoisted(() => ({
  onResized: vi.fn(async () => () => {}),
  onScaleChanged: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: class Webview {
    static getByLabel = vi.fn().mockResolvedValue(null);
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
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
  daemonProcessKill: tauriMock.daemonProcessKill,
  daemonProcessOutput: tauriMock.daemonProcessOutput,
  killPty: tauriMock.killPty,
  launchExternalTool: tauriMock.launchExternalTool,
  probeExternalToolUrl: tauriMock.probeExternalToolUrl,
}));

import ExternalToolMainView from "../ExternalToolMainView.svelte";

class ResizeObserverStub {
  observe = vi.fn();
  disconnect = vi.fn();
}

function makeWebErrorRun(): ExternalToolRun {
  return {
    id: "difit:global",
    toolId: "difit",
    toolName: "Difit",
    surface: "web",
    webEmbedder: "webview",
    keepWebviewAlive: false,
    sessionId: null,
    runtimeId: "process-1",
    runtimeGeneration: null,
    rendered: {
      command: "difit --port 4966",
      cwd: "/repo",
      port: 4966,
      url: "http://127.0.0.1:4966",
    },
    status: "error",
    error: "Process exited with code 1",
    logsOpen: true,
    launchedAtMs: 100,
  };
}

describe("ExternalToolMainView", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
  });

  afterEach(() => {
    externalToolRuns.set(new Map());
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("keeps errored web tools in the web view so process logs remain visible", async () => {
    const run = makeWebErrorRun();
    externalToolRuns.set(new Map([[run.id, run]]));
    tauriMock.daemonProcessOutput.mockResolvedValueOnce({
      record: {
        id: "process-1",
        command: "difit --port 4966",
        workingDir: "/repo",
        startedAtMs: 100,
        running: false,
        exitCode: 1,
        retainedOutputBytes: 64,
        outputTruncated: false,
      },
      output: "command: difit --port 4966\ncwd: /repo\n\nstartup failed",
    });

    const { unmount } = render(ExternalToolMainView, { runId: run.id });

    expect(screen.getByText("Process Logs")).toBeTruthy();
    await waitFor(() =>
      expect(screen.getByText(/startup failed/)).toBeTruthy(),
    );

    unmount();
  });
});
