import { registry } from "./registry";
import { queries } from "$lib/queries";
import { addSession, setActiveSession, triggerRename, setSessionProject } from "$lib/stores/sessions";
import { settings, updateSetting } from "$lib/stores/settings";
import { navigatePane, movePaneInDirection, resizePane, toggleStack } from "$lib/panes/layout";
import { toggleFullscreen } from "$lib/panes/focus";
import { paneInstances, updateInstance } from "$lib/panes/instances";
import { splitPane, closePane, closeFocusedPane, initSession } from "$lib/panes/actions";
import { spawnShell, spawnTask, listDocs, writeToSession, createSession, openInEditor, listBranches, listProjects, setSessionProject as tauriSetSessionProject, createWatch } from "$lib/tauri";
import type { CreateWatchConfig, WatchKind } from "$lib/types";
import { ghAvailable } from "$lib/stores/watches";
import { closeSession } from "$lib/sessions/close";
import { reconnectSession } from "$lib/sessions/reconnect";
import { get } from "svelte/store";
import { log, logError } from "$lib/logging";
import { taskGroups } from "$lib/stores/tasks";
import { runTask } from "$lib/tasks/runner";

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
      const ptyId = crypto.randomUUID();
      log(`Split horizontal: pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath);
      } catch (e) {
        logError("Failed to spawn shell for horizontal split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "h", { type: "shell", ptyId });
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
      log(`Split vertical: pty=${ptyId} cwd=${session.worktreePath}`);
      try {
        await spawnShell(ptyId, session.worktreePath);
      } catch (e) {
        logError("Failed to spawn shell for vertical split", e);
        return;
      }
      const activeId = queries.activeSessionId();
      if (!activeId) return;
      const newPaneId = splitPane(activeId, "v", { type: "shell", ptyId });
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

  // -- Tasks --
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

  // -- Session actions --
  registry.register({
    id: "session.close",
    label: "Close Session",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    execute: async () => {
      const session = queries.activeSession();
      if (session) await closeSession(session);
    },
  });

  registry.register({
    id: "session.reconnect",
    label: "Reconnect Session",
    category: "Sessions",
    available: () => queries.activeSession()?.status === "disconnected",
    execute: async () => {
      const session = queries.activeSession();
      if (session) await reconnectSession(session);
    },
  });

  registry.register({
    id: "session.rename",
    label: "Rename Session",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    execute: () => triggerRename(),
  });

  registry.register({
    id: "session.set-project",
    label: "Set Project",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    inputPlaceholder: "Pick a project or type to create...",
    getItems: async () => {
      const projectList = await listProjects();
      const session = queries.activeSession();
      const items: { id: string; label: string; description?: string; action: () => void }[] = [];
      if (session?.projectId) {
        items.push({
          id: "__remove__",
          label: "Remove Project",
          description: "Unassign project from this session",
          action: async () => {
            setSessionProject(session.id, null);
            await tauriSetSessionProject(session.id, null);
          },
        });
      }
      for (const p of projectList) {
        items.push({
          id: p.id,
          label: p.name,
          description: session?.projectId === p.id ? "current" : undefined,
          action: async () => {
            if (!session) return;
            setSessionProject(session.id, p.id);
            await tauriSetSessionProject(session.id, p.id);
          },
        });
      }
      return items;
    },
    onInput: async (name: string) => {
      const session = queries.activeSession();
      if (!session) return;
      const { createProject } = await import("$lib/stores/projects");
      const project = await createProject(name);
      setSessionProject(session.id, project.id);
      await tauriSetSessionProject(session.id, project.id);
    },
  });

  registry.register({
    id: "session.open-in-editor",
    label: "Open in Editor",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    execute: async () => {
      const session = queries.activeSession();
      if (session) await openInEditor(session.worktreePath);
    },
  });

  // -- Worktree --
  registry.register({
    id: "session.new-worktree",
    label: "New Worktree",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    inputPlaceholder: "Branch name (pick existing or type new)...",
    getItems: async () => {
      const session = queries.activeSession();
      if (!session) return [];
      const branches = await listBranches(session.repoRoot).catch(() => [] as string[]);
      return branches.map((branch) => ({
        id: branch,
        label: branch,
        action: async () => {
          const repo = session.repoRoot;
          const name = repo.split("/").pop() + "-" + branch;
          const newSession = await createSession(repo, name, null, branch);
          addSession(newSession);
          initSession(newSession.id);
        },
      }));
    },
    onInput: async (branch: string) => {
      const session = queries.activeSession();
      if (!session) return;
      const repo = session.repoRoot;
      const name = repo.split("/").pop() + "-" + branch;
      const newSession = await createSession(repo, name, null, branch);
      addSession(newSession);
      initSession(newSession.id);
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
      await spawnTask(ptyId, command, session.worktreePath);
      const newPaneId = splitPane(activeId, "h", {
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

  // -- UI --
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
    id: "ui.toggle-task-panel",
    label: "Toggle Task Panel",
    category: "App",
    available: () => !!queries.activeSessionId(),
    execute: () => {
      const current = get(settings);
      updateSetting("taskPanelCollapsed", !current.taskPanelCollapsed);
    },
  });

  // -- Watches --
  registry.register({
    id: "watch.add",
    label: "Add Watch",
    category: "Watches",
    getItems: () => [
      {
        id: "github-action",
        label: "GitHub Action",
        description: "Watch a GitHub Actions workflow run",
        action: () => { registry.execute("watch.add-github"); },
      },
      {
        id: "http-health",
        label: "HTTP Health Check",
        description: "Monitor a URL for availability",
        action: () => { registry.execute("watch.add-http"); },
      },
      {
        id: "shell-command",
        label: "Shell Command",
        description: "Run a command and watch exit code",
        action: () => { registry.execute("watch.add-shell"); },
      },
    ],
  });

  registry.register({
    id: "watch.add-http",
    label: "Add HTTP Watch",
    category: "Watches",
    inputPlaceholder: "Enter URL to monitor (e.g. https://api.example.com/health)...",
    getItems: () => [],
    onInput: async (url: string) => {
      if (!url.startsWith("http")) return;
      let parsedUrl: URL;
      try {
        parsedUrl = new URL(url);
      } catch {
        return;
      }
      const session = queries.activeSession();
      const config: CreateWatchConfig = {
        name: `Health: ${parsedUrl.hostname}`,
        kind: { type: "httpHealth", url, expectedStatus: 200 },
        mode: { type: "recurring", intervalSecs: 60 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
      };
      await createWatch(config);
    },
  });

  registry.register({
    id: "watch.add-shell",
    label: "Add Shell Command Watch",
    category: "Watches",
    inputPlaceholder: "Enter command to watch (e.g. curl -s http://localhost:3000)...",
    getItems: () => [],
    onInput: async (command: string) => {
      if (!command.trim()) return;
      const session = queries.activeSession();
      const config: CreateWatchConfig = {
        name: `Cmd: ${command.slice(0, 40)}`,
        kind: {
          type: "shellCommand",
          command,
          workingDir: session?.worktreePath ?? null,
          successExitCode: 0,
        },
        mode: { type: "recurring", intervalSecs: 30 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
      };
      await createWatch(config);
    },
  });

  registry.register({
    id: "watch.add-github",
    label: "Add GitHub Action Watch",
    category: "Watches",
    inputPlaceholder: "Enter repo (owner/name) or GitHub Actions URL...",
    getItems: () => [],
    onInput: async (input: string) => {
      if (!input.trim()) return;
      if (!get(ghAvailable)) {
        // gh CLI not installed — can't create GitHub watches
        return;
      }
      const session = queries.activeSession();
      const urlMatch = input.match(
        /github\.com\/([^/]+\/[^/]+)\/actions\/runs\/(\d+)/
      );
      let kind: WatchKind;
      let name: string;
      if (urlMatch) {
        kind = {
          type: "githubAction",
          repo: urlMatch[1],
          runId: parseInt(urlMatch[2], 10),
          workflow: null,
          branch: null,
        };
        name = `GH: ${urlMatch[1]} #${urlMatch[2]}`;
      } else {
        kind = {
          type: "githubAction",
          repo: input.trim(),
          runId: null,
          workflow: null,
          branch: null,
        };
        name = `GH: ${input.trim()}`;
      }
      const config: CreateWatchConfig = {
        name,
        kind,
        mode: { type: "recurring", intervalSecs: 30 },
        scope: session
          ? { type: "session", sessionId: session.id }
          : { type: "global" },
      };
      await createWatch(config);
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
