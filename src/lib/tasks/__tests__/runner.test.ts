import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  spawnTask: vi.fn().mockResolvedValue(undefined),
  spawnShell: vi.fn().mockResolvedValue(undefined),
  writeToSession: vi.fn().mockResolvedValue(undefined),
  onSessionExit: vi.fn().mockResolvedValue(() => {}),
  attachPtyOutput: vi.fn().mockResolvedValue(undefined),
  createPtyOutputChannel: vi.fn((callback: (data: Uint8Array) => void) => ({ callback })),
  discoverTasks: vi.fn().mockResolvedValue([]),
  loadTaskOverrides: vi.fn().mockResolvedValue({}),
  saveTaskOverrides: vi.fn().mockResolvedValue(undefined),
}));

import { runTask, expandTask } from "../runner";
import { spawnTask, onSessionExit, attachPtyOutput, createPtyOutputChannel } from "$lib/tauri";
import { taskRuns } from "$lib/stores/tasks";
import { sessionLayouts, resetLayouts } from "$lib/panes/layout";
import { resetFocus } from "$lib/panes/focus";
import { resetInstances } from "$lib/panes/instances";
import { initSession } from "$lib/panes/actions";
import type { TaskDefinition } from "$lib/types/tasks";

describe("runTask", () => {
  beforeEach(() => {
    taskRuns.set(new Map());
    resetLayouts();
    resetInstances();
    resetFocus();
    vi.mocked(spawnTask).mockClear();
    vi.mocked(onSessionExit).mockClear();
    vi.mocked(attachPtyOutput).mockClear();
    vi.mocked(createPtyOutputChannel).mockClear();
  });

  const task: TaskDefinition = {
    id: "npm:build",
    name: "build",
    description: "Build the project",
    runner: "npm",
    command: "npm run build",
    keepOpen: "on-error",
  };

  it("spawns a task command without creating a pane", async () => {
    await runTask("session-1", "/repo", task);

    expect(spawnTask).toHaveBeenCalledTimes(1);
    const [ptyId, command, workingDir] = vi.mocked(spawnTask).mock.calls[0];
    expect(ptyId).toContain("task-session-1-npm-build-");
    expect(command).toBe("npm run build");
    expect(workingDir).toBe("/repo");

    // No layout should be created (session was never init'd)
    const tree = get(sessionLayouts).get("session-1");
    expect(tree).toBeUndefined();
  });

  it("adds a task run with null paneId and empty outputLines", async () => {
    await runTask("session-1", "/repo", task);

    const runs = get(taskRuns).get("session-1");
    expect(runs).toHaveLength(1);
    expect(runs![0].taskId).toBe("npm:build");
    expect(runs![0].status).toBe("running");
    expect(runs![0].paneId).toBeNull();
    expect(runs![0].outputLines).toEqual([]);
  });

  it("attaches PTY output and session exit", async () => {
    await runTask("session-1", "/repo", task);

    expect(createPtyOutputChannel).toHaveBeenCalledTimes(1);
    expect(attachPtyOutput).toHaveBeenCalledTimes(1);
    expect(onSessionExit).toHaveBeenCalledTimes(1);
  });
});

describe("expandTask", () => {
  beforeEach(() => {
    taskRuns.set(new Map());
    resetLayouts();
    resetInstances();
    resetFocus();
  });

  it("creates a pane and updates the task run paneId", async () => {
    initSession("session-1");

    // Simulate a running task
    const { addTaskRun } = await import("$lib/stores/tasks");
    addTaskRun("session-1", {
      taskId: "npm:build",
      ptyId: "task-pty-1",
      paneId: null,
      status: "running",
      exitCode: null,
      outputLines: ["output line 1"],
      startedAt: 1000,
    });

    expandTask("session-1", "task-pty-1");

    // Layout should now have a split
    const tree = get(sessionLayouts).get("session-1");
    expect(tree?.kind).toBe("split");

    // Task run should have paneId set
    const runs = get(taskRuns).get("session-1");
    expect(runs![0].paneId).toBeDefined();
  });
});
