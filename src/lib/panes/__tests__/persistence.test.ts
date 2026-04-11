// src/lib/panes/__tests__/persistence.test.ts
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadPaneState,
  savePaneState,
  deletePaneState,
  flushPaneState,
  initPersistence,
  stopPersistence,
  stripCommandPanes,
  type PaneDescriptor,
  type PaneStatePayload,
} from "../persistence";
import type { LayoutNode } from "../layout";
import { sessionLayouts } from "../layout";

vi.mock("$lib/tauri", () => ({
  loadPaneStateRaw: vi.fn(),
  savePaneStateRaw: vi.fn(),
  deletePaneStateRaw: vi.fn(),
  getPtyCwd: vi.fn().mockResolvedValue(null),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
}));

import { loadPaneStateRaw, savePaneStateRaw, deletePaneStateRaw, getPtyCwd } from "$lib/tauri";
import { paneInstances, createPane } from "../instances";

describe("persistence — Tauri-backed API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    sessionLayouts.set(new Map());
    stopPersistence();
  });

  afterEach(() => {
    stopPersistence();
    vi.useRealTimers();
  });

  describe("loadPaneState", () => {
    it("returns parsed payload when Tauri call resolves with valid data", async () => {
      const payload: PaneStatePayload = {
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "claude", ptyId: "s1" }],
      };
      vi.mocked(loadPaneStateRaw).mockResolvedValue(payload);

      const result = await loadPaneState("s1");
      expect(result).toEqual(payload);
      expect(loadPaneStateRaw).toHaveBeenCalledWith("s1");
    });

    it("returns null when Tauri call resolves with null", async () => {
      vi.mocked(loadPaneStateRaw).mockResolvedValue(null);
      const result = await loadPaneState("s1");
      expect(result).toBeNull();
    });

    it("returns null (does not throw) when Tauri call rejects", async () => {
      vi.mocked(loadPaneStateRaw).mockRejectedValue(new Error("disk error"));
      const result = await loadPaneState("s1");
      expect(result).toBeNull();
    });
  });

  describe("savePaneState", () => {
    it("delegates to Tauri with session id and payload", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      const payload: PaneStatePayload = {
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "claude", ptyId: "s1" }],
      };
      await savePaneState("s1", payload);
      expect(savePaneStateRaw).toHaveBeenCalledWith("s1", payload);
    });
  });

  describe("deletePaneState", () => {
    it("delegates to Tauri with session id", async () => {
      vi.mocked(deletePaneStateRaw).mockResolvedValue(undefined);
      await deletePaneState("s1");
      expect(deletePaneStateRaw).toHaveBeenCalledWith("s1");
    });
  });

  describe("initPersistence / debounce / flushPaneState", () => {
    it("debounces saves — 3 rapid changes within window produce one Tauri call", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      initPersistence();

      // Trigger three rapid layout changes
      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-main" });
        return next;
      });
      sessionLayouts.update((m) => new Map(m));
      sessionLayouts.update((m) => new Map(m));

      // No save yet — debounce window not elapsed
      expect(savePaneStateRaw).not.toHaveBeenCalled();

      // Advance past the 1500ms debounce window
      await vi.advanceTimersByTimeAsync(1600);
      expect(savePaneStateRaw).toHaveBeenCalledTimes(1);
    });

    it("flushPaneState cancels the pending timer and writes immediately", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-main" });
        return next;
      });

      // Flush before the debounce window elapses
      await flushPaneState();
      expect(savePaneStateRaw).toHaveBeenCalledTimes(1);

      // Advancing time beyond the window should NOT produce a second call
      await vi.advanceTimersByTimeAsync(2000);
      expect(savePaneStateRaw).toHaveBeenCalledTimes(1);
    });

    it("resolves live cwd from Tauri for shell panes when saving", async () => {
      // A shell pane was spawned in /tmp/original, then the user `cd`'d to
      // /tmp/live. At save time we should record the live cwd so reconnect
      // restores where the user actually is.
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      vi.mocked(getPtyCwd).mockImplementation(async (id: string) => {
        if (id === "pty-shell-1") return "/tmp/live";
        return null;
      });

      // Set up a pane instance for the shell. Claude pane is the main leaf.
      paneInstances.set(new Map());
      createPane({
        id: "s1-main",
        type: "claude",
        ptyId: "pty-claude-1",
      });
      createPane({
        id: "s1-shell",
        type: "shell",
        ptyId: "pty-shell-1",
        workingDir: "/tmp/original",
      });

      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", {
          kind: "split",
          direction: "h",
          children: [
            { kind: "leaf", paneId: "s1-main" },
            { kind: "leaf", paneId: "s1-shell" },
          ],
        });
        return next;
      });

      await vi.advanceTimersByTimeAsync(1600);
      // Flush any pending microtasks from the awaited getPtyCwd call.
      await vi.runAllTimersAsync();

      expect(getPtyCwd).toHaveBeenCalledWith("pty-shell-1");
      expect(savePaneStateRaw).toHaveBeenCalledTimes(1);
      const [, payload] = vi.mocked(savePaneStateRaw).mock.calls[0] as [
        string,
        { descriptors: PaneDescriptor[]; layout: LayoutNode },
      ];
      const shellDesc = payload.descriptors.find((d) => d.id === "s1-shell");
      expect(shellDesc?.workingDir).toBe("/tmp/live");
    });

    it("falls back to the stored workingDir if Tauri returns null", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      vi.mocked(getPtyCwd).mockResolvedValue(null);

      paneInstances.set(new Map());
      createPane({
        id: "s1-shell",
        type: "shell",
        ptyId: "pty-dead",
        workingDir: "/tmp/fallback",
      });

      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-shell" });
        return next;
      });

      await vi.advanceTimersByTimeAsync(1600);
      await vi.runAllTimersAsync();

      const [, payload] = vi.mocked(savePaneStateRaw).mock.calls[0] as [
        string,
        { descriptors: PaneDescriptor[]; layout: LayoutNode },
      ];
      const shellDesc = payload.descriptors.find((d) => d.id === "s1-shell");
      expect(shellDesc?.workingDir).toBe("/tmp/fallback");
    });

    it("does not query getPtyCwd for non-shell panes", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      vi.mocked(getPtyCwd).mockResolvedValue("/should/not/be/used");

      paneInstances.set(new Map());
      createPane({
        id: "s1-main",
        type: "claude",
        ptyId: "pty-claude",
        workingDir: "/tmp/claude-cwd",
      });

      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-main" });
        return next;
      });

      await vi.advanceTimersByTimeAsync(1600);
      await vi.runAllTimersAsync();

      expect(getPtyCwd).not.toHaveBeenCalled();
      const [, payload] = vi.mocked(savePaneStateRaw).mock.calls[0] as [
        string,
        { descriptors: PaneDescriptor[]; layout: LayoutNode },
      ];
      const mainDesc = payload.descriptors.find((d) => d.id === "s1-main");
      expect(mainDesc?.workingDir).toBe("/tmp/claude-cwd");
    });

    it("flushPaneState is a no-op when nothing is dirty", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      initPersistence();

      await flushPaneState();
      expect(savePaneStateRaw).not.toHaveBeenCalled();
    });

    it("does not save on initial subscribe — the first callback is the current value, not a mutation", async () => {
      // Simulates the startup race: sessionLayouts already contains restored
      // main-pane leaves by the time initPersistence() subscribes. The first
      // subscribe callback fires immediately with that value, but it's not a
      // real change — it's the initial state. Without the skip-first guard,
      // this would schedule a save that clobbers the persisted full layout.
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      sessionLayouts.set(
        new Map([["s1", { kind: "leaf", paneId: "s1-main" }]])
      );

      initPersistence();

      // Advance past the debounce window. No save should happen because the
      // subscribe-immediate callback is not treated as a mutation.
      await vi.advanceTimersByTimeAsync(2000);
      expect(savePaneStateRaw).not.toHaveBeenCalled();

      // A subsequent real mutation still schedules a save.
      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", {
          kind: "split",
          direction: "h",
          children: [
            { kind: "leaf", paneId: "s1-main" },
            { kind: "leaf", paneId: "s1-shell" },
          ],
        });
        return next;
      });
      await vi.advanceTimersByTimeAsync(1600);
      expect(savePaneStateRaw).toHaveBeenCalledTimes(1);
    });
  });
});

