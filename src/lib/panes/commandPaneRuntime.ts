import { connectPaneTerminal } from "$lib/panes/terminals";
import {
  clearPaneOutputChannel,
  getTerminalController,
} from "$lib/panes/terminalRuntime";
import { killPty, spawnTask, type SessionExitPayload } from "$lib/tauri";
import { get, type Writable } from "svelte/store";

import {
  getInstance,
  paneInstances,
  updateInstance,
  type PaneInstance,
} from "./instances";

interface RerunCommandPaneOptions {
  now?: () => number;
  onElapsedUpdate?: () => void;
  paneStore?: Writable<Map<string, PaneInstance>>;
}

export async function rerunCommandPane(
  paneId: string,
  sessionId: string,
  options?: RerunCommandPaneOptions,
): Promise<void> {
  const instance = getInstance(paneId);
  if (!instance) return;

  const command = instance.command;
  const workingDir = instance.workingDir;
  if (!command || !workingDir) return;

  if (instance.commandStatus === "running") {
    await killPty(instance.ptyId).catch(() => {});
  }

  for (const unlisten of instance.unlisteners.splice(0)) {
    try {
      unlisten();
    } catch {
      // best-effort cleanup
    }
  }
  clearPaneOutputChannel(paneId);

  const now = options?.now ?? Date.now;
  const newPtyId = `${paneId}-${now()}`;

  if (instance.elapsedTimer != null) {
    clearInterval(instance.elapsedTimer);
  }

  const controller = getTerminalController(paneId);
  controller?.clear();
  controller?.reset();

  const onElapsedUpdate = options?.onElapsedUpdate;
  updateInstance(paneId, {
    ptyId: newPtyId,
    commandStatus: "running",
    commandExitCode: null,
    commandStartedAt: now(),
    elapsedTimer: setInterval(() => onElapsedUpdate?.(), 1000),
    unlisteners: [],
  });
  onElapsedUpdate?.();

  await connectPaneTerminal(paneId, (payload) => {
    handleCommandPaneExit(
      paneId,
      payload,
      options?.paneStore ?? paneInstances,
      onElapsedUpdate,
    );
  });

  await spawnTask(newPtyId, command, workingDir, sessionId, paneId, "command");
}

function handleCommandPaneExit(
  paneId: string,
  payload: SessionExitPayload,
  paneStore: Writable<Map<string, PaneInstance>>,
  onElapsedUpdate?: () => void,
): void {
  const exitCode = payload.code;
  const status = exitCode === 0 ? "success" : "error";
  updateInstance(paneId, {
    commandStatus: status,
    commandExitCode: exitCode,
  });

  const inst = get(paneStore).get(paneId);
  if (inst?.elapsedTimer != null) {
    clearInterval(inst.elapsedTimer);
    updateInstance(paneId, { elapsedTimer: null });
  }

  onElapsedUpdate?.();
}
