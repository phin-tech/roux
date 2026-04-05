import { spawnTask, onSessionExit, onPtyOutput } from "$lib/tauri";
import { addSplit } from "$lib/stores/panes";
import {
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  appendTaskOutput,
  setTaskPaneId,
  getEffectiveKeepOpen,
} from "$lib/stores/tasks";
import { focusedPaneId } from "$lib/stores/panes";
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

  // Subscribe to output BEFORE spawning so we don't miss anything
  const outputReady = onPtyOutput(ptyId, (b64data) => {
    const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
    const text = new TextDecoder().decode(bytes);
    appendTaskOutput(sessionId, ptyId, stripAnsi(text));
  });

  const exitReady = onSessionExit(ptyId, (code) => {
    updateTaskRun(sessionId, ptyId, code);
    const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && code === 0)) {
      setTimeout(() => {
        removeTaskRun(sessionId, ptyId);
      }, 3000);
    }
  });

  // Wait for listeners to be registered before spawning
  await outputReady;
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

  // Spawn one-shot command — PTY exits when command finishes, with real exit code
  await spawnTask(ptyId, task.command, repoRoot);

  // If keepOpen is "always", show in a command pane with rerun support
  if (spawnInPane) {
    focusedPaneId.set(`${sessionId}-main`);
    addSplit(sessionId, "horizontal", {
      id: ptyId,
      type: "command",
      ptyId,
      command: task.command,
      workingDir: repoRoot,
    });
  }
}

/** Promote a background task to a visible shell pane */
export function expandTask(sessionId: string, ptyId: string) {
  const paneId = ptyId;
  // Ensure focus is on the session's main pane so addSplit can find a target
  focusedPaneId.set(`${sessionId}-main`);
  addSplit(sessionId, "horizontal", {
    id: paneId,
    type: "shell",
    ptyId,
  });
  setTaskPaneId(sessionId, ptyId, paneId);
}
