import { writable, readable, get, type Readable } from "svelte/store";
import type { Session } from "../types";

interface SessionState {
  sessions: Session[];
  activeSessionId: string | null;
}

export const sessionState = writable<SessionState>({
  sessions: [],
  activeSessionId: null,
});

function selectSessionState<T>(selector: (state: SessionState) => T): Readable<T> {
  let current = selector(get(sessionState));
  return readable(current, (set) =>
    sessionState.subscribe((state) => {
      const next = selector(state);
      if (Object.is(next, current)) return;
      current = next;
      set(next);
    }),
  );
}

export const sessionList = selectSessionState((state) => state.sessions);
export const activeSessionId = selectSessionState((state) => state.activeSessionId);
export const activeSession = selectSessionState(
  (state) => state.sessions.find((s) => s.id === state.activeSessionId) ?? null,
);

export function addSession(session: Session) {
  sessionState.update((state) => ({
    ...state,
    sessions: [...state.sessions, session],
    activeSessionId: session.id,
  }));
}

export function removeSession(id: string) {
  sessionState.update((state) => {
    const sessions = state.sessions.filter((s) => s.id !== id);
    const activeSessionId =
      state.activeSessionId === id
        ? sessions[sessions.length - 1]?.id ?? null
        : state.activeSessionId;
    return { sessions, activeSessionId };
  });
}

export function setActiveSession(id: string) {
  const state = get(sessionState);
  if (state.activeSessionId === id) return;
  sessionState.set({ ...state, activeSessionId: id });
}

export function updateSessionStatus(
  id: string,
  status: Session["status"],
  model?: string | null,
  cost?: number | null
) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id
        ? {
            ...s,
            status,
            model: model ?? s.model,
            cost: cost ?? s.cost,
          }
        : s
    ),
  }));
}

export function setSessionDisconnected(id: string) {
  updateSessionStatus(id, "disconnected");
}

/** Signal to trigger rename editing on the active session card */
export const renameSignal = writable(0);
export function triggerRename() {
  renameSignal.update((n) => n + 1);
}

/**
 * Compute the display name for a session with precedence:
 * 1. user-set rename override
 * 2. branch name (for git repos)
 * 3. worktree folder name (fallback)
 */
export function sessionDisplayName(s: Session): string {
  if (s.nameOverride && s.nameOverride.trim()) return s.nameOverride;
  if (s.isGitRepo && s.branch) return s.branch;
  const parts = s.worktreePath.split("/").filter(Boolean);
  return parts[parts.length - 1] || s.name;
}

export function renameSession(id: string, newName: string) {
  const trimmed = newName.trim() || null;
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, nameOverride: trimmed, name: trimmed ?? s.name } : s
    ),
  }));
  import("../tauri").then(({ setSessionNameOverride }) => {
    void setSessionNameOverride(id, trimmed).catch(() => {});
  });
}

export function clearSessionNameOverride(id: string) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, nameOverride: null } : s
    ),
  }));
  import("../tauri").then(({ setSessionNameOverride }) => {
    void setSessionNameOverride(id, null).catch(() => {});
  });
}

export function updateSessionGitStatus(id: string, isGitRepo: boolean) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, isGitRepo } : s
    ),
  }));
}

export function setSessionProject(id: string, projectId: string | null) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, projectId } : s
    ),
  }));
}

export function clearSessionsProject(projectId: string) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.projectId === projectId ? { ...s, projectId: null, blueprintId: null } : s
    ),
  }));
}
