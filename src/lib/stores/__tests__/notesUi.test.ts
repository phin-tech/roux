import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import { notesUiState, lastNotesScope, setLastNotesScope, type NotesScope } from "../notesUi";

describe("notesUi store", () => {
  beforeEach(() => {
    notesUiState.set({ lastScopeBySession: {} });
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
});
