import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  discoverTasks: vi.fn(),
  loadTaskOverrides: vi.fn(),
  saveTaskOverrides: vi.fn().mockResolvedValue(undefined),
}));

import {
  taskGroups,
  taskRuns,
  taskOverrides,
  refreshTasks,
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  getEffectiveKeepOpen,
  setKeepOpenOverride,
  initTaskOverrides,
  clearDiscoveryCache,
} from "../tasks";
import { discoverTasks, loadTaskOverrides } from "$lib/tauri";
import type { TaskGroup, TaskRun } from "$lib/types/tasks";

describe("task stores", () => {
  beforeEach(() => {
    taskGroups.set([]);
    taskRuns.set(new Map());
    taskOverrides.set({});
    clearDiscoveryCache();
    vi.mocked(discoverTasks).mockReset();
    vi.mocked(loadTaskOverrides).mockReset();
  });

  describe("initTaskOverrides", () => {
    it("loads and sets overrides from tauri", async () => {
      const overrides = { "/repo": { "npm:build": "always" } };
      vi.mocked(loadTaskOverrides).mockResolvedValue(overrides);

      await initTaskOverrides();

      expect(get(taskOverrides)).toEqual(overrides);
    });
  });

  describe("refreshTasks", () => {
    it("discovers tasks and updates store", async () => {
      const groups: TaskGroup[] = [
        {
          runner: "npm scripts",
          configFile: "package.json",
          tasks: [
            { id: "npm:build", name: "build", description: "", runner: "npm", command: "npm run build", keepOpen: "on-error" },
          ],
        },
      ];
      vi.mocked(discoverTasks).mockResolvedValue(groups);

      await refreshTasks("/repo");

      expect(discoverTasks).toHaveBeenCalledWith("/repo");
      expect(get(taskGroups)).toEqual(groups);
    });

    it("caches results per repo root", async () => {
      const groups: TaskGroup[] = [
        { runner: "npm scripts", configFile: "package.json", tasks: [] },
      ];
      vi.mocked(discoverTasks).mockResolvedValue(groups);

      await refreshTasks("/repo");
      await refreshTasks("/repo");

      expect(discoverTasks).toHaveBeenCalledTimes(1);
    });
  });

  describe("task runs", () => {
    it("adds and retrieves a task run", () => {
      const run: TaskRun = {
        taskId: "npm:build",
        paneId: null,
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        outputLines: [],
        startedAt: 1000,
      };
      addTaskRun("session-1", run);

      const runs = get(taskRuns).get("session-1");
      expect(runs).toEqual([run]);
    });

    it("updates a task run status", () => {
      addTaskRun("session-1", {
        taskId: "npm:build",
        paneId: null,
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        outputLines: [],
        startedAt: 1000,
      });

      updateTaskRun("session-1", "pty-1", 0);

      const runs = get(taskRuns).get("session-1")!;
      expect(runs[0].status).toBe("succeeded");
      expect(runs[0].exitCode).toBe(0);
    });

    it("marks nonzero exit as failed", () => {
      addTaskRun("session-1", {
        taskId: "npm:test",
        paneId: null,
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        outputLines: [],
        startedAt: 1000,
      });

      updateTaskRun("session-1", "pty-1", 1);

      const runs = get(taskRuns).get("session-1")!;
      expect(runs[0].status).toBe("failed");
    });

    it("removes a task run", () => {
      addTaskRun("session-1", {
        taskId: "npm:build",
        paneId: null,
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        outputLines: [],
        startedAt: 1000,
      });
      removeTaskRun("session-1", "pty-1");

      const runs = get(taskRuns).get("session-1");
      expect(runs).toEqual([]);
    });
  });

  describe("keepOpen overrides", () => {
    it("returns default when no override exists", () => {
      expect(getEffectiveKeepOpen("/repo", "npm:build", "on-error")).toBe("on-error");
    });

    it("returns override when set", () => {
      setKeepOpenOverride("/repo", "npm:build", "always");
      expect(getEffectiveKeepOpen("/repo", "npm:build", "on-error")).toBe("always");
    });
  });
});
