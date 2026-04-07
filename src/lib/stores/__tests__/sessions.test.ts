import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  sessionState,
  activeSession,
  addSession,
  removeSession,
  setActiveSession,
  updateSessionStatus,
  updateSessionPermission,
  setSessionDisconnected,
  renameSession,
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
    permissionInfo: null,
    createdAt: Date.now(),
    projectId: null,
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

  it("updates permission info", () => {
    const s = makeSession();
    addSession(s);

    updateSessionPermission(s.id, {
      toolName: "Bash",
      toolInput: { command: "npm install" },
      message: "wants to run",
    });

    const session = get(sessionState).sessions[0];
    expect(session.permissionInfo).not.toBeNull();
    expect(session.permissionInfo!.toolName).toBe("Bash");
  });

  it("clears permission info", () => {
    const s = makeSession();
    addSession(s);
    updateSessionPermission(s.id, {
      toolName: "Bash",
      toolInput: {},
      message: "",
    });
    updateSessionPermission(s.id, null);

    expect(get(sessionState).sessions[0].permissionInfo).toBeNull();
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
});
