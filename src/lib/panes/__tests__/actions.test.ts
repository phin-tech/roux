import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { closePane } from "../actions";
import {
  addSplit,
  focusedPaneId,
  initSessionPanes,
  paneTrees,
} from "$lib/stores/panes";

describe("pane close actions", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  it("runs shell cleanup before removing the pane", async () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });

    const events: string[] = [];
    const cleanupShellPane = vi.fn(async (paneId: string, ptyId: string) => {
      events.push(`cleanup:${paneId}:${ptyId}`);
      expect(get(paneTrees).get("s1")?.kind).toBe("split");
    });

    const closed = await closePane("s1", "shell-1", {
      cleanupShellPane,
    });

    expect(closed).toBe(true);
    expect(cleanupShellPane).toHaveBeenCalledWith("shell-1", "pty-1");
    expect(events).toEqual(["cleanup:shell-1:pty-1"]);
    expect(get(paneTrees).get("s1")?.kind).toBe("pane");
    expect(get(focusedPaneId)).toBe("s1-main");
  });

  it("removes document panes without invoking shell cleanup", async () => {
    initSessionPanes("s1");
    addSplit("s1", "horizontal", {
      id: "doc-1",
      type: "doc",
      ptyId: "",
      docPath: "/tmp/note.md",
    });

    const cleanupShellPane = vi.fn();

    const closed = await closePane("s1", "doc-1", {
      cleanupShellPane,
    });

    expect(closed).toBe(true);
    expect(cleanupShellPane).not.toHaveBeenCalled();
    expect(get(paneTrees).get("s1")?.kind).toBe("pane");
    expect(get(focusedPaneId)).toBe("s1-main");
  });

  it("does not close the main claude pane", async () => {
    initSessionPanes("s1");
    focusedPaneId.set("s1-main");

    const cleanupShellPane = vi.fn();

    const closed = await closePane("s1", "s1-main", {
      cleanupShellPane,
    });

    expect(closed).toBe(false);
    expect(cleanupShellPane).not.toHaveBeenCalled();
    expect(get(paneTrees).get("s1")?.kind).toBe("pane");
    expect(get(focusedPaneId)).toBe("s1-main");
  });
});
