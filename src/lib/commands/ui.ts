import { registry } from "./registry";
import { queries } from "$lib/queries";
import { settings, updateSetting } from "$lib/stores/settings";
import { get } from "svelte/store";
import { loadKeymap, exitTree as keymapExitTree } from "$lib/keymap/store";
import { openSidebar } from "$lib/stores/ui";
import { setLastNotesScope, type NotesScope } from "$lib/stores/notesUi";

function showNotesScope(scope: NotesScope) {
  const session = queries.activeSession();
  if (!session) return;
  setLastNotesScope(session.id, scope);
  openSidebar("notes");
}

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
    category: "App",
    available: () => !!queries.activeSession(),
  });

  registry.register({
    id: "ui.notes-show-session",
    label: "Show Session Notes",
    category: "App",
    available: () => !!queries.activeSession(),
    execute: () => showNotesScope("session"),
  });

  registry.register({
    id: "ui.notes-show-repo",
    label: "Show Repo Notes",
    category: "App",
    available: () => !!queries.activeSession()?.repoRoot,
    execute: () => showNotesScope("repo"),
  });

  registry.register({
    id: "ui.notes-show-project",
    label: "Show Project Notes",
    category: "App",
    available: () => !!queries.activeSession()?.projectId,
    execute: () => showNotesScope("project"),
  });

  registry.register({
    id: "ui.notes-show-global",
    label: "Show Global Notes",
    category: "App",
    available: () => !!queries.activeSession(),
    execute: () => showNotesScope("global"),
  });

  registry.register({
    id: "ui.toggle-watches",
    label: "Toggle Watches",
    category: "App",
    available: () => true,
  });

  registry.register({
    id: "ui.toggle-notifications",
    label: "Toggle Notifications",
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
    id: "ui.toggle-sidebar",
    label: "Toggle Sidebar",
    category: "App",
    available: () => true,
    execute: () => {
      const current = get(settings);
      updateSetting("sidebarCollapsed", !current.sidebarCollapsed);
    },
  });

  registry.register({
    id: "app.settings",
    label: "Settings",
    category: "App",
  });

  registry.register({
    id: "app.command-palette",
    label: "Command Palette",
    category: "App",
  });

  registry.register({
    id: "app.leader-mode",
    label: "Leader Mode",
    category: "App",
  });

  registry.register({
    id: "app.check-updates",
    label: "Check for Updates",
    category: "App",
  });

  registry.register({
    id: "app.quit",
    label: "Quit Roux",
    category: "App",
  });

  registry.register({
    id: "keymap.reload",
    label: "Reload Keymap",
    category: "App",
    execute: () => {
      void loadKeymap();
    },
  });

  registry.register({
    id: "keymap.exit-tree",
    label: "Exit Keymap Tree",
    category: "App",
    available: () => false, // only fires from inside an active tree via a bind
    execute: () => keymapExitTree(),
  });
}
