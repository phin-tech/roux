import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { createPane, disposePane, resetInstances } from "$lib/panes/instances";
import { focusedPaneId, resetFocus, setLogicalFocus } from "$lib/panes/focus";
import {
  closeCommandSurface,
  commandSurface,
  openCommandPalette,
  openLeaderPrompt,
  openLeaderMode,
  resetCommandSurface,
  setLeaderSequence,
  setLeaderPromptValue,
} from "../commandSurface";

describe("commandSurface", () => {
  beforeEach(() => {
    resetInstances();
    resetFocus();
    resetCommandSurface();
  });

  it("captures the focused pane when leader mode opens", () => {
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("pane-1");

    openLeaderMode();

    expect(get(commandSurface)).toEqual({
      open: true,
      mode: "leader",
      returnFocusPaneId: "pane-1",
      leaderSequence: [],
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    });
  });

  it("restores focus to the captured pane when leader mode closes", () => {
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("pane-1");
    openLeaderMode();
    setLogicalFocus(null);

    closeCommandSurface();

    expect(get(focusedPaneId)).toBe("pane-1");
    expect(get(commandSurface).open).toBe(false);
  });

  it("clears stale focus when the captured pane disappeared before close", () => {
    createPane({ id: "pane-1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("pane-1");
    openLeaderMode();
    disposePane("pane-1");

    closeCommandSurface();

    expect(get(focusedPaneId)).toBeNull();
    expect(get(commandSurface)).toEqual({
      open: false,
      mode: "palette",
      returnFocusPaneId: null,
      leaderSequence: [],
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    });
  });

  it("tracks and clears leader sequence state", () => {
    openLeaderMode();
    setLeaderSequence(["w"]);
    expect(get(commandSurface).leaderSequence).toEqual(["w"]);

    openCommandPalette();
    expect(get(commandSurface)).toEqual({
      open: true,
      mode: "palette",
      returnFocusPaneId: null,
      leaderSequence: [],
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    });
  });

  it("can step back through a nested leader sequence", () => {
    openLeaderMode();
    setLeaderSequence(["w", "v"]);
    expect(get(commandSurface).leaderSequence).toEqual(["w", "v"]);

    setLeaderSequence(["w"]);
    expect(get(commandSurface).leaderSequence).toEqual(["w"]);
  });

  it("tracks inline leader prompt state", () => {
    openLeaderMode();
    setLeaderSequence(["w", "r"]);
    openLeaderPrompt("pane.rename");
    setLeaderPromptValue("docs");

    expect(get(commandSurface)).toEqual({
      open: true,
      mode: "leader",
      returnFocusPaneId: null,
      leaderSequence: ["w", "r"],
      leaderPromptCommandId: "pane.rename",
      leaderPromptValue: "docs",
    });

    openCommandPalette();
    expect(get(commandSurface)).toEqual({
      open: true,
      mode: "palette",
      returnFocusPaneId: null,
      leaderSequence: [],
      leaderPromptCommandId: null,
      leaderPromptValue: "",
    });
  });
});
