import { registry } from "./registry";
import { queries } from "$lib/queries";
import { settings, updateSetting } from "$lib/stores/settings";
import { get } from "svelte/store";
import { loadKeymap, exitTree as keymapExitTree } from "$lib/keymap/store";
import { openSidebar } from "$lib/stores/ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { logError } from "$lib/logging";
import {
  setLastNotesScope,
  setNotesViewMode,
  toggleNotesViewMode,
  notesViewMode,
  type NotesScope,
} from "$lib/stores/notesUi";

const DOCS_URL = "https://github.com/phin-tech/roux#readme";
const ISSUES_URL = "https://github.com/phin-tech/roux/issues";

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
    id: "ui.notes-toggle-view-mode",
    label: "Toggle Notes View Mode",
    category: "App",
    available: () => !!queries.activeSession(),
    execute: () => {
      const session = queries.activeSession();
      if (!session) return;
      toggleNotesViewMode(session.id);
      openSidebar("notes");
    },
  });

  registry.register({
    id: "ui.notes-edit-mode",
    label: "Notes: Switch to Edit Mode",
    category: "App",
    available: () => {
      const session = queries.activeSession();
      return !!session && notesViewMode(session.id) === "read";
    },
    execute: () => {
      const session = queries.activeSession();
      if (!session) return;
      setNotesViewMode(session.id, "edit");
      openSidebar("notes");
    },
  });

  registry.register({
    id: "ui.notes-read-mode",
    label: "Notes: Switch to Read Mode",
    category: "App",
    available: () => {
      const session = queries.activeSession();
      return !!session && notesViewMode(session.id) === "edit";
    },
    execute: () => {
      const session = queries.activeSession();
      if (!session) return;
      setNotesViewMode(session.id, "read");
      openSidebar("notes");
    },
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

  registry.register({
    id: "help.open-docs",
    label: "Roux Documentation",
    category: "Help",
    execute: () => {
      openUrl(DOCS_URL).catch((e) => logError("help.open-docs failed", e));
    },
  });

  registry.register({
    id: "help.report-issue",
    label: "Report an Issue",
    category: "Help",
    execute: () => {
      openUrl(ISSUES_URL).catch((e) => logError("help.report-issue failed", e));
    },
  });
}
