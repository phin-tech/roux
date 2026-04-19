import { get, writable } from "svelte/store";

export type NotesScope = "global" | "project" | "repo" | "session";
export type NotesViewMode = "read" | "edit";

interface NotesUiState {
  lastScopeBySession: Record<string, NotesScope>;
  viewModeBySession: Record<string, NotesViewMode>;
}

export const notesUiState = writable<NotesUiState>({
  lastScopeBySession: {},
  viewModeBySession: {},
});

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
    ...s,
    lastScopeBySession: { ...s.lastScopeBySession, [sessionId]: scope },
  }));
}

/**
 * Returns the view mode for the notes panel for this session,
 * or the default `"read"` if none has been recorded yet.
 */
export function notesViewMode(sessionId: string): NotesViewMode {
  const snapshot = get(notesUiState);
  return snapshot.viewModeBySession[sessionId] ?? "read";
}

/**
 * Set the view mode for the notes panel for this session.
 */
export function setNotesViewMode(sessionId: string, mode: NotesViewMode): void {
  notesUiState.update((s) => ({
    ...s,
    viewModeBySession: { ...s.viewModeBySession, [sessionId]: mode },
  }));
}

/**
 * Toggle between read and edit view modes for the notes panel.
 */
export function toggleNotesViewMode(sessionId: string): void {
  const current = notesViewMode(sessionId);
  setNotesViewMode(sessionId, current === "read" ? "edit" : "read");
}
