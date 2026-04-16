import { Channel } from "@tauri-apps/api/core";
import { writable, type Readable } from "svelte/store";

import type { TerminalTheme } from "$lib/themes";
import { type PtyOutputPayload } from "$lib/tauri";

import { createXtermTerminalController } from "./xtermController";

export interface TerminalDimensions {
  cols: number;
  rows: number;
}

export interface TerminalController {
  attach(container: HTMLElement): void;
  detach(): void;
  dispose(): void;
  clear(): void;
  reset(): void;
  fit(): TerminalDimensions | null;
  setInputEnabled(enabled: boolean): void;
  onInput(handler: (data: string) => void): () => void;
  write(bytes: Uint8Array): void;
  focus(): void;
  setTheme(theme: TerminalTheme): void;
  setCustomKeyHandler(handler: ((event: KeyboardEvent) => boolean) | null): void;
}

interface PaneTerminalRuntime {
  controller: TerminalController | null;
  outputChannel: Channel<PtyOutputPayload> | null;
}

interface CreateTerminalControllerOptions {
  allowKeyboardEvent?: (event: KeyboardEvent) => boolean;
}

const paneTerminalRuntimes = new Map<string, PaneTerminalRuntime>();

// Reactivity bridge: the runtime Map itself lives outside Svelte's reactivity
// graph, but PaneShell's attach/detach effect needs to re-run when a pane's
// controller appears or disappears. Bumping this counter on every
// controller create/dispose lets consumer effects subscribe via
// `$terminalRuntimeVersion` and re-evaluate their `getTerminalController()`
// lookup.
const terminalRuntimeVersion = writable(0);
export const terminalRuntimeVersionStore: Readable<number> = {
  subscribe: terminalRuntimeVersion.subscribe,
};

function bumpVersion(): void {
  terminalRuntimeVersion.update((v) => v + 1);
}

function getPaneTerminalRuntime(paneId: string): PaneTerminalRuntime | undefined {
  return paneTerminalRuntimes.get(paneId);
}

export function ensureTerminalController(
  paneId: string,
  options?: CreateTerminalControllerOptions,
): TerminalController {
  const existing = getPaneTerminalRuntime(paneId);
  if (existing?.controller) return existing.controller;

  const controller = createXtermTerminalController(options);
  paneTerminalRuntimes.set(paneId, {
    controller,
    outputChannel: existing?.outputChannel ?? null,
  });
  bumpVersion();
  return controller;
}

export function getTerminalController(paneId: string): TerminalController | null {
  return getPaneTerminalRuntime(paneId)?.controller ?? null;
}

export function getPaneOutputChannel(paneId: string): Channel<PtyOutputPayload> | null {
  return getPaneTerminalRuntime(paneId)?.outputChannel ?? null;
}

export function setPaneOutputChannel(
  paneId: string,
  outputChannel: Channel<PtyOutputPayload> | null,
): void {
  const runtime = getPaneTerminalRuntime(paneId);
  if (runtime) {
    runtime.outputChannel = outputChannel;
    return;
  }

  paneTerminalRuntimes.set(paneId, {
    controller: null,
    outputChannel,
  });
}

export function clearPaneOutputChannel(paneId: string): void {
  const runtime = getPaneTerminalRuntime(paneId);
  if (runtime) runtime.outputChannel = null;
}

export function disposePaneTerminalRuntime(paneId: string): void {
  const runtime = getPaneTerminalRuntime(paneId);
  if (!runtime) return;
  runtime.controller?.dispose();
  paneTerminalRuntimes.delete(paneId);
  bumpVersion();
}

export function resetPaneTerminalRuntimes(): void {
  for (const runtime of paneTerminalRuntimes.values()) {
    runtime.controller?.dispose();
  }
  paneTerminalRuntimes.clear();
  bumpVersion();
}
