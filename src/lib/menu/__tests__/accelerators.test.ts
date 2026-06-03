import { describe, expect, it } from "vitest";
import { toTauriAccelerator, eventToAccelerator } from "../accelerators";

describe("toTauriAccelerator", () => {
  it("translates lowercase cmd chord to CmdOrCtrl", () => {
    expect(toTauriAccelerator("cmd+n")).toBe("CmdOrCtrl+N");
  });

  it("preserves modifier order with cmd/shift/alt", () => {
    expect(toTauriAccelerator("shift+cmd+d")).toBe("CmdOrCtrl+Shift+D");
    expect(toTauriAccelerator("cmd+alt+c")).toBe("CmdOrCtrl+Alt+C");
  });

  it("handles the bare ctrl modifier", () => {
    expect(toTauriAccelerator("ctrl+shift+h")).toBe("Control+Shift+H");
  });

  it("substitutes punctuation for tao-compatible names", () => {
    expect(toTauriAccelerator("cmd+,")).toBe("CmdOrCtrl+Comma");
    expect(toTauriAccelerator("cmd+\\")).toBe("CmdOrCtrl+Backslash");
    expect(toTauriAccelerator("cmd+;")).toBe("CmdOrCtrl+Semicolon");
  });

  it("returns null for chord shortcuts", () => {
    expect(toTauriAccelerator("cmd+; b d")).toBeNull();
  });

  it("returns null for empty or modifier-only input", () => {
    expect(toTauriAccelerator(null)).toBeNull();
    expect(toTauriAccelerator("")).toBeNull();
    expect(toTauriAccelerator("cmd")).toBeNull();
    expect(toTauriAccelerator("cmd+shift")).toBeNull();
  });

  it("preserves multi-char function keys and names", () => {
    expect(toTauriAccelerator("cmd+F1")).toBe("CmdOrCtrl+F1");
    expect(toTauriAccelerator("cmd+Escape")).toBe("CmdOrCtrl+Escape");
  });
});

describe("eventToAccelerator", () => {
  const ev = (
    init: Partial<KeyboardEventInit> & { key: string },
  ): KeyboardEvent => new KeyboardEvent("keydown", init);

  it("builds the canonical form from a DOM event", () => {
    expect(eventToAccelerator(ev({ key: "n", metaKey: true }))).toBe(
      "CmdOrCtrl+N",
    );
  });

  it("matches the shortcut-format output for the same chord", () => {
    const fromShortcut = toTauriAccelerator("cmd+shift+d");
    const fromEvent = eventToAccelerator(
      ev({ key: "D", metaKey: true, shiftKey: true }),
    );
    expect(fromEvent).toBe(fromShortcut);
  });

  it("returns null for modifier-less keys (not an OS accelerator)", () => {
    expect(eventToAccelerator(ev({ key: "a" }))).toBeNull();
    expect(eventToAccelerator(ev({ key: "Escape" }))).toBeNull();
  });

  it("returns null for bare modifier presses", () => {
    expect(eventToAccelerator(ev({ key: "Meta", metaKey: true }))).toBeNull();
    expect(eventToAccelerator(ev({ key: "Shift", shiftKey: true }))).toBeNull();
  });

  it("collapses Meta and Control to the same CmdOrCtrl token", () => {
    const mac = eventToAccelerator(ev({ key: "k", metaKey: true }));
    const win = eventToAccelerator(ev({ key: "k", ctrlKey: true }));
    expect(mac).toBe("CmdOrCtrl+K");
    expect(win).toBe("CmdOrCtrl+K");
  });
});
