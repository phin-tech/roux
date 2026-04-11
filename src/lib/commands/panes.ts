import { get } from "svelte/store";
import { registry } from "./registry";
import type { CommandItem } from "./registry";
import { queries } from "$lib/queries";
import { navigatePane, movePaneInDirection, resizePane, toggleStack } from "$lib/panes/layout";
import { toggleFullscreen } from "$lib/panes/focus";
import { updateInstance } from "$lib/panes/instances";
import { splitPane, closePane, closeFocusedPane } from "$lib/panes/actions";
import {
  profileList,
  type SpawnProfile,
  type SpawnProfileRef,
} from "$lib/panes/profiles";
import { runProfileInPane } from "$lib/panes/profileRunner";
import { spawnShell, spawnTask, listDocs } from "$lib/tauri";
import { log, logError } from "$lib/logging";

/**
 * Spawn a new shell pane seeded by a specific profile. Shared by both
 * "Split right with profile" and "Split down with profile" palette
 * commands. Plain shell profiles get the same path with no setup/startup
 * commands, so the helper handles every registry entry uniformly.
 */
async function spawnShellPaneWithProfile(
  direction: "h" | "v",
  profile: SpawnProfile,
): Promise<void> {
  const session = queries.activeSession();
  const activeId = queries.activeSessionId();
  if (!session || !activeId) return;

  const ptyId = crypto.randomUUID();
  const paneId = crypto.randomUUID();
  log(
    `Split ${direction} with profile "${profile.id}": pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`,
  );
  try {
    await spawnShell(ptyId, session.worktreePath, session.id, paneId);
  } catch (e) {
    logError(`Failed to spawn shell for profile "${profile.id}"`, e);
    return;
  }

  const spawnProfileRef: SpawnProfileRef =
    profile.source === "inline"
      ? { kind: "inline", profile }
      : { kind: "registered", id: profile.id };

  const newPaneId = splitPane(activeId, direction, {
    id: paneId,
    type: "shell",
    ptyId,
    spawnProfileRef,
  });
  if (!newPaneId) return;

  const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
  initTerminal(newPaneId);
  await attachPtyListeners(newPaneId, (payload) => {
    log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
    closePane(activeId, newPaneId);
  });

  // The attach is synchronous enough that the pending-output channel
  // catches any bytes emitted before we start typing.
  await runProfileInPane(ptyId, profile);
}

/**
 * Build a palette sub-picker item for each registered profile. Used by
 * both the horizontal and vertical "Split with profile" commands. The
 * `onPick` callback is the concrete action that runs after the user
 * chooses a profile in the drill-in.
 */
function profileSubItems(
  onPick: (profile: SpawnProfile) => void | Promise<void>,
): CommandItem[] {
  const items: CommandItem[] = [];
  for (const profile of get(profileList)) {
    const suffix =
      profile.source === "user"
        ? " (user)"
        : profile.provider
          ? ` · ${profile.provider}`
          : "";
    items.push({
      id: `profile:${profile.id}`,
      label: `${profile.name}${suffix}`,
      description: profile.startupCommand ?? undefined,
      action: () => onPick(profile),
    });
  }
  return items;
}

