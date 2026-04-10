import { registry } from "./registry";
import { queries } from "$lib/queries";
import { paneInstances } from "$lib/panes/instances";
import { taskGroups } from "$lib/stores/tasks";
import { runTask } from "$lib/tasks/runner";
import { get } from "svelte/store";

export function registerTaskCommands() {
  registry.register({
    id: "task.run",
    label: "Run Task",
    category: "Tasks",
    available: () => get(taskGroups).length > 0,
    getItems: () => {
      const session = queries.activeSession();
      if (!session) return [];
      const groups = get(taskGroups);
      return groups.flatMap((group) =>
        group.tasks.map((task) => ({
          id: task.id,
          label: task.name,
          description: `${group.runner} — ${task.description || task.command}`,
          action: () => {
            const activeId = queries.activeSessionId();
            if (activeId) void runTask(activeId, session.worktreePath, task);
          },
        }))
      );
    },
  });

  registry.register({
    id: "task.rerun",
    label: "Rerun Command",
    category: "Tasks",
    available: () => {
      const instances = get(paneInstances);
      for (const inst of instances.values()) {
        if (inst.type === "command") return true;
      }
      return false;
    },
    getItems: () => {
      const items: { id: string; label: string; description: string; action: () => void }[] = [];
      const instances = get(paneInstances);
      for (const inst of instances.values()) {
        if (inst.type !== "command" || !inst.command) continue;
        const status = inst.commandStatus ?? "idle";
        items.push({
          id: inst.id,
          label: inst.command,
          description: status === "running" ? "Running" : status,
          action: () => {
            // Trigger rerun by dispatching a custom event or directly
            // For now, we just note this — PaneShell handles the actual rerun
          },
        });
      }
      return items;
    },
  });
}
