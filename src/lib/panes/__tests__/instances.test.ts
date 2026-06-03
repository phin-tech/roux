import { describe, it, expect, beforeEach, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));
import {
  paneInstances,
  createPane,
  disposePane,
  resetInstances,
  updateInstance,
} from "../instances";
import { upsertPaneRecord } from "$lib/tauri";

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

  it("createPane stores pane metadata without terminal runtime internals", () => {
    const id = createPane({
      type: "markdown",
      ptyId: "",
      docPath: "/tmp/a.md",
    });
    const inst = get(paneInstances).get(id)!;
    expect(inst).toEqual({
      id,
      type: "markdown",
      ptyId: "",
      unlisteners: [],
      docPath: "/tmp/a.md",
      commandStatus: "idle",
      commandExitCode: null,
      commandStartedAt: null,
      elapsedTimer: null,
    });
  });

  it("syncs only stable pane descriptor facts to the backend record store", async () => {
    const id = createPane({
      type: "shell",
      ptyId: "pty-1",
      workingDir: "/tmp",
    });

    updateInstance(id, {
      restoreError: "missing dir",
      commandStatus: "running",
      commandExitCode: 1,
      commandStartedAt: 123,
    });

    await Promise.resolve();

    const payload = vi.mocked(upsertPaneRecord).mock.calls.at(-1)?.[0];
    expect(payload).toMatchObject({
      id,
      type: "shell",
      ptyId: "pty-1",
      workingDir: "/tmp",
    });
    expect(payload).not.toHaveProperty("restoreError");
    expect(payload).not.toHaveProperty("commandStatus");
    expect(payload).not.toHaveProperty("commandExitCode");
    expect(payload).not.toHaveProperty("commandStartedAt");
  });

  it("syncs provider session metadata to the backend record store", async () => {
    const id = createPane({
      type: "shell",
      ptyId: "pty-1",
      provider: "claude",
      providerSessionId: "claude-session-123",
    });

    await Promise.resolve();

    const payload = vi.mocked(upsertPaneRecord).mock.calls.at(-1)?.[0];
    expect(payload).toMatchObject({
      id,
      provider: "claude",
      providerSessionId: "claude-session-123",
    });
  });

  it("does not upsert the backend record when only UI runtime fields change", async () => {
    const id = createPane({
      type: "shell",
      ptyId: "pty-1",
      workingDir: "/tmp",
    });
    vi.mocked(upsertPaneRecord).mockClear();

    updateInstance(id, {
      restoreError: "missing dir",
      commandStatus: "running",
      commandExitCode: 1,
      commandStartedAt: 123,
    });

    await Promise.resolve();

    expect(upsertPaneRecord).not.toHaveBeenCalled();
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
    inst.unlisteners.push(() => {
      cleaned = true;
    });
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
      updateInstance(id, {
        restoreError: "working directory not found: /gone",
      });
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
