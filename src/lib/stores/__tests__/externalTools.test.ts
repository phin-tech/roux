import { afterEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import type { ExternalTool } from "$lib/bindings";
import { DEFAULT_SETTINGS } from "$lib/types";

vi.mock("$lib/tauri", () => ({
  daemonProcessKill: vi.fn().mockResolvedValue(processRecord("process-1")),
  daemonProcessOutput: vi.fn(),
  killPty: vi.fn().mockResolvedValue(undefined),
  launchExternalTool: vi.fn(),
}));

import {
  daemonProcessKill,
  killPty,
  launchExternalTool,
  type ExternalToolLaunchResult,
} from "$lib/tauri";
import {
  externalToolRuns,
  externalToolRunId,
  externalToolRunIsLive,
  closeExternalToolRun,
  failExternalToolRun,
  markExternalToolExited,
  openExternalTool,
  registerExternalToolViewCloser,
  restartExternalToolRun,
  setExternalToolRunError,
  type ExternalToolRun,
  type ExternalToolRunStatus,
} from "../externalTools";
import { closeMainView, mainViewRoute, openMainView } from "../mainView";
import { settings } from "../settings";

function processRecord(id: string) {
  return {
    id,
    command: "",
    workingDir: "",
    startedAtMs: 0,
    running: false,
    exitCode: null,
    retainedOutputBytes: 0,
    outputTruncated: false,
  };
}

function runWithStatus(status: ExternalToolRunStatus): ExternalToolRun {
  return {
    id: "lazygit:session-1",
    toolId: "lazygit",
    toolName: "Lazygit",
    surface: "terminal",
    webEmbedder: "webview",
    keepWebviewAlive: false,
    sessionId: "session-1",
    review: null,
    runtimeId: "pty-1",
    runtimeGeneration: 1,
    rendered: null,
    status,
    error: null,
    logsOpen: false,
    launchedAtMs: 100,
  };
}

function webTool(): ExternalTool {
  return {
    id: "difit",
    name: "Difit",
    enabled: true,
    surface: "web",
    commandTemplate:
      "difit . --host 127.0.0.1 --port {{ port }} --no-open --keep-alive",
    cwdTemplate: ".",
    requiresSession: false,
    urlTemplate: "http://127.0.0.1:4966",
    preferredPort: 4966,
    webEmbedder: "webview",
    keepWebviewAlive: false,
  };
}

function terminalTool(): ExternalTool {
  return {
    id: "lazygit",
    name: "Lazygit",
    enabled: true,
    surface: "terminal",
    commandTemplate: "lazygit",
    cwdTemplate: ".",
    requiresSession: false,
    urlTemplate: null,
    preferredPort: null,
    webEmbedder: "webview",
    keepWebviewAlive: false,
  };
}

function webLaunchResult(runtimeId: string | null): ExternalToolLaunchResult {
  return {
    toolId: "difit",
    surface: "web",
    sessionId: null,
    runtimeId,
    runtimeGeneration: null,
    rendered: {
      command: "difit . --host 127.0.0.1 --port 4966 --no-open --keep-alive",
      cwd: "/repo",
      url: "http://127.0.0.1:4966",
      port: 4966,
    },
  };
}

function terminalLaunchResult(
  runtimeId: string | null,
): ExternalToolLaunchResult {
  return {
    toolId: "lazygit",
    surface: "terminal",
    sessionId: null,
    runtimeId,
    runtimeGeneration: 2,
    rendered: {
      command: "lazygit",
      cwd: "/repo",
      url: null,
      port: null,
    },
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason?: unknown) => void = () => {};
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

describe("externalTools store helpers", () => {
  afterEach(() => {
    externalToolRuns.set(new Map());
    closeMainView();
    settings.set(DEFAULT_SETTINGS);
    vi.mocked(launchExternalTool).mockReset();
    vi.clearAllMocks();
  });

  it("keys runs by tool and bound session", () => {
    expect(externalToolRunId("lazygit", "session-1")).toBe("lazygit:session-1");
    expect(externalToolRunId("difit", null)).toBe("difit:global");
  });

  it("treats only error runs as not live", () => {
    expect(externalToolRunIsLive(runWithStatus("launching"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("starting"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("running"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("ready"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("error"))).toBe(false);
  });

  it("removes an external tool run and closes its view when the runtime exits", () => {
    const run = runWithStatus("running");
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    markExternalToolExited(run.id, run.runtimeId, 0, run.runtimeGeneration);

    expect(get(externalToolRuns).has(run.id)).toBe(false);
    expect(get(mainViewRoute)).toBeNull();
  });

  it("closes the registered view before waiting for runtime cleanup", async () => {
    let finishKill: (
      value: ReturnType<typeof processRecord>,
    ) => void = () => {};
    vi.mocked(daemonProcessKill).mockReturnValueOnce(
      new Promise((resolve) => {
        finishKill = resolve;
      }),
    );
    const run = {
      ...runWithStatus("running"),
      surface: "web" as const,
      runtimeId: "process-1",
      runtimeGeneration: null,
    };
    const closeView = vi.fn();
    const unregister = registerExternalToolViewCloser(run.id, closeView);
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    const closed = closeExternalToolRun(run.id);

    expect(closeView).toHaveBeenCalledOnce();
    expect(get(externalToolRuns).has(run.id)).toBe(false);
    expect(get(mainViewRoute)).toBeNull();
    expect(daemonProcessKill).toHaveBeenCalledWith("process-1");

    finishKill(processRecord("process-1"));
    await closed;
    unregister();
  });

  it("kills a web runtime that resolves after the run was closed while launching", async () => {
    const launch = deferred<ExternalToolLaunchResult>();
    vi.mocked(launchExternalTool).mockReturnValueOnce(launch.promise);
    settings.update((current) => ({ ...current, externalTools: [webTool()] }));

    const opened = openExternalTool("difit");
    const runId = externalToolRunId("difit", null);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
    });

    await closeExternalToolRun(runId);
    expect(get(externalToolRuns).has(runId)).toBe(false);

    launch.resolve(webLaunchResult("process-late"));
    await opened;

    expect(daemonProcessKill).toHaveBeenCalledWith("process-late");
    expect(get(externalToolRuns).has(runId)).toBe(false);
  });

  it("kills a stale web runtime instead of overwriting a reopened launch", async () => {
    const firstLaunch = deferred<ExternalToolLaunchResult>();
    const secondLaunch = deferred<ExternalToolLaunchResult>();
    vi.mocked(launchExternalTool)
      .mockReturnValueOnce(firstLaunch.promise)
      .mockReturnValueOnce(secondLaunch.promise);
    settings.update((current) => ({ ...current, externalTools: [webTool()] }));

    const opened = openExternalTool("difit");
    const runId = externalToolRunId("difit", null);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
    });

    await closeExternalToolRun(runId);
    const reopened = openExternalTool("difit");
    expect(launchExternalTool).toHaveBeenCalledTimes(2);

    firstLaunch.resolve(webLaunchResult("process-old"));
    await opened;
    expect(daemonProcessKill).toHaveBeenCalledWith("process-old");

    secondLaunch.resolve(webLaunchResult("process-new"));
    await reopened;
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "process-new",
      status: "starting",
    });
  });

  it("replaces an in-flight launch when restarting before a runtime exists", async () => {
    const firstLaunch = deferred<ExternalToolLaunchResult>();
    const secondLaunch = deferred<ExternalToolLaunchResult>();
    vi.mocked(launchExternalTool)
      .mockReturnValueOnce(firstLaunch.promise)
      .mockReturnValueOnce(secondLaunch.promise);
    settings.update((current) => ({ ...current, externalTools: [webTool()] }));

    const opened = openExternalTool("difit");
    const runId = externalToolRunId("difit", null);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
      runtimeId: null,
    });

    const restarted = restartExternalToolRun(runId);
    expect(launchExternalTool).toHaveBeenCalledTimes(2);

    firstLaunch.resolve(webLaunchResult("process-old"));
    await opened;
    expect(daemonProcessKill).toHaveBeenCalledWith("process-old");

    secondLaunch.resolve(webLaunchResult("process-new"));
    await restarted;
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "process-new",
      status: "starting",
    });
  });

  it("kills an errored terminal runtime before relaunching the tool", async () => {
    const order: string[] = [];
    vi.mocked(killPty).mockImplementationOnce(async (id) => {
      order.push(`kill:${id}`);
    });
    vi.mocked(launchExternalTool).mockImplementationOnce(async () => {
      order.push("launch");
      return terminalLaunchResult("pty-new");
    });
    settings.update((current) => ({
      ...current,
      externalTools: [terminalTool()],
    }));
    const runId = externalToolRunId("lazygit", null);
    externalToolRuns.set(
      new Map([
        [
          runId,
          {
            ...runWithStatus("error"),
            id: runId,
            toolId: "lazygit",
            toolName: "Lazygit",
            sessionId: null,
            runtimeId: "pty-old",
            error: "attach failed",
          },
        ],
      ]),
    );

    await openExternalTool("lazygit");

    expect(order).toEqual(["kill:pty-old", "launch"]);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "pty-new",
      status: "running",
    });
  });

  it("does not duplicate launches while a restart is retiring the old runtime", async () => {
    const kill = deferred<void>();
    vi.mocked(killPty).mockReturnValueOnce(kill.promise);
    vi.mocked(launchExternalTool).mockResolvedValueOnce(
      terminalLaunchResult("pty-new"),
    );
    settings.update((current) => ({
      ...current,
      externalTools: [terminalTool()],
    }));
    const runId = externalToolRunId("lazygit", null);
    externalToolRuns.set(
      new Map([
        [
          runId,
          {
            ...runWithStatus("running"),
            id: runId,
            toolId: "lazygit",
            toolName: "Lazygit",
            sessionId: null,
            runtimeId: "pty-old",
          },
        ],
      ]),
    );

    const firstRestart = restartExternalToolRun(runId);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
      runtimeId: null,
      runtimeGeneration: null,
      rendered: null,
    });

    await restartExternalToolRun(runId);
    expect(killPty).toHaveBeenCalledTimes(1);
    expect(launchExternalTool).not.toHaveBeenCalled();

    kill.resolve();
    await firstRestart;

    expect(launchExternalTool).toHaveBeenCalledTimes(1);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "pty-new",
      status: "running",
    });
  });

  it("does not duplicate launches while an errored runtime is being retired", async () => {
    const kill = deferred<void>();
    vi.mocked(killPty).mockReturnValueOnce(kill.promise);
    vi.mocked(launchExternalTool).mockResolvedValueOnce(
      terminalLaunchResult("pty-new"),
    );
    settings.update((current) => ({
      ...current,
      externalTools: [terminalTool()],
    }));
    const runId = externalToolRunId("lazygit", null);
    externalToolRuns.set(
      new Map([
        [
          runId,
          {
            ...runWithStatus("error"),
            id: runId,
            toolId: "lazygit",
            toolName: "Lazygit",
            sessionId: null,
            runtimeId: "pty-old",
            error: "attach failed",
          },
        ],
      ]),
    );

    const firstOpen = openExternalTool("lazygit");
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
      runtimeId: null,
      runtimeGeneration: null,
      rendered: null,
      error: null,
    });

    setExternalToolRunError(runId, "stale attach failed", "pty-old", 1);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
      error: null,
    });

    await openExternalTool("lazygit");
    expect(killPty).toHaveBeenCalledTimes(1);
    expect(launchExternalTool).not.toHaveBeenCalled();

    kill.resolve();
    await firstOpen;

    expect(launchExternalTool).toHaveBeenCalledTimes(1);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "pty-new",
      status: "running",
    });
  });

  it("does not let stale relaunch cleanup overwrite a close and reopen", async () => {
    const kill = deferred<void>();
    const replacementLaunch = deferred<ExternalToolLaunchResult>();
    vi.mocked(killPty).mockReturnValueOnce(kill.promise);
    vi.mocked(launchExternalTool).mockReturnValueOnce(
      replacementLaunch.promise,
    );
    settings.update((current) => ({
      ...current,
      externalTools: [terminalTool()],
    }));
    const runId = externalToolRunId("lazygit", null);
    externalToolRuns.set(
      new Map([
        [
          runId,
          {
            ...runWithStatus("error"),
            id: runId,
            toolId: "lazygit",
            toolName: "Lazygit",
            sessionId: null,
            runtimeId: "pty-old",
            error: "attach failed",
          },
        ],
      ]),
    );

    const staleRelaunch = openExternalTool("lazygit");
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      status: "launching",
      runtimeId: null,
    });

    await closeExternalToolRun(runId);
    const reopened = openExternalTool("lazygit");
    expect(launchExternalTool).toHaveBeenCalledTimes(1);

    replacementLaunch.resolve(terminalLaunchResult("pty-new"));
    await reopened;
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "pty-new",
      status: "running",
    });

    kill.resolve();
    await staleRelaunch;

    expect(launchExternalTool).toHaveBeenCalledTimes(1);
    expect(get(externalToolRuns).get(runId)).toMatchObject({
      runtimeId: "pty-new",
      status: "running",
    });
  });

  it("ignores stale exit events from an older runtime id", () => {
    const run = {
      ...runWithStatus("running"),
      runtimeId: "pty-new",
      runtimeGeneration: 2,
    };
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    markExternalToolExited(run.id, "pty-old", 0, 1);

    expect(get(externalToolRuns).get(run.id)).toEqual(run);
    expect(get(mainViewRoute)).toEqual({ kind: "externalTool", runId: run.id });
  });

  it("ignores stale exit events from an older PTY generation", () => {
    const run = { ...runWithStatus("running"), runtimeGeneration: 2 };
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    markExternalToolExited(run.id, run.runtimeId, 0, 1);

    expect(get(externalToolRuns).get(run.id)).toEqual(run);
    expect(get(mainViewRoute)).toEqual({ kind: "externalTool", runId: run.id });
  });

  it("keeps errored runs visible when their cleaned-up runtime exits", () => {
    const run = {
      ...runWithStatus("error"),
      surface: "web" as const,
      runtimeId: "process-1",
      runtimeGeneration: null,
      error: "webview failed",
      logsOpen: true,
    };
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    markExternalToolExited(run.id, run.runtimeId, null);

    expect(get(externalToolRuns).get(run.id)).toEqual(run);
    expect(get(mainViewRoute)).toEqual({ kind: "externalTool", runId: run.id });
  });

  it("keeps a web tool pane open with logs when its process exits", () => {
    const run = {
      ...runWithStatus("starting"),
      id: "difit:global",
      toolId: "difit",
      toolName: "Difit",
      surface: "web" as const,
      sessionId: null,
      runtimeId: "process-1",
      runtimeGeneration: null,
    };
    externalToolRuns.set(new Map([[run.id, run]]));
    openMainView({ kind: "externalTool", runId: run.id });

    markExternalToolExited(run.id, run.runtimeId, 1, run.runtimeGeneration);

    expect(get(externalToolRuns).get(run.id)).toMatchObject({
      status: "error",
      error: "Process exited with code 1",
      logsOpen: true,
      runtimeId: "process-1",
    });
    expect(get(mainViewRoute)).toEqual({ kind: "externalTool", runId: run.id });
    expect(daemonProcessKill).not.toHaveBeenCalled();
  });

  it("kills the matching web runtime before marking a launched run failed", async () => {
    const run = {
      ...runWithStatus("running"),
      surface: "web" as const,
      runtimeId: "process-1",
      runtimeGeneration: null,
    };
    externalToolRuns.set(new Map([[run.id, run]]));

    await failExternalToolRun(run.id, run.runtimeId, "webview failed");

    expect(daemonProcessKill).toHaveBeenCalledWith("process-1");
    expect(get(externalToolRuns).get(run.id)).toMatchObject({
      status: "error",
      error: "webview failed",
      logsOpen: true,
    });
  });

  it("marks a url-only web run failed without runtime cleanup", async () => {
    const run = {
      ...runWithStatus("running"),
      surface: "web" as const,
      runtimeId: null,
      runtimeGeneration: null,
    };
    externalToolRuns.set(new Map([[run.id, run]]));

    await failExternalToolRun(run.id, null, "webview failed");

    expect(daemonProcessKill).not.toHaveBeenCalled();
    expect(get(externalToolRuns).get(run.id)).toMatchObject({
      status: "error",
      error: "webview failed",
      logsOpen: true,
    });
  });

  it("marks a launched run errored before awaiting runtime cleanup", async () => {
    let finishKill: (
      value: ReturnType<typeof processRecord>,
    ) => void = () => {};
    vi.mocked(daemonProcessKill).mockReturnValueOnce(
      new Promise((resolve) => {
        finishKill = resolve;
      }),
    );
    const run = {
      ...runWithStatus("running"),
      surface: "web" as const,
      runtimeId: "process-1",
      runtimeGeneration: null,
    };
    externalToolRuns.set(new Map([[run.id, run]]));

    const failed = failExternalToolRun(run.id, run.runtimeId, "webview failed");

    expect(get(externalToolRuns).get(run.id)).toMatchObject({
      status: "error",
      error: "webview failed",
    });

    markExternalToolExited(run.id, run.runtimeId, null);
    expect(get(externalToolRuns).has(run.id)).toBe(true);

    finishKill(processRecord("process-1"));
    await failed;
  });

  it("ignores launched-run failures from a stale runtime id", async () => {
    const run = {
      ...runWithStatus("running"),
      surface: "web" as const,
      runtimeId: "process-new",
      runtimeGeneration: null,
    };
    externalToolRuns.set(new Map([[run.id, run]]));

    await failExternalToolRun(run.id, "process-old", "webview failed");

    expect(daemonProcessKill).not.toHaveBeenCalled();
    expect(get(externalToolRuns).get(run.id)).toEqual(run);
  });
});
