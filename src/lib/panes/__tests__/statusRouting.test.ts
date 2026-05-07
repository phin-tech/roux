import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

import {
  routeStatusUpdate,
  applyStatusRouting,
  type PaneSessionCheck,
} from "../statusRouting";
import { agentStates, resetAgentStates } from "../agentState";
import { createPane, paneInstances, resetInstances } from "../instances";
import type { StatusUpdate } from "$lib/tauri";

function ev(partial: Partial<StatusUpdate> = {}): StatusUpdate {
  return {
    status: "generating",
    cwd: "/repo",
    providerSessionId: "claude-sess-1",
    provider: "claude",
    rouxSessionId: "sess-1",
    rouxPaneId: "pane-1",
    toolName: null,
    toolInput: null,
    message: null,
    query: null,
    response: null,
    ...partial,
  };
}

/**
 * Trust-all membership check for tests that aren't exercising the
 * cross-session validation path. Individual membership tests still pass
 * their own narrower check.
 */
const trustAll: PaneSessionCheck = () => true;

describe("routeStatusUpdate", () => {
  beforeEach(() => {
    resetAgentStates();
    resetInstances();
  });

  it("routes to the pane tier when rouxPaneId is present", () => {
    const routing = routeStatusUpdate(ev({ status: "generating" }), trustAll);
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.paneId).toBe("pane-1");
    expect(routing.event.provider).toBe("claude");
    expect(routing.event.status).toBe("generating");
    expect(routing.event.source).toBe("hook");
  });

  it("maps thinking to generating and attention to blocked", () => {
    expect(
      (routeStatusUpdate(ev({ status: "thinking" }), trustAll) as {
        event: { status: string };
      }).event.status,
    ).toBe("generating");
    expect(
      (routeStatusUpdate(ev({ status: "attention" }), trustAll) as {
        event: { status: string };
      }).event.status,
    ).toBe("blocked");
  });

  it("routes error to the pane tier and drops disconnected", () => {
    const err = routeStatusUpdate(ev({ status: "error" }), trustAll);
    expect(err.kind).toBe("pane");
    if (err.kind !== "pane") throw new Error("unreachable");
    expect(err.event.status).toBe("error");

    const disc = routeStatusUpdate(ev({ status: "disconnected" }), trustAll);
    expect(disc.kind).toBe("dropped");
  });

  it("falls back to legacy cwd routing when rouxPaneId is missing", () => {
    const routing = routeStatusUpdate(ev({ rouxPaneId: null }), trustAll);
    expect(routing.kind).toBe("legacy");
    if (routing.kind !== "legacy") throw new Error("unreachable");
    expect(routing.cwd).toBe("/repo");
    expect(routing.status).toBe("generating");
  });

  it("infers provider: claude when unset but a provider session id is present", () => {
    const routing = routeStatusUpdate(ev({ provider: "" }), trustAll);
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.event.provider).toBe("claude");
  });

  it("drops the event when no provider can be inferred at all", () => {
    const routing = routeStatusUpdate(
      ev({ provider: "", providerSessionId: null }),
      trustAll,
    );
    expect(routing.kind).toBe("dropped");
  });

  it("drops the event when rouxPaneId does not belong to rouxSessionId", () => {
    // Threat model: a malicious or buggy hook writes a status file
    // claiming it belongs to session A but using a pane id that actually
    // lives in session B. Without validation, this would let the hook
    // smear A's aggregate status with B's pane state. The router asks
    // the provided `paneBelongsToSession` callback and drops mismatches.
    const routing = routeStatusUpdate(
      ev({ rouxSessionId: "sess-1", rouxPaneId: "pane-hijack" }),
      (sessionId, paneId) => sessionId === "sess-1" && paneId === "pane-1",
    );
    expect(routing.kind).toBe("dropped");
    if (routing.kind !== "dropped") throw new Error("unreachable");
    expect(routing.reason).toMatch(/pane-hijack/);
    expect(routing.reason).toMatch(/sess-1/);
  });

  it("drops the event when the claimed session is unknown", () => {
    const routing = routeStatusUpdate(
      ev({ rouxSessionId: "gone-sess", rouxPaneId: "pane-1" }),
      () => false,
    );
    expect(routing.kind).toBe("dropped");
  });

  it("routes when the pane legitimately belongs to the claimed session", () => {
    const routing = routeStatusUpdate(
      ev({ rouxSessionId: "sess-1", rouxPaneId: "pane-1" }),
      (sessionId, paneId) => sessionId === "sess-1" && paneId === "pane-1",
    );
    expect(routing.kind).toBe("pane");
  });

  it("requires rouxSessionId for pane routing", () => {
    // A hook carrying only rouxPaneId (no session id) cannot be validated
    // cross-session, so we refuse to route it rather than trusting it.
    const routing = routeStatusUpdate(
      ev({ rouxSessionId: null, rouxPaneId: "pane-1" }),
      () => true,
    );
    expect(routing.kind).toBe("dropped");
    if (routing.kind !== "dropped") throw new Error("unreachable");
    expect(routing.reason).toMatch(/rouxSessionId/);
  });

  it("builds permissionInfo from toolName / toolInput / message when attention fires", () => {
    const routing = routeStatusUpdate(
      ev({
        status: "attention",
        toolName: "Edit",
        toolInput: { file: "README.md" },
        message: "Allow tool use?",
      }),
      trustAll,
    );
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.event.status).toBe("blocked");
    expect(routing.event.permissionInfo).toEqual({
      toolName: "Edit",
      toolInput: { file: "README.md" },
      message: "Allow tool use?",
    });
  });

  it("builds completionSummary from Stop transcript fields", () => {
    const routing = routeStatusUpdate(
      ev({
        status: "idle",
        query: "make it work",
        response: "done",
      }),
      trustAll,
    );
    expect(routing.kind).toBe("pane");
    if (routing.kind !== "pane") throw new Error("unreachable");
    expect(routing.event.completionSummary).toEqual({
      query: "make it work",
      response: "done",
    });
  });
});

