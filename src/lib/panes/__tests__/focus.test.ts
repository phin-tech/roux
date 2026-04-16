import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";

const controllers = new Map<string, {
  setInputEnabled: ReturnType<typeof vi.fn>;
  focus: ReturnType<typeof vi.fn>;
}>();

vi.mock("../terminalRuntime", () => ({
  getTerminalController: vi.fn((paneId: string) => controllers.get(paneId) ?? null),
  clearPaneOutputChannel: vi.fn(),
  disposePaneTerminalRuntime: vi.fn(),
}));

import {
  focusedPaneId,
  fullscreenPaneId,
  setLogicalFocus,
  requestDomFocus,
  toggleFullscreen,
  resetFocus,
} from "../focus";
import { createPane, resetInstances } from "../instances";

describe("focus", () => {
  beforeEach(() => {
    controllers.clear();
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

  it("setLogicalFocus updates terminal input routing through the runtime registry", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    createPane({ id: "p2", type: "shell", ptyId: "pty-2" });
    const p1 = { setInputEnabled: vi.fn(), focus: vi.fn() };
    const p2 = { setInputEnabled: vi.fn(), focus: vi.fn() };
    controllers.set("p1", p1);
    controllers.set("p2", p2);

    setLogicalFocus("p1");

    expect(p1.setInputEnabled).toHaveBeenCalledWith(true);
    expect(p2.setInputEnabled).toHaveBeenCalledWith(false);
    expect(p1.focus).toHaveBeenCalled();
  });

  it("requestDomFocus forwards to the terminal runtime registry", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    const p1 = { setInputEnabled: vi.fn(), focus: vi.fn() };
    controllers.set("p1", p1);

    requestDomFocus("p1");

    expect(p1.focus).toHaveBeenCalled();
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
