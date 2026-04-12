import { describe, expect, it } from "vitest";
import {
  getVisibleLeaderHints,
  normalizeLeaderKey,
  resolveLeaderSequence,
} from "../leader";

describe("leader", () => {
  it("returns root hints for an empty sequence", () => {
    expect(resolveLeaderSequence([])).toEqual({
      kind: "pending",
      title: "Leader",
      hints: [
        { key: "w", label: "panes" },
        { key: "b", label: "sessions" },
        { key: "n", label: "notes" },
        { key: "i", label: "inbox" },
        { key: "t", label: "watches" },
        { key: ",", label: "settings" },
        { key: "SPC", label: "commands" },
      ],
    });
  });

  it("resolves pane prefix sequences as pending", () => {
    expect(resolveLeaderSequence(["w"])).toEqual({
      kind: "pending",
      title: "Panes",
      hints: [
        { key: "h", label: "left" },
        { key: "j", label: "down" },
        { key: "k", label: "up" },
        { key: "l", label: "right" },
        { key: "s", label: "split" },
        { key: "v", label: "vsplit" },
        { key: "r", label: "rename" },
        { key: "d", label: "close" },
        { key: "f", label: "full" },
        { key: "t", label: "stack" },
      ],
    });
  });

  it("resolves terminal actions to command ids", () => {
    expect(resolveLeaderSequence(["w", "v"])).toEqual({
      kind: "command",
      commandId: "pane.split-vertical",
    });
    expect(resolveLeaderSequence(["w", "r"])).toEqual({
      kind: "command",
      commandId: "pane.rename",
    });
    expect(resolveLeaderSequence(["b", "n"])).toEqual({
      kind: "command",
      commandId: "session.new",
    });
  });

  it("prunes pane hints by command availability without changing order", () => {
    expect(
      getVisibleLeaderHints(["w"], (commandId) =>
        !["pane.rename", "pane.close", "pane.toggle-fullscreen", "pane.toggle-stack"].includes(commandId),
      ),
    ).toEqual([
      { key: "h", label: "left" },
      { key: "j", label: "down" },
      { key: "k", label: "up" },
      { key: "l", label: "right" },
      { key: "s", label: "split" },
      { key: "v", label: "vsplit" },
    ]);
  });

  it("leaves root hints unpruned for now", () => {
    expect(
      getVisibleLeaderHints([], () => false),
    ).toEqual([
      { key: "w", label: "panes" },
      { key: "b", label: "sessions" },
      { key: "n", label: "notes" },
      { key: "i", label: "inbox" },
      { key: "t", label: "watches" },
      { key: ",", label: "settings" },
      { key: "SPC", label: "commands" },
    ]);
  });

  it("resolves space to the full command palette", () => {
    expect(resolveLeaderSequence(["space"])).toEqual({ kind: "palette" });
  });

  it("marks unknown sequences invalid", () => {
    expect(resolveLeaderSequence(["x"])).toEqual({ kind: "invalid" });
    expect(resolveLeaderSequence(["w", "x"])).toEqual({ kind: "invalid" });
  });

  it("normalizes plain leader keys", () => {
    expect(normalizeLeaderKey(new KeyboardEvent("keydown", { key: "w" }))).toBe("w");
    expect(normalizeLeaderKey(new KeyboardEvent("keydown", { key: " " }))).toBe("space");
    expect(normalizeLeaderKey(new KeyboardEvent("keydown", { key: "," }))).toBe(",");
  });

  it("ignores modified keys inside leader mode", () => {
    expect(normalizeLeaderKey(new KeyboardEvent("keydown", { key: "w", metaKey: true }))).toBeNull();
    expect(normalizeLeaderKey(new KeyboardEvent("keydown", { key: "w", ctrlKey: true }))).toBeNull();
  });
});
