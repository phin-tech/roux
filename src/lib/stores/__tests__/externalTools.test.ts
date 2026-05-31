import { afterEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import {
  externalToolRuns,
  externalToolRunId,
  externalToolRunIsLive,
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

    markExternalToolExited(run.id, 0);

    expect(get(externalToolRuns).has(run.id)).toBe(false);
    expect(get(mainViewRoute)).toBeNull();
  });
});