export function registerPaneCommands() {
  registry.register({
    id: "pane.split-horizontal",
    label: "Split Horizontal",
    shortcut: "cmd+d",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const session = queries.activeSession();
      if (!session) return;
      const ptyId = crypto.randomUUID();
      const paneId = crypto.randomUUID();
      log(`Split horizontal: pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath, session.id, paneId);
      } catch (e) {
        logError("Failed to spawn shell for horizontal split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "h", { id: paneId, type: "shell", ptyId });
      if (newPaneId) {
        const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
        initTerminal(newPaneId);
        await attachPtyListeners(newPaneId, (payload) => {
          log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
          closePane(activeId, newPaneId);
        });
      }
    },
  });

  registry.register({
    id: "pane.split-vertical",
    label: "Split Vertical",
    shortcut: "cmd+shift+d",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const session = queries.activeSession();
      if (!session) return;
      const ptyId = crypto.randomUUID();
      const paneId = crypto.randomUUID();
      log(`Split vertical: pane=${paneId} pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath, session.id, paneId);
      } catch (e) {
        logError("Failed to spawn shell for vertical split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "v", { id: paneId, type: "shell", ptyId });
      if (newPaneId) {
        const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
        initTerminal(newPaneId);
        await attachPtyListeners(newPaneId, (payload) => {
          log(`Shell pane ${newPaneId} exited (code=${payload.code})`);
          import("$lib/panes/actions").then(({ closePane: cp }) => cp(activeId, newPaneId));
        });
      }
    },
  });

  registry.register({
    id: "pane.split-horizontal-with-profile",
    label: "Split Right with Profile…",
    category: "Panes",
    available: () => queries.canSplitPane(),
    getItems: () => profileSubItems((p) => spawnShellPaneWithProfile("h", p)),
  });

  registry.register({
    id: "pane.split-vertical-with-profile",
    label: "Split Down with Profile…",
    category: "Panes",
    available: () => queries.canSplitPane(),
    getItems: () => profileSubItems((p) => spawnShellPaneWithProfile("v", p)),
  });

  registry.register({
    id: "pane.focus-left",
    label: "Focus Pane Left",
    shortcut: "alt+h",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "left");
    },
  });

  registry.register({
    id: "pane.focus-down",
    label: "Focus Pane Down",
    shortcut: "alt+j",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "down");
    },
  });

  registry.register({
    id: "pane.focus-up",
    label: "Focus Pane Up",
    shortcut: "alt+k",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "up");
    },
  });

  registry.register({
    id: "pane.focus-right",
    label: "Focus Pane Right",
    shortcut: "alt+l",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) navigatePane(activeId, "right");
    },
  });

  registry.register({
    id: "pane.move-left",
    label: "Move Pane Left",
    shortcut: "ctrl+shift+h",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "left");
    },
  });

  registry.register({
    id: "pane.move-down",
    label: "Move Pane Down",
    shortcut: "ctrl+shift+j",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "down");
    },
  });

  registry.register({
    id: "pane.move-up",
    label: "Move Pane Up",
    shortcut: "ctrl+shift+k",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "up");
    },
  });

  registry.register({
    id: "pane.move-right",
    label: "Move Pane Right",
    shortcut: "ctrl+shift+l",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) movePaneInDirection(activeId, "right");
    },
  });

  registry.register({
    id: "pane.toggle-fullscreen",
    label: "Toggle Fullscreen",
    shortcut: "cmd+shift+f",
    category: "Panes",
    available: () => !!queries.focusedPaneId(),
    execute: () => toggleFullscreen(),
  });

  registry.register({
    id: "pane.resize-left",
    label: "Resize Pane Left",
    shortcut: "ctrl+alt+h",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "left", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-down",
    label: "Resize Pane Down",
    shortcut: "ctrl+alt+j",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "down", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-up",
    label: "Resize Pane Up",
    shortcut: "ctrl+alt+k",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "up", 0.05);
    },
  });

  registry.register({
    id: "pane.resize-right",
    label: "Resize Pane Right",
    shortcut: "ctrl+alt+l",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) resizePane(activeId, "right", 0.05);
    },
  });

  registry.register({
    id: "pane.close",
    label: "Close Pane",
    shortcut: "cmd+w",
    category: "Panes",
    available: () => queries.canClosePane(),
    execute: async () => {
      const activeId = queries.activeSessionId();
      if (activeId) {
        await closeFocusedPane(activeId);
      }
    },
  });

  registry.register({
    id: "pane.toggle-stack",
    label: "Toggle Stack",
    shortcut: "cmd+shift+s",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) toggleStack(activeId);
    },
  });

  registry.register({
    id: "pane.rename",
    label: "Rename Pane",
    category: "Panes",
    available: () => !!queries.focusedPaneId(),
    inputPlaceholder: "Enter pane name...",
    getItems: () => [],
    onInput: (name: string) => {
      const paneId = queries.focusedPaneId();
      if (paneId) {
        updateInstance(paneId, { name: name.trim() || undefined });
      }
    },
  });

  // -- Multi-step: Open Document as Pane --
  registry.register({
    id: "pane.open-doc",
    label: "Open Document",
    shortcut: "cmd+shift+b",
    category: "Documents",
    getItems: async () => {
      const session = queries.activeSession();
      if (!session) return [];
      const docs = await listDocs(session.worktreePath);
      return docs.map((doc) => ({
        id: doc.path,
        label: doc.name,
        description: doc.relativePath,
        action: () => {
          const activeId = queries.activeSessionId();
          if (activeId) {
            splitPane(activeId, "h", {
              type: "markdown",
              ptyId: "",
              docPath: doc.path,
            });
          }
        },
      }));
    },
  });

  // -- Spawn command pane --
  registry.register({
    id: "pane.run-command",
    label: "Run Command",
    category: "Panes",
    available: () => queries.canSplitPane(),
    inputPlaceholder: "Enter command to run...",
    getItems: () => [],
    onInput: async (command: string) => {
      const session = queries.activeSession();
      const activeId = queries.activeSessionId();
      if (!session || !activeId) return;
      const paneId = `cmd-${crypto.randomUUID()}`;
      const ptyId = `${paneId}-${Date.now()}`;
      await spawnTask(ptyId, command, session.worktreePath, session.id, paneId);
      const newPaneId = splitPane(activeId, "h", {
        id: paneId,
        type: "command",
        ptyId,
        command,
        workingDir: session.worktreePath,
      });
      if (newPaneId) {
        const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
        initTerminal(newPaneId);
        updateInstance(newPaneId, {
          commandStatus: "running",
          commandStartedAt: Date.now(),
          elapsedTimer: setInterval(() => {}, 1000), // PaneShell handles display
        });
        await attachPtyListeners(newPaneId, (payload) => {
          const status = payload.code === 0 ? "success" : "error";
          updateInstance(newPaneId, {
            commandStatus: status as "success" | "error",
            commandExitCode: payload.code,
          });
        });
      }
    },
  });
}