describe("applyStatusRouting", () => {
  beforeEach(() => {
    resetAgentStates();
    resetInstances();
  });

  it("writes a pane-tier decision into agentStates", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ status: "generating" }), trustAll),
    );
    const entry = get(agentStates).get("pane-1");
    expect(entry?.provider).toBe("claude");
    expect(entry?.status).toBe("generating");
  });

  it("writes providerSessionId into pane instances", () => {
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1" });

    applyStatusRouting(
      routeStatusUpdate(ev({ providerSessionId: "claude-session-123" }), trustAll),
    );

    const pane = get(paneInstances).get("pane-1");
    expect(pane?.providerSessionId).toBe("claude-session-123");
  });

  it("does not overwrite an existing instance.provider with the inferred routing provider", () => {
    // Regression: routeStatusUpdate infers `provider: "claude"` for legacy
    // hooks that omit the field. If applyStatusRouting persisted that
    // inferred value, a Codex pane whose hook didn't carry `provider`
    // would get its instance.provider clobbered to "claude", and
    // continueSession would then build `claude --resume <id>` instead of
    // `codex resume <id>`. We persist providerSessionId only.
    createPane({
      id: "pane-1",
      type: "shell",
      ptyId: "pty-1",
      provider: "codex",
    });

    applyStatusRouting(
      routeStatusUpdate(
        ev({ provider: "", providerSessionId: "codex-thread-7" }),
        trustAll,
      ),
    );

    const pane = get(paneInstances).get("pane-1");
    expect(pane?.provider).toBe("codex");
    expect(pane?.providerSessionId).toBe("codex-thread-7");
  });

  it("does not write instance.provider at all from routing events", () => {
    // Even when the routing event has an explicit provider, we don't
    // touch instance.provider — that field is owned by createPane / the
    // persisted descriptor and runtime status events shouldn't fight it.
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1" });

    applyStatusRouting(
      routeStatusUpdate(ev({ provider: "claude", providerSessionId: "x" }), trustAll),
    );

    const pane = get(paneInstances).get("pane-1");
    expect(pane?.provider).toBeUndefined();
  });

  it("leaves agentStates untouched for legacy events", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ rouxPaneId: null }), trustAll),
    );
    expect(get(agentStates).size).toBe(0);
  });

  it("leaves agentStates untouched for dropped events", () => {
    applyStatusRouting(
      routeStatusUpdate(ev({ status: "disconnected" }), trustAll),
    );
    expect(get(agentStates).size).toBe(0);
  });
});
