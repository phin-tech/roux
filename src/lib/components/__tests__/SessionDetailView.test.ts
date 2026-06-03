import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { get } from "svelte/store";
import SessionDetailView from "../SessionDetailView.svelte";
import { sessionState } from "$lib/stores/sessions";
import { projects } from "$lib/stores/projects";
import { sessionLayouts } from "$lib/panes/layout";
import { paneInstances, type PaneInstance } from "$lib/panes/instances";
import { closeMainView } from "$lib/stores/mainView";
import { continueSession } from "$lib/sessions/reconnect";
import { getDocument, listDocuments } from "$lib/stores/workItems";
import type { Attachment } from "$lib/types/workItems";
import type { Session } from "$lib/types";

vi.mock("$lib/stores/workItems", () => ({
  listDocuments: vi.fn().mockResolvedValue([]),
  getDocument: vi.fn(),
}));

vi.mock("$lib/stores/mainView", () => ({
  closeMainView: vi.fn(),
}));

vi.mock("$lib/sessions/reconnect", () => ({
  continueSession: vi.fn().mockResolvedValue({}),
}));

vi.mock("$lib/tauri", () => ({
  setSessionNameOverride: vi.fn().mockResolvedValue(undefined),
}));

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "session-1",
    name: "main",
    repoRoot: "/repo",
    worktreePath: "/repo/.worktrees/feature",
    branch: "feature/main-view",
    isWorktree: true,
    status: "idle",
    model: "claude-sonnet",
    cost: 1.25,
    createdAt: 1_700_000_000,
    projectId: "project-1",
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "pty-primary",
    archived: false,
    endedAt: null,
    pinnedPrUrl: "https://github.com/acme/repo/pull/123",
    ...overrides,
  };
}

function makePane(overrides: Partial<PaneInstance> = {}): PaneInstance {
  return {
    id: "pane-1",
    type: "shell",
    ptyId: "pty-primary",
    unlisteners: [],
    name: "main shell",
    spawnProfileRef: { kind: "registered", id: "claude" },
    sessionId: "session-1",
    ...overrides,
  };
}

function makeAttachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "doc-1",
    documentId: "session-1.doc-1",
    targetKind: "session",
    targetId: "session-1",
    title: "Implementation notes",
    contentKind: "text",
    mimeType: "text/markdown",
    sourcePath: null,
    byteLen: 42,
    sha256: "abc",
    createdAt: 1_700_000_100,
    updatedAt: 1_700_000_100,
    ...overrides,
  };
}

