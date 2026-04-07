import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  focusedPaneId,
  fullscreenPaneId,
  setLogicalFocus,
  toggleFullscreen,
  resetFocus,
} from "../focus";
import { createPane, resetInstances } from "../instances";

describe("focus", () => {
  beforeEach(() => {
    resetInstances();
    resetFocus();
  });

  it("setLogicalFocus updates focusedPaneId", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");
    expect(get(focusedPaneId)).toBe("p1");
  });

  it("setLogicalFocus(null) clears focus", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");
    setLogicalFocus(null);
    expect(get(focusedPaneId)).toBeNull();
  });

  it("toggleFullscreen sets and clears fullscreenPaneId", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");

    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBe("p1");

    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBeNull();
  });

  it("toggleFullscreen does nothing without focus", () => {
    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBeNull();
  });
});
