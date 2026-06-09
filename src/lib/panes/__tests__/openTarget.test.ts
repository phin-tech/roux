import { describe, expect, it } from "vitest";
import {
  resolveSessionOpenTarget,
  type SessionOpenTargetInput,
} from "../openTarget";
import type { Session } from "$lib/bindings";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    name: "Test Session",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "disconnected",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    primaryPtyId: null,
    ...overrides,
  };
}

function baseInput(overrides: Partial<SessionOpenTargetInput> = {}): SessionOpenTargetInput {
  return {
    sessionId: "s1",
    requestedPtyId: null,
    activeSession: null,
    localSession: null,
    livePtyIds: null,
    ...overrides,
  };
}

describe("resolveSessionOpenTarget", () => {
  // ── attach ────────────────────────────────────────────────────────────

  it("resolves attach when the requested run.ptyId is live", () => {
    const session = makeSession();
    const decision = resolveSessionOpenTarget(
      baseInput({
        requestedPtyId: "run-pty",
        activeSession: session,
        livePtyIds: new Set(["run-pty"]),
      }),
    );
    expect(decision).toEqual({
      kind: "attach",
      ptyId: "run-pty",
      session,
    });
  });

  it("resolves attach using session.primaryPtyId when no requested PTY", () => {
    const session = makeSession({ primaryPtyId: "primary-pty" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: new Set(["primary-pty"]),
      }),
    );
    expect(decision).toEqual({
      kind: "attach",
      ptyId: "primary-pty",
      session,
    });
  });

  it("resolves attach using sessionId as PTY when neither requestedPtyId nor primaryPtyId is set", () => {
    const session = makeSession({ id: "s1" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: new Set(["s1"]),
      }),
    );
    expect(decision).toEqual({
      kind: "attach",
      ptyId: "s1",
      session,
    });
  });

  it("prefers requestedPtyId over primaryPtyId when both are live", () => {
    const session = makeSession({ primaryPtyId: "primary-pty" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        requestedPtyId: "run-pty",
        activeSession: session,
        livePtyIds: new Set(["run-pty", "primary-pty"]),
      }),
    );
    expect(decision).toEqual({
      kind: "attach",
      ptyId: "run-pty",
      session,
    });
  });

  // ── continue ──────────────────────────────────────────────────────────

  it("resolves continue when the requested run.ptyId is stale (dead)", () => {
    const session = makeSession();
    const decision = resolveSessionOpenTarget(
      baseInput({
        requestedPtyId: "stale-pty",
        activeSession: session,
        livePtyIds: new Set(["other-pty"]),
      }),
    );
    expect(decision).toEqual({ kind: "continue", session });
  });

  it("resolves continue when no live PTYs exist for a disconnected session", () => {
    const session = makeSession({ status: "disconnected" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: new Set(),
      }),
    );
    expect(decision).toEqual({ kind: "continue", session });
  });

  it("resolves continue when livePtyIds is null (inventory read failed)", () => {
    const session = makeSession();
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: null,
      }),
    );
    expect(decision).toEqual({ kind: "continue", session });
  });

  it("resolves continue for an idle session with no live PTY", () => {
    const session = makeSession({ status: "idle" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: new Set(),
      }),
    );
    expect(decision).toEqual({ kind: "continue", session });
  });

  it("resolves continue even when primaryPtyId is set but dead", () => {
    const session = makeSession({ primaryPtyId: "dead-pty" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        activeSession: session,
        livePtyIds: new Set(["other-pty"]),
      }),
    );
    expect(decision).toEqual({ kind: "continue", session });
  });

  // ── restore-then-continue ─────────────────────────────────────────────

  it("resolves restore-then-continue when no active session but local store has one", () => {
    const local = makeSession({ id: "archived-plan" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        sessionId: "archived-plan",
        activeSession: null,
        localSession: local,
      }),
    );
    expect(decision).toEqual({
      kind: "restore-then-continue",
      sessionId: "archived-plan",
      localSession: local,
    });
  });

  it("resolves restore-then-continue with null localSession when neither store has it", () => {
    // This case is handled by the caller: gone vs restore-then-continue
    // depends on whether localSession exists. Without one, it's gone.
    const decision = resolveSessionOpenTarget(
      baseInput({
        sessionId: "unknown",
        activeSession: null,
        localSession: null,
      }),
    );
    expect(decision).toEqual({ kind: "gone", sessionId: "unknown" });
  });

  // ── gone ──────────────────────────────────────────────────────────────

  it("resolves gone when no session exists in active or local store", () => {
    const decision = resolveSessionOpenTarget(
      baseInput({
        sessionId: "missing",
        activeSession: null,
        localSession: null,
      }),
    );
    expect(decision).toEqual({ kind: "gone", sessionId: "missing" });
  });

  // ── edge cases ────────────────────────────────────────────────────────

  it("resolves attach when requestedPtyId equals sessionId and it is live", () => {
    const session = makeSession({ id: "plan-session" });
    const decision = resolveSessionOpenTarget(
      baseInput({
        sessionId: "plan-session",
        requestedPtyId: "plan-session",
        activeSession: session,
        livePtyIds: new Set(["plan-session"]),
      }),
    );
    expect(decision).toEqual({
      kind: "attach",
      ptyId: "plan-session",
      session,
    });
  });

  it("does not mutate input", () => {
    const session = makeSession();
    const liveIds = new Set(["s1"]);
    const input = baseInput({
      activeSession: session,
      livePtyIds: liveIds,
    });
    // Capture the state before calling the resolver.
    const inputKeys = { ...input, livePtyIds: new Set(input.livePtyIds) };
    resolveSessionOpenTarget(input);
    // Verify no new keys were added and the existing values are unchanged.
    expect(Object.keys(input)).toEqual(Object.keys(inputKeys));
    expect(input.sessionId).toBe(inputKeys.sessionId);
    expect(input.requestedPtyId).toBe(inputKeys.requestedPtyId);
    expect(input.activeSession).toBe(inputKeys.activeSession);
    expect(input.localSession).toBe(inputKeys.localSession);
    expect(input.livePtyIds).toBe(liveIds);
    // livePtyIds Set contents unchanged.
    expect([...input.livePtyIds!]).toEqual(["s1"]);
  });
});
