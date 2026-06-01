import { afterEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  daemonProcessKill: vi.fn().mockResolvedValue({
    id: "process-1",
    command: "",
    workingDir: "",
    startedAtMs: 0,
    running: false,
    exitCode: null,
    retainedOutputBytes: 0,
    outputTruncated: false,
  }),
  daemonProcessOutput: vi.fn(),
  killPty: vi.fn().mockResolvedValue(undefined),
  launchExternalTool: vi.fn(),
}));

import { daemonProcessKill } from "$lib/tauri";
import {
  externalToolRuns,
  externalToolRunId,
  externalToolRunIsLive,
  failExternalToolRun,
  markExternalToolExited,
  type ExternalToolRun,
  type ExternalToolRunStatus,
} from "../externalTools";
import { closeMainView, mainViewRoute, openMainView } from "../mainView";

function runWithStatus(status: ExternalToolRunStatus): ExternalToolRun {
  return {
    id: "lazygit:session-1",
    toolId: "lazygit",
    toolName: "Lazygit",
    surface: "terminal",
    sessionId: "session-1",
    runtimeId: "pty-1",
    runtimeGeneration: 1,
    rendered: null,
    status,
    error: null,
    exitCode: null,
    logsOpen: false,
    launchedAtMs: 100,
  };
}

describe("externalTools store helpers", () => {
  afterEach(() => {
    externalToolRuns.set(new Map());
    closeMainView();
    vi.clearAllMocks();
  });

  it("keys runs by tool and bound session", () => {
    expect(externalToolRunId("lazygit", "session-1")).toBe("lazygit:session-1");
    expect(externalToolRunId("difit", null)).toBe("difit:global");
  });

  it("treats only exited and error runs as not live", () => {
    expect(externalToolRunIsLive(runWithStatus("launching"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("starting"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("running"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("ready"))).toBe(true);
    expect(externalToolRunIsLive(runWithStatus("exited"))).toBe(false);
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

  it("ignores stale exit events from an older runtime id", () => {
    const run = { ...runWithStatus("running"), runtimeId: "pty-new", runtimeGeneration: 2 };
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
