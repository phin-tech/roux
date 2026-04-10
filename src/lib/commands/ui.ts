import { registry } from "./registry";
import { queries } from "$lib/queries";
import { settings, updateSetting } from "$lib/stores/settings";
import { get } from "svelte/store";

export function registerUiCommands() {
  registry.register({
    id: "ui.group-by",
    label: "Group Sessions By",
    category: "App",
    getItems: () => {
      const current = get(settings).groupBy;
      return [
        {
          id: "repo",
          label: "Repository",
          description: current === "repo" ? "current" : undefined,
          action: () => updateSetting("groupBy", "repo"),
        },
        {
          id: "project",
          label: "Project",
          description: current === "project" ? "current" : undefined,
          action: () => updateSetting("groupBy", "project"),
        },
      ];
    },
  });

  registry.register({
    id: "ui.toggle-notes",
    label: "Toggle Notes",
    shortcut: "cmd+b",
    category: "App",
    available: () => !!queries.activeSession(),
  });

  registry.register({
    id: "ui.toggle-watches",
    label: "Toggle Watches",
    shortcut: "cmd+shift+w",
    category: "App",
    available: () => true,
  });

  registry.register({
    id: "ui.toggle-notifications",
    label: "Toggle Notifications",
    shortcut: "cmd+i",
    category: "App",
    available: () => true,
  });

  registry.register({
    id: "ui.toggle-task-panel",
    label: "Toggle Task Panel",
    category: "App",
    available: () => !!queries.activeSessionId(),
    execute: () => {
      const current = get(settings);
      updateSetting("taskPanelCollapsed", !current.taskPanelCollapsed);
    },
  });

  registry.register({
    id: "app.settings",
    label: "Settings",
    shortcut: "cmd+,",
    category: "App",
  });

  registry.register({
    id: "app.command-palette",
    label: "Command Palette",
    shortcut: "cmd+k",
    category: "App",
  });
}
