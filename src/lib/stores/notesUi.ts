import { get, writable } from "svelte/store";

export type NotesScope = "global" | "project" | "repo" | "session";

interface NotesUiState {
  lastScopeBySession: Record<string, NotesScope>;
}

export const notesUiState = writable<NotesUiState>({ lastScopeBySession: {} });

/**
 * Returns the scope most recently selected in the panel for this session,
 * or the default `"session"` if none has been recorded yet.
 */
export function lastNotesScope(sessionId: string): NotesScope {
  const snapshot = get(notesUiState);
  return snapshot.lastScopeBySession[sessionId] ?? "session";
}

/**
 * Record the current panel scope for the given session so it's restored on
 * next open. Overwrites any prior selection for the same session.
 */
export function setLastNotesScope(sessionId: string, scope: NotesScope): void {
  notesUiState.update((s) => ({
    lastScopeBySession: { ...s.lastScopeBySession, [sessionId]: scope },
  }));
}
