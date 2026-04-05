import { spawnShell, writeToSession, onSessionExit, onPtyOutput } from "$lib/tauri";
import { addSplit } from "$lib/stores/panes";
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
  const ptyId = `task-${sessionId}-${task.id}-${Date.now()}`;

  // Spawn PTY but don't create a pane
  await spawnShell(ptyId, repoRoot);
  await writeToSession(ptyId, task.command + "\n");

  addTaskRun(sessionId, {
    taskId: task.id,
    ptyId,
    paneId: null,
    status: "running",
    exitCode: null,
    outputLines: [],
    startedAt: Date.now(),
  });

  // Buffer PTY output into the task run store
  await onPtyOutput(ptyId, (b64data) => {
    const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
    const text = new TextDecoder().decode(bytes);
    appendTaskOutput(sessionId, ptyId, stripAnsi(text));
  });

  await onSessionExit(ptyId, (code) => {
    updateTaskRun(sessionId, ptyId, code);
    const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && code === 0)) {
      setTimeout(() => {
        removeTaskRun(sessionId, ptyId);
      }, 3000);
    }
  });
}

/** Promote a background task to a visible shell pane */
export function expandTask(sessionId: string, ptyId: string) {
  const paneId = ptyId;
  addSplit(sessionId, "horizontal", {
    id: paneId,
    type: "shell",
    ptyId,
  });
  setTaskPaneId(sessionId, ptyId, paneId);
}
