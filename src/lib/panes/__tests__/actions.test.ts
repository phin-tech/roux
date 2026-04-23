import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";
import { settings } from "$lib/stores/settings";
import { DEFAULT_SETTINGS } from "$lib/types";

// Stub $lib/tauri so we can observe which kill/detach primitive disposePane
// chose. The real invoke() would just reject silently in jsdom; this mock
// lets the pane-disposal path reach the assertion surface.
vi.mock("$lib/tauri", () => ({
  killPty: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn().mockResolvedValue(undefined),
  detachPty: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

import {
  splitPane,
  closePane,
  closeFocusedPane,
  closeSessionPanes,
  initSession,
} from "../actions";
import { paneInstances, resetInstances, getInstance } from "../instances";
import { sessionLayouts, resetLayouts, collectLeafIds } from "../layout";
import { focusedPaneId, fullscreenPaneId, resetFocus, setLogicalFocus, toggleFullscreen } from "../focus";
import { killPty, killSession, detachPty } from "$lib/tauri";

describe("pane actions", () => {
  beforeEach(() => {
    resetInstances();
    resetLayouts();
    resetFocus();
    fullscreenPaneId.set(null);
    settings.set(DEFAULT_SETTINGS); // resets to onPaneClose: "kill"
    vi.mocked(killPty).mockClear();
    vi.mocked(killSession).mockClear();
    vi.mocked(detachPty).mockClear();
  });

  describe("initSession", () => {
    it("creates a shell pane instance tagged with the claude built-in profile and layout", () => {
      const mainId = initSession("s1");
      expect(mainId).toBe("s1-main");

      const tree = get(sessionLayouts).get("s1");
      expect(tree?.kind).toBe("leaf");

      const inst = getInstance("s1-main");
      expect(inst).toBeDefined();
      expect(inst!.type).toBe("shell");
      expect(inst!.ptyId).toBe("s1");
      expect(inst!.spawnProfileRef).toEqual({ kind: "registered", id: "claude" });
    });

    it("focuses the main pane", () => {
      initSession("s1");
      expect(get(focusedPaneId)).toBe("s1-main");
    });

    it("is idempotent — does not reinitialize", () => {
      initSession("s1");
      initSession("s1");
      expect(collectLeafIds(get(sessionLayouts).get("s1")!)).toHaveLength(1);
    });
  });

  describe("splitPane", () => {
    it("creates a new pane and inserts into layout", () => {
      initSession("s1");
      const newId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      expect(newId).not.toBeNull();
      const tree = get(sessionLayouts).get("s1")!;
      expect(tree.kind).toBe("split");
      const ids = collectLeafIds(tree);
      expect(ids).toHaveLength(2);
      for (const id of ids) {
        expect(get(paneInstances).has(id)).toBe(true);
      }
    });

    it("focuses the new pane after split", () => {
      initSession("s1");
      const newId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });
      expect(get(focusedPaneId)).toBe(newId);
      expect(getInstance(newId!)!.type).toBe("shell");
    });

    it("seeds a single-leaf layout when splitting into a zero-pane session", () => {
      // A session whose last pane was closed has no layout entry. Splitting
      // into it must re-populate the layout with the new pane as sole
      // leaf — spec allows zero-pane sessions as a transient state and
      // dropping the split on the floor would strand the user.
      const newId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });
      expect(newId).not.toBeNull();
      const tree = get(sessionLayouts).get("s1");
      expect(tree).toEqual({ kind: "leaf", paneId: newId });
      expect(get(paneInstances).has(newId!)).toBe(true);
    });
  });

  describe("closePane", () => {
    it("removes pane from layout and disposes instance", () => {
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;

      const closed = closePane("s1", shellId);
      expect(closed).toBe(true);
      expect(get(paneInstances).has(shellId)).toBe(false);
      expect(get(sessionLayouts).get("s1")!.kind).toBe("leaf");
    });

    it("closes the primary pane and leaves the session with zero panes", () => {
      initSession("s1");
      const closed = closePane("s1", "s1-main");
      expect(closed).toBe(true);
      expect(get(paneInstances).has("s1-main")).toBe(false);
      expect(get(sessionLayouts).has("s1")).toBe(false);
    });

    it("uses killPty (not killSession) by default", () => {
      // Default behaviour: closing a pane kills its PTY. This makes "close"
      // behave like close rather than hide.
      initSession("s1");
      closePane("s1", "s1-main");
      expect(killPty).toHaveBeenCalledWith("s1");
      expect(detachPty).not.toHaveBeenCalled();
      expect(killSession).not.toHaveBeenCalled();
    });

    it("detaches PTY (not kills) when onPaneClose is explicitly 'detach'", () => {
      settings.set({ ...DEFAULT_SETTINGS, onPaneClose: "detach" });
      initSession("s1");
      closePane("s1", "s1-main");
      expect(detachPty).toHaveBeenCalledWith("s1");
      expect(killPty).not.toHaveBeenCalled();
      expect(killSession).not.toHaveBeenCalled();
    });

    it("uses killPty (not killSession) when onPaneClose is 'kill'", () => {
      // Regression guard: phase 4 made disposePane kill PTYs for every shell,
      // which destroyed sessions when the primary pane was closed because
      // killSession removed the session record as a side effect. Pane
      // disposal must only touch the PTY.
      settings.set({ ...DEFAULT_SETTINGS, onPaneClose: "kill" });
      initSession("s1");
      closePane("s1", "s1-main");
      expect(killPty).toHaveBeenCalledWith("s1");
      expect(killSession).not.toHaveBeenCalled();
    });

    it("uses killPty when onPaneClose is 'kill' and disposing a shell pane", () => {
      settings.set({ ...DEFAULT_SETTINGS, onPaneClose: "kill" });
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;
      closePane("s1", shellId);
      expect(killPty).toHaveBeenCalledWith("pty-1");
      expect(killSession).not.toHaveBeenCalled();
    });

    it("moves focus when closing focused pane", () => {
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;
      // shellId is focused after split

      closePane("s1", shellId);
      expect(get(focusedPaneId)).not.toBeNull();
      expect(get(focusedPaneId)).not.toBe(shellId);
    });

    it("returns false for nonexistent pane", () => {
      initSession("s1");
      expect(closePane("s1", "nope")).toBe(false);
    });

    it("clears fullscreenPaneId when the fullscreened pane is closed", () => {
      // Regression: closePane disposed the pane and cleared focus but left
      // fullscreenPaneId pointing at a dead id. SplitPane then filtered
      // every sibling out of the DOM and the content area went blank.
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;
      setLogicalFocus(shellId);
      toggleFullscreen();
      expect(get(fullscreenPaneId)).toBe(shellId);

      closePane("s1", shellId);
      expect(get(fullscreenPaneId)).toBeNull();
    });

    it("leaves fullscreenPaneId alone when a non-fullscreened pane is closed", () => {
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;
      setLogicalFocus("s1-main");
      toggleFullscreen();
      expect(get(fullscreenPaneId)).toBe("s1-main");

      closePane("s1", shellId);
      expect(get(fullscreenPaneId)).toBe("s1-main");
    });

    it("refocuses onto the visible leaf (not a hidden stacked tab)", () => {
      // Regression: firstLeafId walks children[0] without consulting the
      // stack's activeIndex, so closing a sibling while a stack had tab 1
      // visible routed focus to the hidden tab at index 0.
      initSession("s1");
      const stackTab2 = splitPane("s1", "v", { type: "shell", ptyId: "pty-a" })!;
      const rightPane = splitPane("s1", "h", { type: "shell", ptyId: "pty-b" })!;
      // Build the target layout directly: an outer horizontal split whose
      // left child is a stacked split (s1-main, stackTab2) with tab 2
      // currently visible, and the right child is rightPane.
      sessionLayouts.update((m) => {
        const next = new Map(m);
        next.set("s1", {
          kind: "split",
          direction: "h",
          children: [
            {
              kind: "split",
              direction: "v",
              stacked: true,
              activeIndex: 1,
              children: [
                { kind: "leaf", paneId: "s1-main" },
                { kind: "leaf", paneId: stackTab2 },
              ],
            },
            { kind: "leaf", paneId: rightPane },
          ],
        });
        return next;
      });

      setLogicalFocus(rightPane);
      closePane("s1", rightPane);

      // Focus must land on the visible stack tab, not children[0].
      expect(get(focusedPaneId)).toBe(stackTab2);
    });
  });

  describe("closeFocusedPane", () => {
    it("closes the currently focused pane", () => {
      initSession("s1");
      const shellId = splitPane("s1", "h", { type: "shell", ptyId: "pty-1" })!;
      // shellId is focused

      const closed = closeFocusedPane("s1");
      expect(closed).toBe(true);
      expect(get(paneInstances).has(shellId)).toBe(false);
    });

    it("returns false if nothing focused", () => {
      resetFocus();
      expect(closeFocusedPane("s1")).toBe(false);
    });
  });

  describe("closeSessionPanes", () => {
    it("disposes all panes and removes layout", () => {
      initSession("s1");
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const ids = collectLeafIds(get(sessionLayouts).get("s1")!);

      closeSessionPanes("s1");

      expect(get(sessionLayouts).has("s1")).toBe(false);
      for (const id of ids) {
        expect(get(paneInstances).has(id)).toBe(false);
      }
    });

    it("clears focus if focused pane was in the session", () => {
      initSession("s1");
      closeSessionPanes("s1");
      expect(get(focusedPaneId)).toBeNull();
    });
  });
});
