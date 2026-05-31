import { get, writable } from "svelte/store";
import type { ExternalTool, ExternalToolSurface } from "$lib/bindings";
import {
  daemonProcessKill,
  daemonProcessOutput,
  killPty,
  launchExternalTool,
  type ProcessSnapshot,
  type RenderedExternalTool,
} from "$lib/tauri";
import { activeSession, sessionList } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";
import { closeMainView, mainViewRoute, openMainView } from "$lib/stores/mainView";

export type ExternalToolRunStatus =
  | "launching"
  | "starting"
  | "running"
  | "ready"
  | "exited"
  | "error";

export interface ExternalToolRun {
  id: string;
  toolId: string;
  toolName: string;
  surface: ExternalToolSurface;
  sessionId: string | null;
  runtimeId: string | null;
  runtimeGeneration: number | null;
  rendered: RenderedExternalTool | null;
  status: ExternalToolRunStatus;
  error: string | null;
  exitCode: number | null;
  logsOpen: boolean;
  launchedAtMs: number;
}

export const externalToolRuns = writable<Map<string, ExternalToolRun>>(new Map());

export function externalToolRunId(toolId: string, sessionId: string | null): string {
  return `${toolId}:${sessionId ?? "global"}`;
}

export function externalToolDisabledReason(tool: ExternalTool): string | null {
  if (tool.enabled === false) return "Disabled in Preferences";
  if (tool.requiresSession && !get(activeSession)) return "Requires an active session";
  if ((tool.surface ?? "terminal") === "web" && !tool.urlTemplate?.trim()) {
    return "Web tools require a URL template";
  }
  return null;
}

export function listEnabledExternalTools(): ExternalTool[] {
  return (get(settings).externalTools ?? []).filter((tool) => tool.enabled !== false);
}

export async function openExternalTool(toolId: string): Promise<void> {
  const tool = findTool(toolId);
  const boundSessionId = resolveBoundSessionId(tool);
  const runId = externalToolRunId(tool.id, boundSessionId);
  const existing = get(externalToolRuns).get(runId);
  if (existing && externalToolRunIsLive(existing)) {
    openMainView({ kind: "externalTool", runId });
    return;
  }
  await launchRun(tool, boundSessionId, existing?.id);
}

export async function restartExternalToolRun(runId: string): Promise<void> {
  const run = get(externalToolRuns).get(runId);
  if (!run) return;
  const tool = findTool(run.toolId);
  await killRunRuntime(run);
  await launchRun(tool, run.sessionId, runId);
}

export async function closeExternalToolRun(runId: string): Promise<void> {
  const run = get(externalToolRuns).get(runId);
  if (run) await killRunRuntime(run);
  removeExternalToolRun(runId);
}

function removeExternalToolRun(runId: string): void {
  externalToolRuns.update((runs) => {
    const next = new Map(runs);
    next.delete(runId);
    return next;
  });
  const route = get(mainViewRoute);
  if (route?.kind === "externalTool" && route.runId === runId) {
    closeMainView();
  }
}

export function markExternalToolReady(runId: string): void {
  updateRun(runId, (run) => ({ ...run, status: "ready", error: null }));
}

export function markExternalToolExited(
  runId: string,
  runtimeId: string | null | undefined,
  _exitCode: number | null,
  generation?: number | null,
): void {
  if (!runtimeId) return;
  const run = get(externalToolRuns).get(runId);
  if (!run || run.runtimeId !== runtimeId) return;
  if (generation != null && run.runtimeGeneration !== generation) return;

  void killRunRuntime(run);
  removeExternalToolRun(runId);
}

export function setExternalToolRunError(runId: string, error: string): void {
  updateRun(runId, (run) => ({ ...run, status: "error", error, logsOpen: true }));
}

export function setExternalToolLogsOpen(runId: string, logsOpen: boolean): void {
  updateRun(runId, (run) => ({ ...run, logsOpen }));
}

export async function readExternalToolProcess(run: ExternalToolRun): Promise<ProcessSnapshot | null> {
  if (run.surface !== "web" || !run.runtimeId) return null;
  return daemonProcessOutput(run.runtimeId, 64 * 1024);
}

export function externalToolRunIsLive(run: ExternalToolRun): boolean {
  return run.status !== "exited" && run.status !== "error";
}

async function launchRun(
  tool: ExternalTool,
  sessionId: string | null,
  existingRunId?: string,
): Promise<void> {
  const runId = existingRunId ?? externalToolRunId(tool.id, sessionId);
  const surface = tool.surface ?? "terminal";
  const base: ExternalToolRun = {
    id: runId,
    toolId: tool.id,
    toolName: tool.name,
    surface,
    sessionId,
    runtimeId: null,
    runtimeGeneration: null,
    rendered: null,
    status: "launching",
    error: null,
    exitCode: null,
    logsOpen: false,
    launchedAtMs: Date.now(),
  };
  externalToolRuns.update((runs) => new Map(runs).set(runId, base));
  openMainView({ kind: "externalTool", runId });

  try {
    const result = await launchExternalTool(tool.id, sessionId);
    updateRun(runId, (run) => ({
      ...run,
      surface: result.surface,
      runtimeId: result.runtimeId,
      runtimeGeneration: result.runtimeGeneration ?? null,
      rendered: result.rendered,
      status: result.surface === "web" ? "starting" : "running",
      error: null,
      exitCode: null,
      launchedAtMs: Date.now(),
    }));
  } catch (err) {
    setExternalToolRunError(runId, formatError(err));
  }
}

function resolveBoundSessionId(tool: ExternalTool): string | null {
  if (!tool.requiresSession) return null;
  const session = get(activeSession);
  if (!session) throw new Error("External tool requires an active session");
  return session.id;
}

function findTool(toolId: string): ExternalTool {
  const tool = (get(settings).externalTools ?? []).find((candidate) => candidate.id === toolId);
  if (!tool) throw new Error(`External tool "${toolId}" not found`);
  if (tool.enabled === false) throw new Error(`External tool "${tool.name}" is disabled`);
  if (tool.requiresSession) {
    const sessionId = get(activeSession)?.id ?? null;
    if (!sessionId || !get(sessionList).some((session) => session.id === sessionId)) {
      throw new Error("External tool requires an active session");
    }
  }
  return tool;
}

function updateRun(runId: string, updater: (run: ExternalToolRun) => ExternalToolRun): void {
  externalToolRuns.update((runs) => {
    const current = runs.get(runId);
    if (!current) return runs;
    const next = new Map(runs);
    next.set(runId, updater(current));
    return next;
  });
}

async function killRunRuntime(run: ExternalToolRun): Promise<void> {
  if (!run.runtimeId) return;
  try {
    if (run.surface === "terminal") {
      await killPty(run.runtimeId);
    } else {
      await daemonProcessKill(run.runtimeId);
    }
  } catch {
    // The process may have already exited or been cleaned up by the daemon.
  }
}

function formatError(err: unknown): string {
  return err instanceof Error && err.message ? err.message : String(err);
}
