import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  spawnShell: vi.fn().mockResolvedValue(undefined),
  writeToSession: vi.fn().mockResolvedValue(undefined),
  onSessionExit: vi.fn().mockResolvedValue(() => {}),
  onPtyOutput: vi.fn().mockResolvedValue(() => {}),
  discoverTasks: vi.fn().mockResolvedValue([]),
  loadTaskOverrides: vi.fn().mockResolvedValue({}),
  saveTaskOverrides: vi.fn().mockResolvedValue(undefined),
}));

import { runTask, expandTask } from "../runner";
import { spawnShell, writeToSession, onSessionExit, onPtyOutput } from "$lib/tauri";
import { taskRuns } from "$lib/stores/tasks";
import { paneTrees, focusedPaneId, initSessionPanes } from "$lib/stores/panes";
import type { TaskDefinition } from "$lib/types/tasks";

describe("runTask", () => {
  beforeEach(() => {
    taskRuns.set(new Map());
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    vi.mocked(spawnShell).mockClear();
    vi.mocked(writeToSession).mockClear();
    vi.mocked(onSessionExit).mockClear();
    vi.mocked(onPtyOutput).mockClear();
  });

  const task: TaskDefinition = {
    id: "npm:build",
    name: "build",
    description: "Build the project",
    runner: "npm",
    command: "npm run build",
    keepOpen: "on-error",
  };

  it("spawns a shell and writes the command without creating a pane", async () => {
    await runTask("session-1", "/repo", task);

    expect(spawnShell).toHaveBeenCalledTimes(1);
    const ptyId = vi.mocked(spawnShell).mock.calls[0][0];
    expect(ptyId).toContain("task-session-1-npm:build-");
    expect(writeToSession).toHaveBeenCalledWith(ptyId, "npm run build\n");

    // No pane should be created
    const tree = get(paneTrees).get("session-1");
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

  it("subscribes to PTY output and session exit", async () => {
    await runTask("session-1", "/repo", task);

    expect(onPtyOutput).toHaveBeenCalledTimes(1);
    expect(onSessionExit).toHaveBeenCalledTimes(1);
  });
});

describe("expandTask", () => {
  beforeEach(() => {
    taskRuns.set(new Map());
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  it("creates a pane and updates the task run paneId", async () => {
    initSessionPanes("session-1");

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

    // Pane tree should now have a split
    const tree = get(paneTrees).get("session-1");
    expect(tree?.kind).toBe("split");

    // Task run should have paneId set
    const runs = get(taskRuns).get("session-1");
    expect(runs![0].paneId).toBe("task-pty-1");
  });
});
