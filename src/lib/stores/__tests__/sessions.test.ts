import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  sessionState,
  activeSession,
  activeSessionId,
  sessionList,
  addSession,
  removeSession,
  setActiveSession,
  updateSessionStatus,
  setSessionDisconnected,
  renameSession,
  updateSessionGitStatus,
  clearSessionsProject,
} from "../sessions";
import type { Session } from "$lib/types";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: crypto.randomUUID(),
    name: "test-session",
    repoRoot: "/tmp/repo",
    worktreePath: "/tmp/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: Date.now(),
    projectId: null,
    isGitRepo: true,
    ...overrides,
  };
}

describe("sessions store", () => {
  beforeEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("adds a session and sets it as active", () => {
    const s = makeSession({ name: "first" });
    addSession(s);

    const state = get(sessionState);
    expect(state.sessions).toHaveLength(1);
    expect(state.sessions[0].name).toBe("first");
    expect(state.activeSessionId).toBe(s.id);
  });

  it("sets the most recently added session as active", () => {
    const s1 = makeSession({ name: "first" });
    const s2 = makeSession({ name: "second" });
    addSession(s1);
    addSession(s2);

    expect(get(sessionState).activeSessionId).toBe(s2.id);
  });

  it("removes a session", () => {
    const s1 = makeSession();
    const s2 = makeSession();
    addSession(s1);
    addSession(s2);

    removeSession(s1.id);

    const state = get(sessionState);
    expect(state.sessions).toHaveLength(1);
    expect(state.sessions[0].id).toBe(s2.id);
  });

  it("switches active session when the active one is removed", () => {
    const s1 = makeSession();
    const s2 = makeSession();
    addSession(s1);
    addSession(s2);

    // s2 is active, remove it
    removeSession(s2.id);

    expect(get(sessionState).activeSessionId).toBe(s1.id);
  });

  it("sets activeSessionId to null when last session is removed", () => {
    const s = makeSession();
    addSession(s);
    removeSession(s.id);

    expect(get(sessionState).activeSessionId).toBeNull();
  });

  it("switches active session", () => {
    const s1 = makeSession();
    const s2 = makeSession();
    addSession(s1);
    addSession(s2);

    setActiveSession(s1.id);
    expect(get(sessionState).activeSessionId).toBe(s1.id);
  });

  it("does not notify when setting the already-active session", () => {
    const s1 = makeSession();
    addSession(s1);

    let calls = 0;
    const unsubscribe = sessionState.subscribe(() => {
      calls += 1;
    });
    calls = 0;

    setActiveSession(s1.id);

    expect(calls).toBe(0);
    unsubscribe();
  });

  it("does not notify session-list subscribers when only the active session changes", () => {
    const s1 = makeSession();
    const s2 = makeSession();
    addSession(s1);
    addSession(s2);

    let calls = 0;
    const unsubscribe = sessionList.subscribe(() => {
      calls += 1;
    });
    calls = 0;

    setActiveSession(s1.id);

    expect(calls).toBe(0);
    unsubscribe();
  });

  it("notifies active-session-id subscribers when the active session changes", () => {
    const s1 = makeSession();
    const s2 = makeSession();
    addSession(s1);
    addSession(s2);

    const values: Array<string | null> = [];
    const unsubscribe = activeSessionId.subscribe((id) => values.push(id));

    setActiveSession(s1.id);

    expect(values).toEqual([s2.id, s1.id]);
    unsubscribe();
  });

  it("does not notify active-session subscribers for non-active session updates", () => {
    const s1 = makeSession({ status: "idle" });
    const s2 = makeSession({ status: "idle" });
    addSession(s1);
    addSession(s2);
    setActiveSession(s1.id);

    let calls = 0;
    const unsubscribe = activeSession.subscribe(() => {
      calls += 1;
    });
    calls = 0;

    updateSessionStatus(s2.id, "generating");

    expect(calls).toBe(0);
    unsubscribe();
  });

  it("updates session status", () => {
    const s = makeSession({ status: "idle" });
    addSession(s);

    updateSessionStatus(s.id, "generating", "Opus 4.6", 0.42);

    const session = get(sessionState).sessions[0];
    expect(session.status).toBe("generating");
    expect(session.model).toBe("Opus 4.6");
    expect(session.cost).toBe(0.42);
  });

  it("preserves existing model/cost when not provided", () => {
    const s = makeSession({ model: "Opus 4.6", cost: 0.10 });
    addSession(s);

    updateSessionStatus(s.id, "thinking");

    const session = get(sessionState).sessions[0];
    expect(session.status).toBe("thinking");
    expect(session.model).toBe("Opus 4.6");
    expect(session.cost).toBe(0.10);
  });

  it("sets session as disconnected", () => {
    const s = makeSession({ status: "generating" });
    addSession(s);

    setSessionDisconnected(s.id);

    expect(get(sessionState).sessions[0].status).toBe("disconnected");
  });

  it("renames a session", () => {
    const s = makeSession({ name: "old-name" });
    addSession(s);

    renameSession(s.id, "new-name");

    expect(get(sessionState).sessions[0].name).toBe("new-name");
  });

  it("derives active session correctly", () => {
    const s1 = makeSession({ name: "first" });
    const s2 = makeSession({ name: "second" });
    addSession(s1);
    addSession(s2);

    expect(get(activeSession)?.name).toBe("second");

    setActiveSession(s1.id);
    expect(get(activeSession)?.name).toBe("first");
  });

  it("derives null active session when none exist", () => {
    expect(get(activeSession)).toBeNull();
  });

  it("updates session git status", () => {
    const s = makeSession({ isGitRepo: false });
    addSession(s);

    expect(get(sessionState).sessions[0].isGitRepo).toBe(false);

    updateSessionGitStatus(s.id, true);

    expect(get(sessionState).sessions[0].isGitRepo).toBe(true);
  });

  it("does not affect other sessions when updating git status", () => {
    const s1 = makeSession({ isGitRepo: false });
    const s2 = makeSession({ isGitRepo: false });
    addSession(s1);
    addSession(s2);

    updateSessionGitStatus(s1.id, true);

    expect(get(sessionState).sessions.find((s) => s.id === s1.id)?.isGitRepo).toBe(true);
    expect(get(sessionState).sessions.find((s) => s.id === s2.id)?.isGitRepo).toBe(false);
  });

  it("clears project and blueprint refs for sessions attached to a deleted project", () => {
    const s1 = makeSession({ projectId: "proj-1", blueprintId: "bp-1" });
    const s2 = makeSession({ projectId: "proj-2", blueprintId: "bp-2" });
    addSession(s1);
    addSession(s2);

    clearSessionsProject("proj-1");

    const state = get(sessionState);
    expect(state.sessions.find((s) => s.id === s1.id)).toMatchObject({
      projectId: null,
      blueprintId: null,
    });
    expect(state.sessions.find((s) => s.id === s2.id)).toMatchObject({
      projectId: "proj-2",
      blueprintId: "bp-2",
    });
  });
});
