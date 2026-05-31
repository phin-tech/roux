import { describe, expect, it } from "vitest";
import {
  externalToolRunId,
  externalToolRunIsLive,
  type ExternalToolRun,
  type ExternalToolRunStatus,
} from "../externalTools";

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
});
