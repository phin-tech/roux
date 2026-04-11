import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import {
  routeStatusUpdate,
  applyStatusRouting,
} from "../statusRouting";
import { agentStates, resetAgentStates } from "../agentState";
import type { StatusUpdate } from "$lib/tauri";

function ev(partial: Partial<StatusUpdate> = {}): StatusUpdate {
  return {
    status: "generating",
    cwd: "/repo",
    claudeSessionId: "claude-sess-1",
    provider: "claude",
    rouxSessionId: "sess-1",
    rouxPaneId: "pane-1",
    toolName: null,
    toolInput: null,
    message: null,
    ...partial,
  };
}

describe("routeStatusUpdate", () => {
  beforeEach(() => {
    resetAgentStates();
  });

  it("routes to the pane tier when rouxPaneId is present", () => {
    const routing = routeStatusUpdate(ev({ status: "generating" }));
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.paneId).toBe("pane-1");
    expect(routing.event.provider).toBe("claude");
    expect(routing.event.status).toBe("generating");
    expect(routing.event.source).toBe("hook");
  });

  it("maps thinking / attention to generating", () => {
    expect(
      (routeStatusUpdate(ev({ status: "thinking" })) as { event: { status: string } })
        .event.status,
    ).toBe("generating");
    expect(
      (routeStatusUpdate(ev({ status: "attention" })) as { event: { status: string } })
        .event.status,
    ).toBe("generating");
  });

  it("drops non-routable pane statuses (error/disconnected)", () => {
    const err = routeStatusUpdate(ev({ status: "error" }));
    expect(err.kind).toBe("dropped");
    const disc = routeStatusUpdate(ev({ status: "disconnected" }));
    expect(disc.kind).toBe("dropped");
  });

  it("falls back to legacy cwd routing when rouxPaneId is missing", () => {
    const routing = routeStatusUpdate(ev({ rouxPaneId: null }));
    expect(routing.kind).toBe("legacy");
    if (routing.kind !== "legacy") throw new Error("unreachable");
    expect(routing.cwd).toBe("/repo");
    expect(routing.status).toBe("generating");
  });

  it("infers provider: claude when unset but a claude session id is present", () => {
    const routing = routeStatusUpdate(ev({ provider: "" }));
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.event.provider).toBe("claude");
  });

  it("drops the event when no provider can be inferred at all", () => {
    const routing = routeStatusUpdate(
      ev({ provider: "", claudeSessionId: "" }),
    );
    expect(routing.kind).toBe("dropped");
  });

  it("builds permissionInfo from toolName / toolInput / message when attention fires", () => {
    const routing = routeStatusUpdate(
      ev({
        status: "attention",
        toolName: "Edit",
        toolInput: { file: "README.md" },
        message: "Allow tool use?",
      }),
    );
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.event.permissionInfo).toEqual({
      toolName: "Edit",
      toolInput: { file: "README.md" },
      message: "Allow tool use?",
    });
  });
});

describe("applyStatusRouting", () => {
  beforeEach(() => {
    resetAgentStates();
  });

  it("writes a pane-tier decision into agentStates", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ status: "generating" })),
    );
    const entry = get(agentStates).get("pane-1");
    expect(entry?.provider).toBe("claude");
    expect(entry?.status).toBe("generating");
  });

  it("leaves agentStates untouched for legacy events", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ rouxPaneId: null })),
    );
    expect(get(agentStates).size).toBe(0);
  });

  it("leaves agentStates untouched for dropped events", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ status: "error" })),
    );
    expect(get(agentStates).size).toBe(0);
  });
});
