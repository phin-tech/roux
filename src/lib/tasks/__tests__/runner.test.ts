import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  spawnShell: vi.fn().mockResolvedValue(undefined),
  writeToSession: vi.fn().mockResolvedValue(undefined),
  onSessionExit: vi.fn().mockResolvedValue(() => {}),
  discoverTasks: vi.fn().mockResolvedValue([]),
  loadTaskOverrides: vi.fn().mockResolvedValue({}),
  saveTaskOverrides: vi.fn().mockResolvedValue(undefined),
}));

import { runTask } from "../runner";
import { spawnShell, writeToSession, onSessionExit } from "$lib/tauri";
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
  });

  const task: TaskDefinition = {
    id: "npm:build",
    name: "build",
    description: "Build the project",
    runner: "npm",
    command: "npm run build",
    keepOpen: "on-error",
  };

  it("spawns a shell and writes the command", async () => {
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    expect(spawnShell).toHaveBeenCalledTimes(1);
    const ptyId = vi.mocked(spawnShell).mock.calls[0][0];
    expect(ptyId).toContain("task-session-1-npm:build-");
    expect(writeToSession).toHaveBeenCalledWith(ptyId, "npm run build\n");
  });

  it("adds a task run to the store", async () => {
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    const runs = get(taskRuns).get("session-1");
    expect(runs).toHaveLength(1);
    expect(runs![0].taskId).toBe("npm:build");
    expect(runs![0].status).toBe("running");
  });

  it("listens for session exit", async () => {
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    expect(onSessionExit).toHaveBeenCalledTimes(1);
  });
});
