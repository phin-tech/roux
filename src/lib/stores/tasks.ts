import { writable, get } from "svelte/store";
import { discoverTasks, loadTaskOverrides, saveTaskOverrides } from "$lib/tauri";
import type { TaskGroup, TaskRun, KeepOpen } from "$lib/types/tasks";

export const taskGroups = writable<TaskGroup[]>([]);
export const taskRuns = writable<Map<string, TaskRun[]>>(new Map());
export const taskOverrides = writable<Record<string, Record<string, string>>>({});

const discoveryCache = new Map<string, TaskGroup[]>();

export function clearDiscoveryCache() {
  discoveryCache.clear();
}

export async function refreshTasks(repoRoot: string, force = false) {
  if (!force && discoveryCache.has(repoRoot)) {
    taskGroups.set(discoveryCache.get(repoRoot)!);
    return;
  }
  const groups = await discoverTasks(repoRoot);
  discoveryCache.set(repoRoot, groups);
  taskGroups.set(groups);
}

export async function initTaskOverrides() {
  const overrides = await loadTaskOverrides();
  taskOverrides.set(overrides);
}

export function addTaskRun(sessionId: string, run: TaskRun) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId) ?? [];
    runs.push(run);
    map.set(sessionId, runs);
    return new Map(map);
  });
}

export function updateTaskRun(sessionId: string, ptyId: string, exitCode: number | null) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    const idx = runs.findIndex((r) => r.ptyId === ptyId);
    if (idx === -1) return map;
    const updated = [...runs];
    updated[idx] = { ...runs[idx], exitCode, status: exitCode === 0 ? "succeeded" : "failed" };
    const next = new Map(map);
    next.set(sessionId, updated);
    return next;
  });
}

export function removeTaskRun(sessionId: string, ptyId: string) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    map.set(
      sessionId,
      runs.filter((r) => r.ptyId !== ptyId)
    );
    return new Map(map);
  });
}

export function getEffectiveKeepOpen(
  repoRoot: string,
  taskId: string,
  defaultKeepOpen: KeepOpen
): KeepOpen {
  const overrides = get(taskOverrides);
  const repoOverrides = overrides[repoRoot];
  if (repoOverrides && repoOverrides[taskId]) {
    return repoOverrides[taskId] as KeepOpen;
  }
  return defaultKeepOpen;
}

export function setKeepOpenOverride(repoRoot: string, taskId: string, keepOpen: KeepOpen) {
  taskOverrides.update((overrides) => {
    const repoOverrides = overrides[repoRoot] ?? {};
    repoOverrides[taskId] = keepOpen;
    overrides[repoRoot] = repoOverrides;
    return { ...overrides };
  });
  const current = get(taskOverrides);
  saveTaskOverrides(current).catch(() => {});
}

const MAX_OUTPUT_LINES = 200;

export function appendTaskOutput(sessionId: string, ptyId: string, text: string) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    const idx = runs.findIndex((r) => r.ptyId === ptyId);
    if (idx === -1) return map;
    const run = runs[idx];
    const newLines = text.split("\n");
    let lines = [...run.outputLines, ...newLines];
    if (lines.length > MAX_OUTPUT_LINES) {
      lines = lines.slice(lines.length - MAX_OUTPUT_LINES);
    }
    const updated = [...runs];
    updated[idx] = { ...run, outputLines: lines };
    const next = new Map(map);
    next.set(sessionId, updated);
    return next;
  });
}

export function setTaskPaneId(sessionId: string, ptyId: string, paneId: string) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    const run = runs.find((r) => r.ptyId === ptyId);
    if (run) run.paneId = paneId;
    return new Map(map);
  });
}

export function getTaskRun(sessionId: string, taskId: string): TaskRun | undefined {
  const runs = get(taskRuns).get(sessionId);
  return runs?.find((r) => r.taskId === taskId);
}
