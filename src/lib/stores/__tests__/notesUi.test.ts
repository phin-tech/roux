import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  notesUiState,
  lastNotesScope,
  setLastNotesScope,
  notesViewMode,
  setNotesViewMode,
  toggleNotesViewMode,
  type NotesScope,
  type NotesViewMode,
} from "../notesUi";

describe("notesUi store", () => {
  beforeEach(() => {
    notesUiState.set({ lastScopeBySession: {}, viewModeBySession: {} });
  });

  it("defaults to 'session' when no scope has been recorded", () => {
    expect(lastNotesScope("unknown-id")).toBe<NotesScope>("session");
  });

  it("remembers the last-selected scope per session id", () => {
    setLastNotesScope("sess-a", "repo");
    setLastNotesScope("sess-b", "project");
    expect(lastNotesScope("sess-a")).toBe("repo");
    expect(lastNotesScope("sess-b")).toBe("project");
  });

  it("overwrites an earlier selection for the same session", () => {
    setLastNotesScope("sess-a", "repo");
    setLastNotesScope("sess-a", "global");
    expect(lastNotesScope("sess-a")).toBe("global");
  });

  it("exposes the store snapshot for debugging / persistence hooks", () => {
    setLastNotesScope("sess-a", "repo");
    const snapshot = get(notesUiState);
    expect(snapshot.lastScopeBySession["sess-a"]).toBe("repo");
  });

  describe("view mode", () => {
    it("defaults to 'read' when no view mode has been recorded", () => {
      expect(notesViewMode("unknown-id")).toBe<NotesViewMode>("read");
    });

    it("remembers the view mode per session id", () => {
      setNotesViewMode("sess-a", "edit");
      setNotesViewMode("sess-b", "read");
      expect(notesViewMode("sess-a")).toBe("edit");
      expect(notesViewMode("sess-b")).toBe("read");
    });

    it("overwrites an earlier view mode for the same session", () => {
      setNotesViewMode("sess-a", "edit");
      setNotesViewMode("sess-a", "read");
      expect(notesViewMode("sess-a")).toBe("read");
    });

    it("toggles between read and edit", () => {
      expect(notesViewMode("sess-a")).toBe("read");
      toggleNotesViewMode("sess-a");
      expect(notesViewMode("sess-a")).toBe("edit");
      toggleNotesViewMode("sess-a");
      expect(notesViewMode("sess-a")).toBe("read");
    });

    it("includes view mode in store snapshot", () => {
      setNotesViewMode("sess-a", "edit");
      const snapshot = get(notesUiState);
      expect(snapshot.viewModeBySession["sess-a"]).toBe("edit");
    });
  });
});
