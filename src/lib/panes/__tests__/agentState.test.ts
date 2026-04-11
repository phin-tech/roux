import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import {
  agentStates,
  updateAgentState,
  disposeAgentState,
  getSessionAgentStatus,
  sessionAgentStatus,
  resetAgentStates,
} from "../agentState";
import { sessionLayouts, resetLayouts, insertLeaf } from "../layout";

function setSingleLeafLayout(sessionId: string, paneId: string): void {
  sessionLayouts.update((m) => {
    const next = new Map(m);
    next.set(sessionId, { kind: "leaf", paneId });
    return next;
  });
}

function setSplitLayout(sessionId: string, panes: string[]): void {
  sessionLayouts.update((m) => {
    let tree = { kind: "leaf" as const, paneId: panes[0] };
    let current: ReturnType<typeof insertLeaf> = tree;
    for (let i = 1; i < panes.length; i++) {
      current = insertLeaf(current, panes[i - 1], "h", panes[i]);
    }
    const next = new Map(m);
    next.set(sessionId, current);
    return next;
  });
}

describe("agentState store", () => {
  beforeEach(() => {
    resetAgentStates();
    resetLayouts();
  });

  describe("updateAgentState", () => {
    it("creates a new entry with updatedAt stamped", () => {
      const before = Date.now();
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        source: "hook",
      });
      const entry = get(agentStates).get("pane-1");
      expect(entry?.provider).toBe("claude");
      expect(entry?.status).toBe("generating");
      expect(entry?.source).toBe("hook");
      expect(entry?.updatedAt).toBeGreaterThanOrEqual(before);
    });

    it("merges previous permissionInfo forward when a later event omits it", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        permissionInfo: { toolName: "Edit" },
        source: "hook",
      });
      updateAgentState("pane-1", {
        provider: "claude",
        status: "idle",
        source: "hook",
      });

      const entry = get(agentStates).get("pane-1");
      expect(entry?.status).toBe("idle");
      expect(entry?.permissionInfo?.toolName).toBe("Edit");
    });

    it("allows explicit permissionInfo to replace the merged-forward one", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        permissionInfo: { toolName: "Edit" },
        source: "hook",
      });
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        permissionInfo: { toolName: "Bash" },
        source: "hook",
      });
      expect(get(agentStates).get("pane-1")?.permissionInfo?.toolName).toBe("Bash");
    });
  });

  describe("disposeAgentState", () => {
    it("removes the entry on pane disposal", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "idle",
        source: "hook",
      });
      disposeAgentState("pane-1");
      expect(get(agentStates).has("pane-1")).toBe(false);
    });

    it("is a no-op when the pane has no entry", () => {
      expect(() => disposeAgentState("never-seen")).not.toThrow();
    });
  });

  describe("sessionAgentStatus aggregate", () => {
    it("is null when no pane has an agent state", () => {
      setSingleLeafLayout("sess-1", "pane-1");
      expect(getSessionAgentStatus("sess-1")).toBeNull();
    });

    it("is null for sessions with no layout", () => {
      expect(getSessionAgentStatus("missing")).toBeNull();
    });

    it("is idle when the only pane with state is idle", () => {
      setSingleLeafLayout("sess-1", "pane-1");
      updateAgentState("pane-1", { provider: "claude", status: "idle", source: "hook" });
      expect(getSessionAgentStatus("sess-1")).toBe("idle");
    });

    it("is generating when any pane is generating, even alongside idle panes", () => {
      setSplitLayout("sess-1", ["pane-a", "pane-b"]);
      updateAgentState("pane-a", { provider: "claude", status: "idle", source: "hook" });
      updateAgentState("pane-b", { provider: "claude", status: "generating", source: "hook" });
      expect(getSessionAgentStatus("sess-1")).toBe("generating");
    });

    it("is idle when at least one pane is idle and none are generating", () => {
      setSplitLayout("sess-1", ["pane-a", "pane-b"]);
      updateAgentState("pane-a", { provider: "claude", status: "idle", source: "hook" });
      // pane-b has no state — should not count.
      expect(getSessionAgentStatus("sess-1")).toBe("idle");
    });

    it("the derived store emits the same map as the synchronous snapshot", () => {
      setSplitLayout("sess-1", ["pane-a", "pane-b"]);
      setSingleLeafLayout("sess-2", "pane-c");
      updateAgentState("pane-a", { provider: "claude", status: "generating", source: "hook" });
      updateAgentState("pane-c", { provider: "codex", status: "idle", source: "hook" });

      const map = get(sessionAgentStatus);
      expect(map.get("sess-1")).toBe("generating");
      expect(map.get("sess-2")).toBe("idle");
    });
  });
});
