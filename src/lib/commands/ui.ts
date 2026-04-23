import { registry } from "./registry";
import { queries } from "$lib/queries";
import { settings, updateSetting } from "$lib/stores/settings";
import { get } from "svelte/store";
import { loadKeymap, exitTree as keymapExitTree } from "$lib/keymap/store";
import {
  activeSidebar,
  openSidebar,
  pinnedSidebar,
  pinSidebar,
  PINNABLE_SIDEBARS,
  toggleSidebar,
  unpinSidebar,
} from "$lib/stores/ui";
import {
  setRailSide,
  toggleRailSide,
  toggleSidebarHidden,
} from "$lib/stores/sidebarLayout";
import { openUrl } from "@tauri-apps/plugin-opener";
import { logError } from "$lib/logging";
import {
  THEME_DEFINITIONS,
  getAllTerminalThemeDefinitions,
  MATCH_GUI_TERMINAL_THEME_ID,
} from "$lib/themes";
import {
  userTerminalThemes,
  loadUserTerminalThemes,
} from "$lib/stores/userTerminalThemes";
import {
  setLastNotesScope,
  setNotesViewMode,
  toggleNotesViewMode,
  notesViewMode,
  type NotesScope,
} from "$lib/stores/notesUi";

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
    id: "appearance.theme",
    label: "Switch GUI Theme",
    category: "Appearance",
    getItems: () => {
      const current = get(settings).theme;
      return THEME_DEFINITIONS.map((t) => ({
        id: t.id,
        label: t.label,
        description: t.id === current ? "current" : t.description,
        action: () => updateSetting("theme", t.id),
      }));
    },
  });

  registry.register({
    id: "appearance.terminal-theme",
    label: "Switch Terminal Theme",
    category: "Appearance",
    getItems: () => {
      const current = get(settings).terminalTheme ?? MATCH_GUI_TERMINAL_THEME_ID;
      const defs = getAllTerminalThemeDefinitions(get(userTerminalThemes));
      return defs.map((t) => {
        const tag =
          t.category === "auto"
            ? "auto"
            : t.category === "matching"
              ? "app palette"
              : t.category === "editor"
                ? "editor"
                : "user";
        const description = t.id === current ? "current" : tag;
        return {
          id: t.id,
          label: t.label,
          description,
          action: () => updateSetting("terminalTheme", t.id),
        };
      });
    },
  });

  registry.register({
    id: "appearance.reload-terminal-themes",
    label: "Reload Terminal Themes",
    category: "Appearance",
    execute: () => {
      void loadUserTerminalThemes();
    },
  });

  registry.register({
    id: "ui.toggle-notes",
    label: "Toggle Notes",
    category: "App",
    available: () => !!queries.activeSession(),
    execute: () => toggleSidebar("notes"),
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
    execute: () => toggleSidebar("watches"),
  });

  registry.register({
    id: "ui.toggle-notifications",
    label: "Toggle Notifications",
    category: "App",
    available: () => true,
    execute: () => toggleSidebar("notifications"),
  });

  registry.register({
    id: "ui.toggle-sessions",
    label: "Toggle Sessions List",
    category: "App",
    available: () => true,
    execute: () => toggleSidebar("sessions"),
  });

  registry.register({
    id: "ui.toggle-task-panel",
    label: "Toggle Tasks",
    category: "App",
    available: () => !!queries.activeSessionId(),
    execute: () => toggleSidebar("tasks"),
  });

  registry.register({
    id: "ui.toggle-tasks",
    label: "Toggle Tasks Panel",
    category: "App",
    available: () => !!queries.activeSessionId(),
    execute: () => toggleSidebar("tasks"),
  });

  registry.register({
    id: "ui.toggle-docs",
    label: "Toggle Docs",
    category: "App",
    available: () => true,
    execute: () => toggleSidebar("docs"),
  });

  registry.register({
    id: "ui.pin-sidebar",
    label: "Pin Current Sidebar",
    category: "App",
    available: () => {
      const id = get(activeSidebar);
      return id !== null && PINNABLE_SIDEBARS.has(id);
    },
    execute: () => {
      const id = get(activeSidebar);
      if (id && PINNABLE_SIDEBARS.has(id)) pinSidebar(id);
    },
  });

  registry.register({
    id: "ui.unpin-sidebar",
    label: "Unpin Sidebar",
    category: "App",
    available: () => get(pinnedSidebar) !== null,
    execute: () => unpinSidebar(),
  });

  registry.register({
    id: "ui.rail-side-left",
    label: "Sidebar: Move to Left",
    category: "App",
    available: () => true,
    execute: () => setRailSide("left"),
  });

  registry.register({
    id: "ui.rail-side-right",
    label: "Sidebar: Move to Right",
    category: "App",
    available: () => true,
    execute: () => setRailSide("right"),
  });

  registry.register({
    id: "ui.toggle-rail-side",
    label: "Toggle Sidebar Left/Right",
    category: "App",
    available: () => true,
    execute: () => toggleRailSide(),
  });

  registry.register({
    id: "ui.toggle-pin-sidebar",
    label: "Pin / Unpin Current Sidebar",
    category: "App",
    available: () => {
      const pinned = get(pinnedSidebar);
      const active = get(activeSidebar);
      if (pinned) return true;
      return active !== null && PINNABLE_SIDEBARS.has(active);
    },
    execute: () => {
      const pinned = get(pinnedSidebar);
      if (pinned) {
        unpinSidebar();
        return;
      }
      const active = get(activeSidebar);
      if (active && PINNABLE_SIDEBARS.has(active)) pinSidebar(active);
    },
  });

  registry.register({
    id: "ui.toggle-sidebar",
    label: "Toggle Sidebar (Rail + Dock)",
    category: "App",
    available: () => true,
    execute: () => toggleSidebarHidden(),
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
    execute: () => openSidebar("docs"),
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
