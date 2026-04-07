// src/lib/panes/__tests__/persistence.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  savePaneDescriptors,
  loadPaneDescriptors,
  saveLayout,
  loadLayout,
  clearLayout,
  clearPaneDescriptors,
  stripCommandPanes,
  type PaneDescriptor,
} from "../persistence";
import type { LayoutNode } from "../layout";

// Mock localStorage
const storage = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, val: string) => storage.set(key, val),
  removeItem: (key: string) => storage.delete(key),
});

describe("persistence", () => {
  beforeEach(() => {
    storage.clear();
  });

  describe("layout", () => {
    it("round-trips a layout tree", () => {
      const tree: LayoutNode = {
        kind: "split",
        direction: "h",
        children: [
          { kind: "leaf", paneId: "p1" },
          { kind: "leaf", paneId: "p2" },
        ],
      };
      saveLayout("s1", tree);
      expect(loadLayout("s1")).toEqual(tree);
    });

    it("returns null for unknown session", () => {
      expect(loadLayout("nope")).toBeNull();
    });

    it("clearLayout removes a session", () => {
      const tree: LayoutNode = { kind: "leaf", paneId: "p1" };
      saveLayout("s1", tree);
      clearLayout("s1");
      expect(loadLayout("s1")).toBeNull();
    });
  });

  describe("descriptors", () => {
    it("round-trips pane descriptors", () => {
      const descs: PaneDescriptor[] = [
        { id: "p1", type: "claude", ptyId: "s1" },
        { id: "p2", type: "shell", ptyId: "pty-2", name: "test" },
      ];
      savePaneDescriptors("s1", descs);
      expect(loadPaneDescriptors("s1")).toEqual(descs);
    });

    it("returns null for unknown session", () => {
      expect(loadPaneDescriptors("nope")).toBeNull();
    });

    it("clearPaneDescriptors removes a session", () => {
      savePaneDescriptors("s1", [{ id: "p1", type: "claude", ptyId: "s1" }]);
      clearPaneDescriptors("s1");
      expect(loadPaneDescriptors("s1")).toBeNull();
    });
  });

  describe("stripCommandPanes", () => {
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
      // The inner split should collapse to just p2
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
});