describe("SessionDetailView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sessionState.set({ sessions: [], activeSessionId: null });
    projects.set([]);
    sessionLayouts.set(new Map());
    paneInstances.set(new Map());
    vi.mocked(listDocuments).mockResolvedValue([]);
  });

  it("renders live session metadata and read-only pane summary", async () => {
    sessionState.set({
      sessions: [makeSession()],
      activeSessionId: "session-1",
    });
    projects.set([
      { id: "project-1", name: "Main Project", repoRoots: ["/repo"] },
    ]);
    sessionLayouts.set(
      new Map([["session-1", { kind: "leaf", paneId: "pane-1" }]]),
    );
    paneInstances.set(new Map([["pane-1", makePane()]]));

    render(SessionDetailView, { sessionId: "session-1" });

    expect(await screen.findByText("Main Project")).toBeTruthy();
    expect(screen.getByText("/repo/.worktrees/feature")).toBeTruthy();
    expect(screen.getAllByText("feature/main-view").length).toBeGreaterThan(0);
    expect(screen.getByText("main shell")).toBeTruthy();
    expect(screen.getByText("shell")).toBeTruthy();
    expect(screen.getByText("claude")).toBeTruthy();
    expect(listDocuments).toHaveBeenCalledWith("session", "session-1");
  });

  it("renders a Unix epoch createdAt value instead of treating it as missing", async () => {
    sessionState.set({
      sessions: [makeSession({ createdAt: 0 })],
      activeSessionId: "session-1",
    });

    render(SessionDetailView, { sessionId: "session-1" });

    expect(await screen.findByText(new Date(0).toLocaleString())).toBeTruthy();
  });

  it("opens an attached document inline", async () => {
    const attachment = makeAttachment();
    sessionState.set({
      sessions: [makeSession()],
      activeSessionId: "session-1",
    });
    vi.mocked(listDocuments).mockResolvedValue([attachment]);
    vi.mocked(getDocument).mockResolvedValue({
      attachment,
      content: "These are the attached notes.",
    });

    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      await screen.findByRole("button", { name: "Implementation notes" }),
    );

    expect(getDocument).toHaveBeenCalledWith("session-1.doc-1");
    expect(
      await screen.findByText("These are the attached notes."),
    ).toBeTruthy();
  });

  it("ignores stale attachment reads when a later selection resolves first", async () => {
    const first = makeAttachment({
      id: "doc-1",
      documentId: "session-1.doc-1",
      title: "First note",
    });
    const second = makeAttachment({
      id: "doc-2",
      documentId: "session-1.doc-2",
      title: "Second note",
    });
    let resolveFirst:
      | ((value: { attachment: Attachment; content: string }) => void)
      | undefined;
    let resolveSecond:
      | ((value: { attachment: Attachment; content: string }) => void)
      | undefined;

    sessionState.set({
      sessions: [makeSession()],
      activeSessionId: "session-1",
    });
    vi.mocked(listDocuments).mockResolvedValue([first, second]);
    vi.mocked(getDocument).mockImplementation((documentId) => {
      return new Promise((resolve) => {
        if (documentId === first.documentId) resolveFirst = resolve;
        if (documentId === second.documentId) resolveSecond = resolve;
      });
    });

    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      await screen.findByRole("button", { name: "First note" }),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Second note" }));

    resolveSecond?.({ attachment: second, content: "Second content" });
    await tick();
    expect(await screen.findByText("Second content")).toBeTruthy();

    resolveFirst?.({ attachment: first, content: "First stale content" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
    expect(screen.queryByText("First stale content")).toBeNull();
    expect(screen.getByText("Second content")).toBeTruthy();
  });

  it("ignores pending attachment reads after the session detail route changes", async () => {
    const firstAttachment = makeAttachment({
      id: "doc-1",
      documentId: "session-1.doc-1",
      targetId: "session-1",
      title: "First session note",
    });
    const secondAttachment = makeAttachment({
      id: "doc-2",
      documentId: "session-2.doc-2",
      targetId: "session-2",
      title: "Second session note",
    });
    let resolveFirst:
      | ((value: { attachment: Attachment; content: string }) => void)
      | undefined;

    sessionState.set({
      sessions: [
        makeSession(),
        makeSession({
          id: "session-2",
          name: "second",
          primaryPtyId: "pty-second",
        }),
      ],
      activeSessionId: "session-1",
    });
    vi.mocked(listDocuments).mockImplementation((_, targetId) => {
      if (targetId === "session-1") return Promise.resolve([firstAttachment]);
      return Promise.resolve([secondAttachment]);
    });
    vi.mocked(getDocument).mockImplementation((documentId) => {
      return new Promise((resolve) => {
        if (documentId === firstAttachment.documentId) resolveFirst = resolve;
      });
    });

    const view = render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      await screen.findByRole("button", { name: "First session note" }),
    );
    await view.rerender({ sessionId: "session-2" });
    expect(
      await screen.findByRole("button", { name: "Second session note" }),
    ).toBeTruthy();

    resolveFirst?.({
      attachment: firstAttachment,
      content: "First session stale content",
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();

    expect(screen.queryByText("First session stale content")).toBeNull();
    expect(screen.getByText("Select an attachment to read it.")).toBeTruthy();
  });

  it("keeps pending attachment reads alive across same-session metadata updates", async () => {
    const attachment = makeAttachment({
      title: "Live session note",
    });
    let resolveDocument:
      | ((value: { attachment: Attachment; content: string }) => void)
      | undefined;

    sessionState.set({
      sessions: [makeSession()],
      activeSessionId: "session-1",
    });
    vi.mocked(listDocuments).mockResolvedValue([attachment]);
    vi.mocked(getDocument).mockImplementation(() => {
      return new Promise((resolve) => {
        resolveDocument = resolve;
      });
    });

    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      await screen.findByRole("button", { name: "Live session note" }),
    );
    sessionState.update((state) => ({
      ...state,
      sessions: state.sessions.map((session) =>
        session.id === "session-1"
          ? { ...session, status: "generating" }
          : session,
      ),
    }));
    await tick();

    expect(listDocuments).toHaveBeenCalledTimes(1);

    resolveDocument?.({ attachment, content: "Still current content" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();

    expect(screen.getByText("Still current content")).toBeTruthy();
  });

  it("renames the session inline", async () => {
    sessionState.set({
      sessions: [makeSession()],
      activeSessionId: "session-1",
    });
    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Rename session" }),
    );
    const input = screen.getByLabelText("Session name");
    await fireEvent.input(input, { target: { value: "Renamed Session" } });
    await fireEvent.keyDown(input, { key: "Enter" });

    expect(screen.getByText("Renamed Session")).toBeTruthy();
  });

  it("focuses the session and closes the main view when opening the terminal", async () => {
    sessionState.set({ sessions: [makeSession()], activeSessionId: null });
    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Open terminal" }),
    );

    expect(get(sessionState).activeSessionId).toBe("session-1");
    expect(closeMainView).toHaveBeenCalled();
  });

  it("continues a disconnected session from the detail view", async () => {
    const session = makeSession({ status: "disconnected" });
    sessionState.set({ sessions: [session], activeSessionId: "session-1" });
    render(SessionDetailView, { sessionId: "session-1" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Continue session" }),
    );

    expect(continueSession).toHaveBeenCalledWith(session);
  });

  it("shows an empty state when the session no longer exists", () => {
    render(SessionDetailView, { sessionId: "missing-session" });

    expect(screen.getByText("Session no longer available")).toBeTruthy();
  });
});
