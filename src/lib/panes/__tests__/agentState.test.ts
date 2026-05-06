import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import {
  agentStates,
  updateAgentState,
  disposeAgentState,
  clearPermissionInfo,
  getSessionAgentStatus,
  sessionAgentStatus,
  resetAgentStates,
  computeEffectiveSessionStatus,
} from "../agentState";
import { sessionLayouts, resetLayouts, insertLeaf } from "../layout";
// Importing actions.ts registers the post-dispose hook that wires
// disposePane → disposeAgentState. We exercise that integration below.
import { closePane, initSession } from "../actions";
import { resetInstances } from "../instances";
import { resetFocus } from "../focus";

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

    it("does not carry a stale completionSummary into later events", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "idle",
        completionSummary: { query: "old prompt", response: "old response" },
        source: "hook",
      });
      expect(get(agentStates).get("pane-1")?.completionSummary?.query).toBe("old prompt");

      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        source: "hook",
      });
      expect(get(agentStates).get("pane-1")?.completionSummary).toBeUndefined();

      updateAgentState("pane-1", {
        provider: "claude",
        status: "idle",
        source: "hook",
      });
      expect(get(agentStates).get("pane-1")?.completionSummary).toBeUndefined();
    });
  });

  describe("clearPermissionInfo", () => {
    it("clears permissionInfo without touching status", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        permissionInfo: { toolName: "Bash", toolInput: { command: "rm -rf /" } },
        source: "hook",
      });
      expect(get(agentStates).get("pane-1")?.permissionInfo?.toolName).toBe("Bash");

      clearPermissionInfo("pane-1");
      const entry = get(agentStates).get("pane-1");
      expect(entry?.permissionInfo).toBeUndefined();
      expect(entry?.status).toBe("generating");
      expect(entry?.provider).toBe("claude");
    });

    it("is a no-op when the pane has no entry", () => {
      expect(() => clearPermissionInfo("never-seen")).not.toThrow();
      expect(get(agentStates).has("never-seen")).toBe(false);
    });

    it("is a no-op when permissionInfo is already undefined", () => {
      updateAgentState("pane-1", {
        provider: "claude",
        status: "generating",
        source: "hook",
      });
      const before = get(agentStates).get("pane-1");
      clearPermissionInfo("pane-1");
      const after = get(agentStates).get("pane-1");
      // Map reference unchanged when nothing moved — updatedAt stays stable.
      expect(after?.updatedAt).toBe(before?.updatedAt);
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

    it("fires automatically when closePane disposes a pane (dispose-hook integration)", () => {
      // Regression guard against the easy-to-forget pattern of calling
      // disposePane without disposeAgentState. A post-dispose hook is
      // registered in actions.ts at module load to close the gap for
      // every caller — this test exercises that hook end-to-end.
      resetInstances();
      resetLayouts();
      resetFocus();
      resetAgentStates();

      initSession("sess-1");
      updateAgentState("sess-1-main", {
        provider: "claude",
        status: "generating",
        source: "hook",
      });
      expect(get(agentStates).has("sess-1-main")).toBe(true);

      closePane("sess-1", "sess-1-main");
      expect(get(agentStates).has("sess-1-main")).toBe(false);
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

    it("prefers blocked over generating", () => {
      setSplitLayout("sess-1", ["pane-a", "pane-b"]);
      updateAgentState("pane-a", { provider: "claude", status: "generating", source: "hook" });
      updateAgentState("pane-b", { provider: "claude", status: "blocked", source: "hook" });
      expect(getSessionAgentStatus("sess-1")).toBe("blocked");
    });

    it("prefers error over blocked and generating", () => {
      setSplitLayout("sess-1", ["pane-a", "pane-b", "pane-c"]);
      updateAgentState("pane-a", { provider: "claude", status: "generating", source: "hook" });
      updateAgentState("pane-b", { provider: "claude", status: "blocked", source: "hook" });
      updateAgentState("pane-c", { provider: "claude", status: "error", source: "hook" });
      expect(getSessionAgentStatus("sess-1")).toBe("error");
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

  describe("computeEffectiveSessionStatus", () => {
    it("disconnected session wins over any agent aggregate", () => {
      // A stale agentState entry that predates the disconnect must not
      // silently repaint the sidebar card as generating.
      expect(computeEffectiveSessionStatus("disconnected", "generating")).toBe(
        "disconnected",
      );
      expect(computeEffectiveSessionStatus("disconnected", "idle")).toBe(
        "disconnected",
      );
      expect(computeEffectiveSessionStatus("disconnected", null)).toBe(
        "disconnected",
      );
    });

    it("error session wins over any agent aggregate", () => {
      expect(computeEffectiveSessionStatus("error", "generating")).toBe("error");
      expect(computeEffectiveSessionStatus("error", "idle")).toBe("error");
      expect(computeEffectiveSessionStatus("error", null)).toBe("error");
    });

    it("live agent aggregate overrides a stale legacy status", () => {
      // Session-level thinking/idle/generating from the old path may be
      // stuck on a value we never cleared. A real agent update from a
      // pane supersedes it.
      expect(computeEffectiveSessionStatus("idle", "generating")).toBe(
        "generating",
      );
      expect(computeEffectiveSessionStatus("thinking", "generating")).toBe(
        "generating",
      );
      expect(computeEffectiveSessionStatus("generating", "idle")).toBe("idle");
      expect(computeEffectiveSessionStatus("idle", "blocked")).toBe("attention");
      expect(computeEffectiveSessionStatus("idle", "error")).toBe("error");
    });

    it("legacy field passes through when no agent aggregate is present", () => {
      expect(computeEffectiveSessionStatus("idle", null)).toBe("idle");
      expect(computeEffectiveSessionStatus("thinking", null)).toBe("thinking");
      expect(computeEffectiveSessionStatus("attention", null)).toBe("attention");
    });
  });
});
