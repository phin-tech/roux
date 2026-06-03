import { get } from "svelte/store";
import { registry } from "./registry";
import { queries } from "$lib/queries";
import {
  addSession,
  setActiveSession,
  triggerRename,
  setSessionProject,
  sessionState,
} from "$lib/stores/sessions";
import { projects } from "$lib/stores/projects";
import { settings } from "$lib/stores/settings";
import { defaultAgentProfileId } from "$lib/panes/defaultAgent";
import { getVisualSessionOrder } from "$lib/sessions/order";
import { initSessionWithProfile } from "$lib/panes/actions";
import {
  createSessionShell,
  openInEditor,
  listProjects,
  setSessionProject as tauriSetSessionProject,
} from "$lib/tauri";
import type { SpawnProfileRef } from "$lib/panes/profiles";
import { closeSession } from "$lib/sessions/close";
import { reconnectSession } from "$lib/sessions/reconnect";

/**
 * Create a worktree-backed session that launches the built-in Claude profile.
 *
 * `base` is the git starting point for the new branch ("main", "origin/main",
 * or the session's current branch). When `fetchFirst` is true, the backend
 * runs `git fetch origin` before branching — used for `origin/*` bases.
 */
async function createWorktreeClaudeSession(
  repo: string,
  name: string,
  branch: string,
  base: string | null,
  fetchFirst: boolean,
) {
  const { resolveProfileRef } = await import("$lib/panes/profiles");
  const { runProfileInPane } = await import("$lib/panes/profileRunner");
  const profileId = defaultAgentProfileId();
  const profileRef: SpawnProfileRef = { kind: "registered", id: profileId };
  const profile = resolveProfileRef(profileRef);

  const newSession = await createSessionShell(repo, name, null, branch, {
    profile: profileId,
    base,
    fetchFirst,
  });
  addSession(newSession);
  const mainPaneId = initSessionWithProfile(newSession.id, profileRef);
  const { connectPaneTerminal } = await import("$lib/panes/terminals");
  await connectPaneTerminal(mainPaneId);
  if (profile) await runProfileInPane(newSession.id, profile);
}

function registerWorktreeChild(opts: {
  id: string;
  label: string;
  resolveBase: () => string | null;
  fetchFirst: boolean;
}) {
  registry.register({
    id: opts.id,
    label: opts.label,
    category: "Sessions",
    available: () => !!queries.activeSession(),
    inputPlaceholder: "New branch name (e.g. feature/my-thing)...",
    getItems: () => [],
    onInput: async (branch: string) => {
      const session = queries.activeSession();
      if (!session || !branch.trim()) return;
      const repo = session.repoRoot;
      const name = repo.split("/").pop() + "-" + branch;
      const base = opts.resolveBase();
      await createWorktreeClaudeSession(
        repo,
        name,
        branch.trim(),
        base,
        opts.fetchFirst,
      );
    },
  });
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
      const items: {
        id: string;
        label: string;
        description?: string;
        action: () => void;
      }[] = [];
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
  // Parent: drill into base picker. Matches the pattern used by `watch.add`
  // in commands/watches.ts.
  registry.register({
    id: "session.new-worktree",
    label: "New Worktree",
    category: "Sessions",
    available: () => !!queries.activeSession(),
    getItems: () => [
      {
        id: "current",
        label: "From current branch",
        description: "Branch from this session's current branch",
        drillCommand: "session.new-worktree-from-current",
      },
      {
        id: "main",
        label: "From main",
        description: "Branch from local main",
        drillCommand: "session.new-worktree-from-main",
      },
      {
        id: "origin-main",
        label: "From origin/main",
        description: "Fetches origin, then branches from origin/main",
        drillCommand: "session.new-worktree-from-origin-main",
      },
    ],
  });

  registerWorktreeChild({
    id: "session.new-worktree-from-current",
    label: "New Worktree (from current branch)",
    // Detached-HEAD sessions report `branch` as "" — normalize to null so
    // backend falls back to HEAD instead of failing start-point validation.
    resolveBase: () => {
      const branch = queries.activeSession()?.branch?.trim();
      return branch ? branch : null;
    },
    fetchFirst: false,
  });
  registerWorktreeChild({
    id: "session.new-worktree-from-main",
    label: "New Worktree (from main)",
    resolveBase: () => "main",
    fetchFirst: false,
  });
  registerWorktreeChild({
    id: "session.new-worktree-from-origin-main",
    label: "New Worktree (from origin/main)",
    resolveBase: () => "origin/main",
    fetchFirst: true,
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
