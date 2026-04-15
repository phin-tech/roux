import { get } from "svelte/store";
import { registry } from "./registry";
import { queries } from "$lib/queries";
import { addSession, setActiveSession, triggerRename, setSessionProject, sessionState } from "$lib/stores/sessions";
import { projects } from "$lib/stores/projects";
import { settings } from "$lib/stores/settings";
import { getVisualSessionOrder } from "$lib/sessions/order";
import { initSessionWithProfile } from "$lib/panes/actions";
import { createSessionShell, openInEditor, listBranches, listProjects, setSessionProject as tauriSetSessionProject } from "$lib/tauri";
import type { SpawnProfileRef } from "$lib/panes/profiles";
import { closeSession } from "$lib/sessions/close";
import { reconnectSession } from "$lib/sessions/reconnect";

/**
 * Create a worktree-backed session that launches the built-in Claude profile.
 * Resolves the profile's nono config up-front so the primary shell is
 * nono-wrapped from the start, matching the layout/dialog paths.
 */
async function createWorktreeClaudeSession(repo: string, name: string, branch: string) {
  const { resolveProfileRef } = await import("$lib/panes/profiles");
  const { runProfileInPane } = await import("$lib/panes/profileRunner");
  const profileRef: SpawnProfileRef = { kind: "registered", id: "claude" };
  const profile = resolveProfileRef(profileRef);
  const nonoProfile = profile?.nonoProfile ?? undefined;
  const nonoAllowDirs = profile?.nonoAllowDirs ?? undefined;

  const newSession = await createSessionShell(
    repo, name, null, branch,
    nonoProfile, nonoAllowDirs,
  );
  addSession(newSession);
  const mainPaneId = initSessionWithProfile(newSession.id, profileRef, {
    nonoProfile,
    nonoAllowDirs,
  });
  const { initTerminal, attachPtyListeners } = await import("$lib/panes/terminals");
  initTerminal(mainPaneId);
  await attachPtyListeners(mainPaneId);
  if (profile) await runProfileInPane(newSession.id, profile);
}

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
          await createWorktreeClaudeSession(repo, name, branch);
        },
      }));
    },
    onInput: async (branch: string) => {
      const session = queries.activeSession();
      if (!session) return;
      const repo = session.repoRoot;
      const name = repo.split("/").pop() + "-" + branch;
      await createWorktreeClaudeSession(repo, name, branch);
    },
  });

  // -- Simple commands (handled externally via callbacks) --
  registry.register({
    id: "session.new",
    label: "New Session",
    category: "Sessions",
  });

  registry.register({
    id: "session.next",
    label: "Next Session",
    category: "Sessions",
    available: () => get(sessionState).sessions.length > 1,
    execute: () => cycleSession(1),
  });

  registry.register({
    id: "session.prev",
    label: "Previous Session",
    category: "Sessions",
    available: () => get(sessionState).sessions.length > 1,
    execute: () => cycleSession(-1),
  });

  for (let slot = 1; slot <= 10; slot++) {
    registry.register({
      id: `session.focus-index-${slot}`,
      label: `Focus Session ${slot}`,
      category: "Sessions",
      available: () => sessionInVisualOrder(slot) !== null,
      execute: () => {
        const target = sessionInVisualOrder(slot);
        if (target) setActiveSession(target);
      },
    });
  }
}

function sessionInVisualOrder(slot: number): string | null {
  const state = get(sessionState);
  const order = getVisualSessionOrder(
    state.sessions,
    get(projects),
    get(settings).groupBy ?? "repo",
  );
  return order[slot - 1]?.id ?? null;
}

function cycleSession(delta: number): void {
  const state = get(sessionState);
  if (state.sessions.length === 0) return;
  const order = getVisualSessionOrder(
    state.sessions,
    get(projects),
    get(settings).groupBy ?? "repo",
  );
  if (order.length === 0) return;
  const currentIndex = state.activeSessionId
    ? order.findIndex((s) => s.id === state.activeSessionId)
    : -1;
  const nextIndex =
    currentIndex === -1
      ? 0
      : (currentIndex + delta + order.length) % order.length;
  setActiveSession(order[nextIndex].id);
}
