import { attachPtyOutput, createPtyOutputChannel, spawnTask, onSessionExit, type SessionExitPayload } from "$lib/tauri";
import { splitPane } from "$lib/panes/actions";
import { updateInstance, getInstance } from "$lib/panes/instances";
import { focusedPaneId } from "$lib/panes/focus";
import { log } from "$lib/logging";
import {
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  appendTaskOutput,
  setTaskPaneId,
  getEffectiveKeepOpen,
} from "$lib/stores/tasks";
import type { TaskDefinition } from "$lib/types/tasks";

/** Simple ANSI escape code stripper for inline display */
function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "").replace(/\x1b\][^\x07]*\x07/g, "");
}

export async function runTask(
  sessionId: string,
  repoRoot: string,
  task: TaskDefinition
): Promise<void> {
  const safeId = task.id.replace(/:/g, "-");
  const ptyId = `task-${sessionId}-${safeId}-${Date.now()}`;

  const exitReady = onSessionExit(ptyId, (payload: SessionExitPayload) => {
    updateTaskRun(sessionId, ptyId, payload.code);
    const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && payload.code === 0)) {
      setTimeout(() => {
        removeTaskRun(sessionId, ptyId);
      }, 3000);
    }
  });

  await exitReady;

  const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
  const spawnInPane = keepOpen === "always";

  addTaskRun(sessionId, {
    taskId: task.id,
    ptyId,
    paneId: spawnInPane ? ptyId : null,
    status: "running",
    exitCode: null,
    outputLines: [],
    startedAt: Date.now(),
  });

  // Spawn one-shot command — output buffers in Rust backlog until channel attaches
  log(`runTask: spawning ptyId=${ptyId} cmd="${task.command}" keepOpen=${keepOpen} spawnInPane=${spawnInPane}`);
  await spawnTask(ptyId, task.command, repoRoot);

  // If keepOpen is "always", show in a command pane with full terminal
  if (spawnInPane) {
    focusedPaneId.set(`${sessionId}-main`);
    const newPaneId = splitPane(sessionId, "h", {
      type: "command",
      ptyId,
      command: task.command,
      workingDir: repoRoot,
    });

    if (newPaneId) {
      const { initTerminal } = await import("$lib/panes/terminals");
      initTerminal(newPaneId);
      updateInstance(newPaneId, {
        commandStatus: "running",
        commandStartedAt: Date.now(),
      });

      // Create output channel that feeds both the inline task panel and the terminal.
      // We attach it ourselves (not via attachPtyListeners) to avoid a double-attach
      // that invalidates the Tauri callback ID.
      const outputChannel = createPtyOutputChannel((bytes) => {
        const text = new TextDecoder().decode(bytes);
        appendTaskOutput(sessionId, ptyId, stripAnsi(text));
        getInstance(newPaneId)?.terminal?.write(bytes);
      });
      updateInstance(newPaneId, { outputChannel });
      await attachPtyOutput(ptyId, outputChannel);

      // Set up exit listener for the pane's command status display
      const unlisten = await onSessionExit(ptyId, (payload) => {
        const status = payload.code === 0 ? "success" : "error";
        updateInstance(newPaneId, {
          commandStatus: status as "success" | "error",
          commandExitCode: payload.code,
        });
      });
      const inst = getInstance(newPaneId);
      if (inst) inst.unlisteners.push(unlisten);
    }
  } else {
    // Background mode: inline output only
    const outputChannel = createPtyOutputChannel((bytes) => {
      const text = new TextDecoder().decode(bytes);
      appendTaskOutput(sessionId, ptyId, stripAnsi(text));
    });
    await attachPtyOutput(ptyId, outputChannel);
  }
}

/** Promote a background task to a visible shell pane */
export function expandTask(sessionId: string, ptyId: string) {
  // Ensure focus is on the session's main pane so splitPane can find a target
  focusedPaneId.set(`${sessionId}-main`);
  const newPaneId = splitPane(sessionId, "h", {
    type: "shell",
    ptyId,
  });
  if (newPaneId) {
    setTaskPaneId(sessionId, ptyId, newPaneId);
  }
}
