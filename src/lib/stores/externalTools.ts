import { get, writable } from "svelte/store";
import type { ExternalTool, ExternalToolSurface, ExternalToolWebEmbedder } from "$lib/bindings";
import {
  daemonProcessKill,
  daemonProcessOutput,
  killPty,
  launchExternalTool,
  type ExternalToolLaunchResult,
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
  | "error";

export interface ExternalToolRun {
  id: string;
  toolId: string;
  toolName: string;
  surface: ExternalToolSurface;
  webEmbedder: ExternalToolWebEmbedder;
  sessionId: string | null;
  runtimeId: string | null;
  runtimeGeneration: number | null;
  rendered: RenderedExternalTool | null;
  status: ExternalToolRunStatus;
  error: string | null;
  logsOpen: boolean;
  launchedAtMs: number;
}

export const externalToolRuns = writable<Map<string, ExternalToolRun>>(new Map());
const externalToolViewClosers = new Map<string, () => void>();
const externalToolLaunchTokens = new Map<string, number>();
const externalToolRelaunchCleanupTokens = new Map<string, number>();
let nextExternalToolLaunchToken = 0;
let nextExternalToolRelaunchCleanupToken = 0;

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
  if (existing) {
    const relaunchToken = markExternalToolRelaunching(runId);
    await killRunRuntime(existing);
    if (!externalToolRelaunchCleanupIsCurrent(runId, relaunchToken)) return;
    finishExternalToolRelaunchCleanup(runId, relaunchToken);
  }
  await launchRun(tool, boundSessionId, existing?.id);
}

export async function restartExternalToolRun(runId: string): Promise<void> {
  const run = get(externalToolRuns).get(runId);
  if (!run) return;
  if (externalToolRelaunchCleanupTokens.has(runId)) return;
  const tool = findTool(run.toolId);
  if (run.runtimeId) {
    const relaunchToken = markExternalToolRelaunching(runId);
    await killRunRuntime(run);
    if (!externalToolRelaunchCleanupIsCurrent(runId, relaunchToken)) return;
    finishExternalToolRelaunchCleanup(runId, relaunchToken);
  } else {
    closeExternalToolView(runId);
    cancelExternalToolLaunch(runId);
  }
  await launchRun(tool, run.sessionId, runId);
}

function markExternalToolRelaunching(runId: string): number {
  const relaunchToken = ++nextExternalToolRelaunchCleanupToken;
  closeExternalToolView(runId);
  cancelExternalToolLaunch(runId);
  externalToolRelaunchCleanupTokens.set(runId, relaunchToken);
  updateRun(runId, (run) => ({
    ...run,
    runtimeId: null,
    runtimeGeneration: null,
    rendered: null,
    status: "launching",
    error: null,
    logsOpen: false,
  }));
  return relaunchToken;
}

function externalToolRelaunchCleanupIsCurrent(runId: string, token: number): boolean {
  return externalToolRelaunchCleanupTokens.get(runId) === token;
}

function finishExternalToolRelaunchCleanup(runId: string, token: number): void {
  if (externalToolRelaunchCleanupIsCurrent(runId, token)) {
    externalToolRelaunchCleanupTokens.delete(runId);
  }
}

export async function closeExternalToolRun(runId: string): Promise<void> {
  const run = get(externalToolRuns).get(runId);
  removeExternalToolRun(runId);
  if (run) await killRunRuntime(run);
}

export function registerExternalToolViewCloser(runId: string, closeView: () => void): () => void {
  externalToolViewClosers.set(runId, closeView);
  return () => {
    if (externalToolViewClosers.get(runId) === closeView) {
      externalToolViewClosers.delete(runId);
    }
  };
}

