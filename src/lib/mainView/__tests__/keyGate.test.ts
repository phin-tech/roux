import { describe, expect, it } from "vitest";
import {
  commandBlockedByMainView,
  eventTargetIsEditable,
  eventTargetIsMainViewKeyboardOwner,
  eventTargetIsInsideMainView,
  mainViewTargetShouldBypassAppKeymap,
} from "../keyGate";
import type { Command } from "$lib/commands/registry";

function command(overrides: Partial<Command>): Command {
  return {
    id: "app.test",
    label: "Test",
    category: "App",
    ...overrides,
  };
}

describe("main-view key gate", () => {
  it("blocks pane commands by category or id prefix", () => {
    expect(commandBlockedByMainView(command({ id: "pane.split-right", category: "Panes" }))).toBe(true);
    expect(commandBlockedByMainView(command({ id: "pane.open-doc", category: "Documents" }))).toBe(true);
    expect(commandBlockedByMainView(command({ id: "session.next", category: "Sessions" }))).toBe(false);
    expect(commandBlockedByMainView(command({ id: "ui.toggle-board", category: "App" }))).toBe(false);
  });

  it("detects editable targets that should own Escape", () => {
    const input = document.createElement("input");
    const textarea = document.createElement("textarea");
    const select = document.createElement("select");
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    const emptyEditable = document.createElement("div");
    emptyEditable.setAttribute("contenteditable", "");
    const plaintextEditable = document.createElement("div");
    plaintextEditable.setAttribute("contenteditable", "plaintext-only");
    const inheritedEditable = document.createElement("div");
    inheritedEditable.setAttribute("contenteditable", "true");
    const inheritedChild = document.createElement("span");
    inheritedEditable.appendChild(inheritedChild);
    const disabledEditable = document.createElement("div");
    disabledEditable.setAttribute("contenteditable", "false");
    const codeMirror = document.createElement("div");
    codeMirror.className = "cm-editor";
    const button = document.createElement("button");

    expect(eventTargetIsEditable(input)).toBe(true);
    expect(eventTargetIsEditable(textarea)).toBe(true);
    expect(eventTargetIsEditable(select)).toBe(true);
    expect(eventTargetIsEditable(editable)).toBe(true);
    expect(eventTargetIsEditable(emptyEditable)).toBe(true);
    expect(eventTargetIsEditable(plaintextEditable)).toBe(true);
    expect(eventTargetIsEditable(inheritedChild)).toBe(true);
    expect(eventTargetIsEditable(disabledEditable)).toBe(false);
    expect(eventTargetIsEditable(codeMirror)).toBe(true);
    expect(eventTargetIsEditable(button)).toBe(false);
  });

  it("detects focus inside the main-view root", () => {
    const root = document.createElement("div");
    root.dataset.mainViewRoot = "";
    const child = document.createElement("button");
    root.appendChild(child);

    expect(eventTargetIsInsideMainView(child)).toBe(true);
    expect(eventTargetIsInsideMainView(document.createElement("button"))).toBe(false);
  });

  it("lets non-editable main-view focus continue to app shortcut handling", () => {
    const root = document.createElement("div");
    root.dataset.mainViewRoot = "";
    const button = document.createElement("button");
    const input = document.createElement("input");
    root.append(button, input);

    expect(eventTargetIsMainViewKeyboardOwner(button)).toBe(true);
    expect(mainViewTargetShouldBypassAppKeymap(button)).toBe(false);
    expect(mainViewTargetShouldBypassAppKeymap(input)).toBe(true);
  });
});