describe("persistence — stripCommandPanes (unchanged)", () => {
  it("removes command leaves from tree and descriptors", () => {
    const tree: LayoutNode = {
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        { kind: "leaf", paneId: "cmd-1" },
      ],
    };
    const descs: PaneDescriptor[] = [
      { id: "p1", type: "claude", ptyId: "s1" },
      { id: "cmd-1", type: "command", ptyId: "pty-cmd", command: "npm test" },
    ];
    const result = stripCommandPanes(tree, descs);
    expect(result.tree).toEqual({ kind: "leaf", paneId: "p1" });
    expect(result.descriptors).toEqual([descs[0]]);
  });

  it("returns null tree when all panes are commands", () => {
    const tree: LayoutNode = { kind: "leaf", paneId: "cmd-1" };
    const descs: PaneDescriptor[] = [
      { id: "cmd-1", type: "command", ptyId: "pty-cmd", command: "npm test" },
    ];
    const result = stripCommandPanes(tree, descs);
    expect(result.tree).toBeNull();
  });

  it("collapses single-child splits after stripping", () => {
    const tree: LayoutNode = {
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        {
          kind: "split",
          direction: "v",
          children: [
            { kind: "leaf", paneId: "p2" },
            { kind: "leaf", paneId: "cmd-1" },
          ],
        },
      ],
    };
    const descs: PaneDescriptor[] = [
      { id: "p1", type: "claude", ptyId: "s1" },
      { id: "p2", type: "shell", ptyId: "pty-2" },
      { id: "cmd-1", type: "command", ptyId: "pty-cmd", command: "npm test" },
    ];
    const result = stripCommandPanes(tree, descs);
    expect(result.tree).toEqual({
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        { kind: "leaf", paneId: "p2" },
      ],
    });
  });
});
