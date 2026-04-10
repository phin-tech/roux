import { registry } from "./registry";
import { queries } from "$lib/queries";
import { addSession, setActiveSession, triggerRename, setSessionProject } from "$lib/stores/sessions";
import { initSession } from "$lib/panes/actions";
import { createSession, openInEditor, listBranches, listProjects, setSessionProject as tauriSetSessionProject } from "$lib/tauri";
import { closeSession } from "$lib/sessions/close";
import { reconnectSession } from "$lib/sessions/reconnect";

export function registerSessionCommands() {
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

  // -- Simple commands (handled externally via callbacks) --
  registry.register({
    id: "session.new",
    label: "New Session",
    shortcut: "cmd+n",
    category: "Sessions",
  });
}
