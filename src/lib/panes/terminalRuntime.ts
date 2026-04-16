import { Channel } from "@tauri-apps/api/core";
import { readonly, writable } from "svelte/store";

import type { TerminalTheme } from "$lib/themes";
import { type PtyOutputPayload } from "$lib/tauri";

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

type TerminalControllerFactory = (
  options?: CreateTerminalControllerOptions,
) => TerminalController;

const paneTerminalRuntimes = new Map<string, PaneTerminalRuntime>();
const terminalRuntimeVersionStore = writable(0);

let terminalControllerFactory: TerminalControllerFactory | null = null;

function getPaneTerminalRuntime(paneId: string): PaneTerminalRuntime | undefined {
  return paneTerminalRuntimes.get(paneId);
}

function bumpTerminalRuntimeVersion(): void {
  terminalRuntimeVersionStore.update((version) => version + 1);
}

function getTerminalControllerFactory(): TerminalControllerFactory {
  if (!terminalControllerFactory) {
    throw new Error("Terminal controller factory has not been registered");
  }

  return terminalControllerFactory;
}

export const terminalRuntimeVersion = readonly(terminalRuntimeVersionStore);

export function registerTerminalControllerFactory(
  factory: TerminalControllerFactory,
): void {
  terminalControllerFactory = factory;
}

export function ensureTerminalController(
  paneId: string,
  options?: CreateTerminalControllerOptions,
): TerminalController {
  const existing = getPaneTerminalRuntime(paneId);
  if (existing?.controller) return existing.controller;

  const controller = getTerminalControllerFactory()(options);
  paneTerminalRuntimes.set(paneId, {
    controller,
    outputChannel: existing?.outputChannel ?? null,
  });
  bumpTerminalRuntimeVersion();
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
  const hadController = runtime.controller != null;
  runtime.controller?.dispose();
  paneTerminalRuntimes.delete(paneId);
  if (hadController) bumpTerminalRuntimeVersion();
}

export function resetPaneTerminalRuntimes(): void {
  let disposedControllers = false;
  for (const runtime of paneTerminalRuntimes.values()) {
    if (runtime.controller) disposedControllers = true;
    runtime.controller?.dispose();
  }
  paneTerminalRuntimes.clear();
  if (disposedControllers) bumpTerminalRuntimeVersion();
}
