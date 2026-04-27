import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Session } from "$lib/types";
import { archivedSessionsState } from "$lib/stores/archivedSessions";
import ArchivedSessionsList from "../ArchivedSessionsList.svelte";
import {
  deleteSessionPermanently,
  listArchivedSessions,
  listSessions,
  openPathInFinder,
  removeWorktree,
  restoreSession,
  sessionWorktreeExists,
} from "$lib/tauri";

vi.mock("$lib/tauri", () => ({
  deleteSessionPermanently: vi.fn(),
  listArchivedSessions: vi.fn(),
  listSessions: vi.fn(),
  openPathInFinder: vi.fn(),
  removeWorktree: vi.fn(),
  restoreSession: vi.fn(),
  sessionWorktreeExists: vi.fn(),
}));

const mockListArchivedSessions = vi.mocked(listArchivedSessions);
const mockListSessions = vi.mocked(listSessions);
const mockSessionWorktreeExists = vi.mocked(sessionWorktreeExists);
const mockRemoveWorktree = vi.mocked(removeWorktree);

function makeArchived(id: string, overrides: Partial<Session> = {}): Session {
  return {
    id,
    name: id,
    repoRoot: "/repo",
    worktreePath: `/repo/.worktrees/${id}`,
    branch: id,
    isWorktree: true,
    status: "idle",
    model: null,
    cost: null,
    createdAt: 1,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: `pty-${id}`,
    archived: true,
    endedAt: 1_700_000_000,
    ...overrides,
  };
}

describe("ArchivedSessionsList", () => {
  beforeEach(() => {
    archivedSessionsState.set({ sessions: [], loaded: false, worktreeExists: new Map() });
    mockListArchivedSessions.mockReset();
    mockListSessions.mockReset().mockResolvedValue([]);
    mockSessionWorktreeExists.mockReset();
    mockRemoveWorktree.mockReset();
    vi.mocked(deleteSessionPermanently).mockReset();
    vi.mocked(openPathInFinder).mockReset();
    vi.mocked(restoreSession).mockReset();
    vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
  });

  afterEach(() => {
    archivedSessionsState.set({ sessions: [], loaded: false, worktreeExists: new Map() });
    vi.restoreAllMocks();
  });

  it("renders dense archived rows with promoted restore and titled overflow actions", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("feature-gone"),
    ]);
    mockSessionWorktreeExists.mockImplementation(async (id) => id !== "feature-gone");

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await screen.findByText("feature-gone");

    const restoreButtons = screen.getAllByText("Restore");
    expect(restoreButtons).toHaveLength(2);
    const enabledRestore = restoreButtons[0].closest("button") as HTMLButtonElement;
    const disabledRestore = restoreButtons[1].closest("button") as HTMLButtonElement;
    expect(enabledRestore.disabled).toBe(false);
    expect(enabledRestore.getAttribute("title")).toBe(
      "Move this session back to Active sessions",
    );
    expect(disabledRestore.disabled).toBe(true);
    expect(disabledRestore.getAttribute("title")).toBe(
      "Cannot restore because the worktree is no longer on disk",
    );
    expect(screen.getByText("on disk").getAttribute("title")).toBe(
      "Worktree still exists on disk",
    );
    expect(screen.getByText("gone").getAttribute("title")).toBe(
      "Worktree has been removed",
    );

    await fireEvent.click(screen.getAllByTestId("archived-session-menu")[0]);

    expect(screen.getByText("Notes").closest("button")?.getAttribute("title")).toBe(
      "Open notes for this archived session",
    );
    expect(
      screen.getByText("Reveal in Finder").closest("button")?.getAttribute("title"),
    ).toBe("Show this worktree folder in Finder");
    expect(
      screen.getByText("Remove worktree").closest("button")?.getAttribute("title"),
    ).toBe("Delete the worktree folder but keep this history entry");
    expect(
      screen.getByText("Delete history").closest("button")?.getAttribute("title"),
    ).toBe("Permanently delete this archived session entry");
    expect(screen.queryByText("Clean")).toBeNull();
    expect(screen.queryByText("Delete")).toBeNull();
  });

  it("shows remove-worktree failures inline with the affected archived row", async () => {
    const session = makeArchived("feature-a");
    mockListArchivedSessions.mockResolvedValue([session]);
    mockSessionWorktreeExists.mockResolvedValue(true);
    mockRemoveWorktree.mockRejectedValue(new Error("permission denied"));
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByTestId("archived-session-menu"));
    await fireEvent.click(screen.getByText("Remove worktree"));

    await waitFor(() => {
      expect(screen.getByText(/Failed to remove worktree:/)).not.toBeNull();
    });
    expect(mockRemoveWorktree).toHaveBeenCalledWith(
      session.repoRoot,
      session.worktreePath,
    );
  });
});