function removeExternalToolRun(runId: string): void {
  closeExternalToolView(runId);
  cancelExternalToolLaunch(runId);
  externalToolRelaunchCleanupTokens.delete(runId);
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

function closeExternalToolView(runId: string): void {
  try {
    externalToolViewClosers.get(runId)?.();
  } catch {
    // The component can already be mid-destroy; store cleanup should still continue.
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
  if (run.status === "error") return;
  if (generation != null && run.runtimeGeneration !== generation) return;

  void killRunRuntime(run);
  removeExternalToolRun(runId);
}

export function setExternalToolRunError(
  runId: string,
  error: string,
  runtimeId?: string | null,
  generation?: number | null,
): void {
  if (runtimeId !== undefined) {
    const run = get(externalToolRuns).get(runId);
    if (!run || run.runtimeId !== (runtimeId ?? null)) return;
    if (generation != null && run.runtimeGeneration !== generation) return;
  }
  updateRun(runId, (run) => ({ ...run, status: "error", error, logsOpen: true }));
}

export async function failExternalToolRun(
  runId: string,
  runtimeId: string | null | undefined,
  error: string,
): Promise<void> {
  const run = get(externalToolRuns).get(runId);
  if (!run) return;
  if (runtimeId && run.runtimeId !== runtimeId) return;
  if (!runtimeId && run.runtimeId) return;

  setExternalToolRunError(runId, error);
  await killRunRuntime(run);
}

export function setExternalToolLogsOpen(runId: string, logsOpen: boolean): void {
  updateRun(runId, (run) => ({ ...run, logsOpen }));
}

export async function readExternalToolProcess(run: ExternalToolRun): Promise<ProcessSnapshot | null> {
  if (run.surface !== "web" || !run.runtimeId) return null;
  return daemonProcessOutput(run.runtimeId, 64 * 1024);
}

export function externalToolRunIsLive(run: ExternalToolRun): boolean {
  return run.status !== "error";
}

async function launchRun(
  tool: ExternalTool,
  sessionId: string | null,
  existingRunId?: string,
): Promise<void> {
  const runId = existingRunId ?? externalToolRunId(tool.id, sessionId);
  const launchToken = beginExternalToolLaunch(runId);
  const surface = tool.surface ?? "terminal";
  const base: ExternalToolRun = {
    id: runId,
    toolId: tool.id,
    toolName: tool.name,
    surface,
    webEmbedder: externalToolWebEmbedder(tool),
    sessionId,
    runtimeId: null,
    runtimeGeneration: null,
    rendered: null,
    status: "launching",
    error: null,
    logsOpen: false,
    launchedAtMs: Date.now(),
  };
  externalToolRuns.update((runs) => new Map(runs).set(runId, base));
  openMainView({ kind: "externalTool", runId });

  try {
    const result = await launchExternalTool(tool.id, sessionId);
    if (!externalToolLaunchIsCurrent(runId, launchToken)) {
      await killLaunchResultRuntime(result);
      return;
    }
    updateRun(runId, (run) => ({
      ...run,
      surface: result.surface,
      webEmbedder: externalToolWebEmbedder(tool),
      runtimeId: result.runtimeId,
      runtimeGeneration: result.runtimeGeneration ?? null,
      rendered: result.rendered,
      status: result.surface === "web" ? "starting" : "running",
      error: null,
      launchedAtMs: Date.now(),
    }));
  } catch (err) {
    if (externalToolLaunchIsCurrent(runId, launchToken)) {
      setExternalToolRunError(runId, formatError(err));
    }
  }
}

function beginExternalToolLaunch(runId: string): number {
  const token = ++nextExternalToolLaunchToken;
  externalToolLaunchTokens.set(runId, token);
  return token;
}

function cancelExternalToolLaunch(runId: string): void {
  externalToolLaunchTokens.delete(runId);
}

function externalToolLaunchIsCurrent(runId: string, token: number): boolean {
  return externalToolLaunchTokens.get(runId) === token;
}

function resolveBoundSessionId(tool: ExternalTool): string | null {
  if (!tool.requiresSession) return null;
  const session = get(activeSession);
  if (!session) throw new Error("External tool requires an active session");
  return session.id;
}

function externalToolWebEmbedder(tool: ExternalTool): ExternalToolWebEmbedder {
  if ((tool.surface ?? "terminal") !== "web") return "webview";
  return tool.webEmbedder ?? "webview";
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

async function killLaunchResultRuntime(result: ExternalToolLaunchResult): Promise<void> {
  if (!result.runtimeId) return;
  try {
    if (result.surface === "terminal") {
      await killPty(result.runtimeId);
    } else {
      await daemonProcessKill(result.runtimeId);
    }
  } catch {
    // The process may have already exited or been cleaned up by the daemon.
  }
}

function formatError(err: unknown): string {
  return err instanceof Error && err.message ? err.message : String(err);
}
