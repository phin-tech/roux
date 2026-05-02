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
  saveLivePaneStateRaw: vi.fn(),
  deletePaneStateRaw: vi.fn(),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
}));

import {
  loadPaneStateRaw,
  savePaneStateRaw,
  saveLivePaneStateRaw,
  deletePaneStateRaw,
} from "$lib/tauri";
import { paneInstances } from "../instances";

describe("persistence — Tauri-backed API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    sessionLayouts.set(new Map());
    paneInstances.set(new Map());
    stopPersistence();
  });

  afterEach(() => {
    stopPersistence();
    vi.useRealTimers();
  });

  describe("loadPaneState", () => {
    it("returns parsed payload when Tauri call resolves with valid data", async () => {
      const payload: PaneStatePayload = {
        schemaVersion: 4,
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "shell", ptyId: "s1" }],
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

    it("drops payloads with a missing schemaVersion (pre-v3)", async () => {
      vi.mocked(loadPaneStateRaw).mockResolvedValue({
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "claude", ptyId: "s1" }],
      } as unknown);
      const result = await loadPaneState("s1");
      expect(result).toBeNull();
    });

    it("drops payloads with a lower schemaVersion", async () => {
      vi.mocked(loadPaneStateRaw).mockResolvedValue({
        schemaVersion: 2,
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "shell", ptyId: "s1" }],
      } as unknown);
      const result = await loadPaneState("s1");
      expect(result).toBeNull();
    });

    it("drops payloads with null children before they reach the renderer", async () => {
      vi.mocked(loadPaneStateRaw).mockResolvedValue({
        schemaVersion: 4,
        layout: {
          kind: "split",
          direction: "h",
          children: [
            { kind: "leaf", paneId: "s1-main" },
            null,
          ],
        },
        descriptors: [{ id: "s1-main", type: "shell", ptyId: "s1" }],
      } as unknown);

      const result = await loadPaneState("s1");

      expect(result).toBeNull();
    });

    it("drops payloads with activeIndex outside stacked children", async () => {
      vi.mocked(loadPaneStateRaw).mockResolvedValue({
        schemaVersion: 4,
        layout: {
          kind: "split",
          direction: "h",
          stacked: true,
          activeIndex: 3,
          children: [
            { kind: "leaf", paneId: "s1-main" },
            { kind: "leaf", paneId: "s1-shell" },
          ],
        },
        descriptors: [
          { id: "s1-main", type: "shell", ptyId: "s1" },
          { id: "s1-shell", type: "shell", ptyId: "pty-shell" },
        ],
      } as unknown);

      const result = await loadPaneState("s1");

      expect(result).toBeNull();
    });
  });

  describe("savePaneState", () => {
    it("delegates to Tauri with session id and payload", async () => {
      vi.mocked(savePaneStateRaw).mockResolvedValue(undefined);
      const payload: PaneStatePayload = {
        schemaVersion: 4,
        layout: { kind: "leaf", paneId: "s1-main" },
        descriptors: [{ id: "s1-main", type: "shell", ptyId: "s1" }],
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
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
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
      expect(saveLivePaneStateRaw).not.toHaveBeenCalled();

      // Advance past the 1500ms debounce window
      await vi.advanceTimersByTimeAsync(1600);
      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
    });

    it("flushPaneState cancels the pending timer and writes immediately", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-main" });
        return next;
      });

      // Flush before the debounce window elapses
      await flushPaneState();
      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);

      // Advancing time beyond the window should NOT produce a second call
      await vi.advanceTimersByTimeAsync(2000);
      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
    });

    it("asks Rust to persist the live pane snapshot instead of serializing descriptors in the frontend", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);

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
      await vi.runAllTimersAsync();

      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
      expect(saveLivePaneStateRaw).toHaveBeenCalledWith(
        "s1",
        4,
        {
          kind: "split",
          direction: "h",
          children: [
            { kind: "leaf", paneId: "s1-main" },
            { kind: "leaf", paneId: "s1-shell" },
          ],
        },
        ["s1-main", "s1-shell"],
      );
      expect(savePaneStateRaw).not.toHaveBeenCalled();
    });

    it("passes the current layout and leaf ids to Rust for live snapshot persistence", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);

      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "s1-shell" });
        return next;
      });

      await vi.advanceTimersByTimeAsync(1600);
      await vi.runAllTimersAsync();

      const [sessionId, schemaVersion, layout, paneIds] = vi.mocked(saveLivePaneStateRaw)
        .mock.calls[0] as [
        string,
        number,
        LayoutNode,
        string[],
      ];
      expect(sessionId).toBe("s1");
      expect(schemaVersion).toBe(4);
      expect(layout).toEqual({ kind: "leaf", paneId: "s1-shell" });
      expect(paneIds).toEqual(["s1-shell"]);
    });

    it("passes markdown pane ids to Rust without local descriptor rebuilding", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);

      initPersistence();

      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", { kind: "leaf", paneId: "doc-1" });
        return next;
      });

      await vi.advanceTimersByTimeAsync(1600);
      await vi.runAllTimersAsync();

      expect(saveLivePaneStateRaw).toHaveBeenCalledWith(
        "s1",
        4,
        { kind: "leaf", paneId: "doc-1" },
        ["doc-1"],
      );
    });

    it("does not save when the layout map republishes unchanged tree references", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
      sessionLayouts.set(
        new Map([["s1", { kind: "leaf", paneId: "s1-main" }]])
      );

      initPersistence();
      sessionLayouts.update((m) => new Map(m));

      await vi.advanceTimersByTimeAsync(2000);

      expect(saveLivePaneStateRaw).not.toHaveBeenCalled();
    });

    it("only marks sessions whose layout tree changed as dirty", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
      sessionLayouts.set(new Map([
        ["s1", { kind: "leaf", paneId: "s1-main" }],
        ["s2", { kind: "leaf", paneId: "s2-main" }],
      ]));

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
      await vi.runAllTimersAsync();

      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
      expect(saveLivePaneStateRaw).toHaveBeenCalledWith(
        "s1",
        4,
        {
          kind: "split",
          direction: "h",
          children: [
            { kind: "leaf", paneId: "s1-main" },
            { kind: "leaf", paneId: "s1-shell" },
          ],
        },
        ["s1-main", "s1-shell"],
      );
    });

    it("flushPaneState writes all current sessions even when nothing was marked dirty", async () => {
      // Regression: flush must still persist the current layout even when
      // nothing marked the session dirty locally.
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);

      sessionLayouts.set(
        new Map([["s1", { kind: "leaf", paneId: "s1-shell" }]])
      );

      initPersistence();

      // No mutation between subscribe and flush — dirtySessions is empty.
      await flushPaneState();

      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
      const [sessionId, schemaVersion, layout, paneIds] = vi.mocked(saveLivePaneStateRaw)
        .mock.calls[0] as [
        string,
        number,
        LayoutNode,
        string[],
      ];
      expect(sessionId).toBe("s1");
      expect(schemaVersion).toBe(4);
      expect(layout).toEqual({ kind: "leaf", paneId: "s1-shell" });
      expect(paneIds).toEqual(["s1-shell"]);
    });

    it("flushPaneState is a no-op when there are no sessions at all", async () => {
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
      initPersistence();

      await flushPaneState();
      expect(saveLivePaneStateRaw).not.toHaveBeenCalled();
    });

    it("does not save on initial subscribe — the first callback is the current value, not a mutation", async () => {
      // Simulates the startup race: sessionLayouts already contains restored
      // main-pane leaves by the time initPersistence() subscribes. The first
      // subscribe callback fires immediately with that value, but it's not a
      // real change — it's the initial state. Without the skip-first guard,
      // this would schedule a save that clobbers the persisted full layout.
      vi.mocked(saveLivePaneStateRaw).mockResolvedValue(undefined);
      sessionLayouts.set(
        new Map([["s1", { kind: "leaf", paneId: "s1-main" }]])
      );

      initPersistence();

      // Advance past the debounce window. No save should happen because the
      // subscribe-immediate callback is not treated as a mutation.
      await vi.advanceTimersByTimeAsync(2000);
      expect(saveLivePaneStateRaw).not.toHaveBeenCalled();

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
      expect(saveLivePaneStateRaw).toHaveBeenCalledTimes(1);
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
      { id: "p1", type: "shell", ptyId: "s1" },
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
      { id: "p1", type: "shell", ptyId: "s1" },
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
