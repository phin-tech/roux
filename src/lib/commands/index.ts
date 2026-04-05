import { registry } from "./registry";
import { queries } from "$lib/queries";
import { setActiveSession } from "$lib/stores/sessions";
import { addSplit, removePane } from "$lib/stores/panes";
import { spawnShell, listDocs, writeToSession } from "$lib/tauri";

export function registerCommands() {
  // -- Panes --

  registry.register({
    id: "pane.split-horizontal",
    label: "Split Horizontal",
    shortcut: "cmd+d",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: async () => {
      const session = queries.activeSession();
      if (!session) return;
      const paneId = crypto.randomUUID();
      const ptyId = crypto.randomUUID();
      await spawnShell(ptyId, session.worktreePath);
      const activeId = queries.activeSessionId();
      if (activeId)
        addSplit(activeId, "horizontal", { id: paneId, type: "shell", ptyId });
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
      const paneId = crypto.randomUUID();
      const ptyId = crypto.randomUUID();
      await spawnShell(ptyId, session.worktreePath);
      const activeId = queries.activeSessionId();
      if (activeId)
        addSplit(activeId, "vertical", { id: paneId, type: "shell", ptyId });
    },
  });

  registry.register({
    id: "pane.close",
    label: "Close Pane",
    shortcut: "cmd+w",
    category: "Panes",
    available: () => queries.canClosePane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      const focused = queries.focusedPaneId();
      if (activeId && focused) removePane(activeId, focused);
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
            const paneId = crypto.randomUUID();
            addSplit(activeId, "horizontal", {
              id: paneId,
              type: "doc",
              ptyId: "",
              docPath: doc.path,
            });
          }
        },
      }));
    },
  });

  // -- Multi-step: Switch Session --
  registry.register({
    id: "session.switch",
    label: "Switch Session",
    category: "Sessions",
    getItems: () => {
      return queries.sessions().map((s) => ({
        id: s.id,
        label: s.name,
        description: `${s.branch} \u00b7 ${s.status}`,
        action: () => setActiveSession(s.id),
      }));
    },
  });

  // -- Multi-step: Approve Permission --
  registry.register({
    id: "session.approve",
    label: "Approve Permission",
    category: "Sessions",
    available: () => queries.hasAttentionSession(),
    getItems: () => {
      return queries
        .sessions()
        .filter((s) => s.status === "attention")
        .map((s) => ({
          id: s.id,
          label: s.name,
          description: s.permissionInfo
            ? `${s.permissionInfo.toolName}: ${JSON.stringify(s.permissionInfo.toolInput).slice(0, 60)}`
            : "Permission needed",
          substeps: () => [
            {
              id: "allow",
              label: "Allow",
              description: "Yes, this time",
              action: async () => {
                await writeToSession(s.id, "\r");
              },
            },
            {
              id: "always",
              label: "Always",
              description: "Allow during this session",
              action: async () => {
                await writeToSession(s.id, "\x1b[Z");
              },
            },
            {
              id: "deny",
              label: "Deny",
              description: "No",
              action: async () => {
                await writeToSession(s.id, "\x1b[B\x1b[B\r");
              },
            },
          ],
        }));
    },
  });

  // -- Simple commands (handled externally via callbacks) --
  registry.register({
    id: "session.new",
    label: "New Session",
    shortcut: "cmd+n",
    category: "Sessions",
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

export { registry } from "./registry";
