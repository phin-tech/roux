import { writable, derived } from "svelte/store";
import type { Session } from "../types";

interface SessionState {
  sessions: Session[];
  activeSessionId: string | null;
}

export const sessionState = writable<SessionState>({
  sessions: [],
  activeSessionId: null,
});

export const activeSession = derived(sessionState, ($state) =>
  $state.sessions.find((s) => s.id === $state.activeSessionId) ?? null
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
  sessionState.update((state) => ({ ...state, activeSessionId: id }));
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

export function renameSession(id: string, newName: string) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, name: newName } : s
    ),
  }));
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
