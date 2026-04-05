import { spawnShell, writeToSession, onSessionExit } from "$lib/tauri";
import { addSplit } from "$lib/stores/panes";
import {
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  getEffectiveKeepOpen,
} from "$lib/stores/tasks";
import { closePane } from "$lib/panes/actions";
import type { TaskDefinition } from "$lib/types/tasks";

export async function runTask(
  sessionId: string,
  repoRoot: string,
  task: TaskDefinition
): Promise<void> {
  const ptyId = `task-${sessionId}-${task.id}-${Date.now()}`;
  const paneId = ptyId;

  await spawnShell(ptyId, repoRoot);
  addSplit(sessionId, "horizontal", {
    id: paneId,
    type: "shell",
    ptyId,
  });

  await writeToSession(ptyId, task.command + "\n");

  addTaskRun(sessionId, {
    taskId: task.id,
    paneId,
    ptyId,
    status: "running",
    exitCode: null,
    startedAt: Date.now(),
  });

  await onSessionExit(ptyId, (code) => {
    updateTaskRun(sessionId, ptyId, code);
    const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && code === 0)) {
      setTimeout(() => {
        closePane(sessionId, paneId);
        removeTaskRun(sessionId, ptyId);
      }, 2000);
    }
  });
}
