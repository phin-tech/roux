import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneInstances,
  createPane,
  disposePane,
  resetInstances,
  updateInstance,
} from "../instances";

describe("pane instances", () => {
  beforeEach(() => {
    resetInstances();
  });

  it("createPane adds an instance to the store", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    const instances = get(paneInstances);
    expect(instances.has(id)).toBe(true);
    const inst = instances.get(id)!;
    expect(inst.type).toBe("shell");
    expect(inst.ptyId).toBe("pty-1");
  });

  it("createPane accepts an explicit id", () => {
    const id = createPane({ id: "my-pane", type: "shell", ptyId: "s1" });
    expect(id).toBe("my-pane");
    expect(get(paneInstances).has("my-pane")).toBe(true);
  });

  it("createPane stores optional metadata", () => {
    const id = createPane({
      type: "command",
      ptyId: "pty-2",
      name: "test cmd",
      command: "npm test",
      workingDir: "/tmp",
    });
    const inst = get(paneInstances).get(id)!;
    expect(inst.name).toBe("test cmd");
    expect(inst.command).toBe("npm test");
    expect(inst.workingDir).toBe("/tmp");
  });

  it("createPane for markdown has no terminal", () => {
    const id = createPane({ type: "markdown", ptyId: "", docPath: "/tmp/a.md" });
    const inst = get(paneInstances).get(id)!;
    expect(inst.terminal).toBeNull();
    expect(inst.fitAddon).toBeNull();
  });

  it("disposePane removes the instance", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    disposePane(id);
    expect(get(paneInstances).has(id)).toBe(false);
  });

  it("disposePane is idempotent", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    disposePane(id);
    disposePane(id); // no error
    expect(get(paneInstances).has(id)).toBe(false);
  });

  it("disposePane cleans up unlisteners", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    let cleaned = false;
    const inst = get(paneInstances).get(id)!;
    inst.unlisteners.push(() => { cleaned = true; });
    disposePane(id);
    expect(cleaned).toBe(true);
  });

  describe("restoreError field", () => {
    it("createPane defaults restoreError to undefined", () => {
      const id = createPane({ type: "shell", ptyId: "pty-1" });
      const inst = get(paneInstances).get(id)!;
      expect(inst.restoreError).toBeUndefined();
    });

    it("updateInstance can set restoreError", () => {
      const id = createPane({ type: "shell", ptyId: "pty-1" });
      updateInstance(id, { restoreError: "working directory not found: /gone" });
      const inst = get(paneInstances).get(id)!;
      expect(inst.restoreError).toBe("working directory not found: /gone");
    });

    it("updateInstance can clear restoreError on retry success", () => {
      const id = createPane({ type: "shell", ptyId: "pty-1" });
      updateInstance(id, { restoreError: "some error" });
      updateInstance(id, { restoreError: undefined });
      const inst = get(paneInstances).get(id)!;
      expect(inst.restoreError).toBeUndefined();
    });
  });
});
