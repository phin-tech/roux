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
    archivedSessionsState.set({
      sessions: [],
      loaded: false,
      worktreeExists: new Map(),
    });
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
    archivedSessionsState.set({
      sessions: [],
      loaded: false,
      worktreeExists: new Map(),
    });
    vi.restoreAllMocks();
  });

  it("renders dense archived rows with promoted restore and titled overflow actions", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("feature-gone"),
    ]);
    mockSessionWorktreeExists.mockImplementation(
      async (id) => id !== "feature-gone",
    );

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await screen.findByText("feature-gone");

    const restoreButtons = screen.getAllByText("Restore");
    expect(restoreButtons).toHaveLength(2);
    const enabledRestore = restoreButtons[0].closest(
      "button",
    ) as HTMLButtonElement;
    const disabledRestore = restoreButtons[1].closest(
      "button",
    ) as HTMLButtonElement;
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

    expect(
      screen.getByText("Notes").closest("button")?.getAttribute("title"),
    ).toBe("Open notes for this archived session");
    expect(
      screen.getByText("Reveal").closest("button")?.getAttribute("title"),
    ).toBe("Show this worktree folder in your file manager");
    expect(
      screen
        .getByText("Remove worktree")
        .closest("button")
        ?.getAttribute("title"),
    ).toBe("Delete the worktree folder but keep this history entry");
    expect(
      screen
        .getByText("Delete history")
        .closest("button")
        ?.getAttribute("title"),
    ).toBe("Permanently delete this archived session entry");
    expect(screen.queryByText("Clean")).toBeNull();
    expect(screen.queryByText("Delete")).toBeNull();
  });

  it("closes the overflow menu on outside pointerdown and Escape", async () => {
    mockListArchivedSessions.mockResolvedValue([makeArchived("feature-a")]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByTestId("archived-session-menu"));
    expect(screen.getByTestId("archived-session-menu-content")).not.toBeNull();

    await fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId("archived-session-menu-content")).toBeNull();

    await fireEvent.click(screen.getByTestId("archived-session-menu"));
    expect(screen.getByTestId("archived-session-menu-content")).not.toBeNull();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("archived-session-menu-content")).toBeNull();
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

  it("filters archived rows by name, branch, or worktree path", async () => {
    // sessionDisplayName prefers branch when isGitRepo, so the visible name
    // for these rows is the branch string.
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("login"),
      makeArchived("checkout-fix"),
      makeArchived("docs-pass", {
        branch: "docs-pass",
        worktreePath: "/repo/.worktrees/docs-special",
      }),
    ]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("login");
    expect(screen.getAllByTestId("archived-session-row")).toHaveLength(3);

    const filterInput = screen.getByTestId(
      "archived-filter-input",
    ) as HTMLInputElement;
    await fireEvent.input(filterInput, { target: { value: "checkout" } });
    expect(screen.getAllByTestId("archived-session-row")).toHaveLength(1);
    expect(screen.getByText("checkout-fix")).not.toBeNull();

    await fireEvent.input(filterInput, { target: { value: "docs-special" } });
    expect(screen.getAllByTestId("archived-session-row")).toHaveLength(1);
    expect(screen.getByText("docs-pass")).not.toBeNull();

    await fireEvent.input(filterInput, {
      target: { value: "no-match-anywhere" },
    });
    expect(screen.queryAllByTestId("archived-session-row")).toHaveLength(0);
    expect(
      screen.getByText(/No archived sessions match "no-match-anywhere"/),
    ).not.toBeNull();
  });

  it("bulk-deletes selected rows via the selection toolbar", async () => {
    const sessions = [
      makeArchived("feature-a"),
      makeArchived("feature-b"),
      makeArchived("feature-c"),
    ];
    mockListArchivedSessions.mockResolvedValue(sessions);
    mockSessionWorktreeExists.mockResolvedValue(true);
    vi.mocked(deleteSessionPermanently).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    const checkboxes = screen.getAllByTestId(
      "archived-row-checkbox",
    ) as HTMLInputElement[];
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[2]);

    expect(screen.getByText("2 selected")).not.toBeNull();
    await fireEvent.click(screen.getByText("Delete"));

    await waitFor(() => {
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledWith(
        "feature-a",
      );
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledWith(
        "feature-c",
      );
    });
    expect(vi.mocked(deleteSessionPermanently)).not.toHaveBeenCalledWith(
      "feature-b",
    );
  });

  it("select-all only checks the currently filtered rows", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("bugfix-b"),
      makeArchived("feature-c"),
    ]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    const filterInput = screen.getByTestId(
      "archived-filter-input",
    ) as HTMLInputElement;
    await fireEvent.input(filterInput, { target: { value: "feature" } });
    expect(screen.getAllByTestId("archived-session-row")).toHaveLength(2);

    await fireEvent.click(screen.getByTestId("archived-select-all"));
    expect(screen.getByText("2 selected")).not.toBeNull();

    await fireEvent.input(filterInput, { target: { value: "" } });
    // Selection is preserved across filter changes for rows still present.
    expect(screen.getByText("2 selected")).not.toBeNull();
  });

  it("preserves selections of rows hidden by the filter and acts on them in bulk", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("bugfix-b"),
      makeArchived("feature-c"),
    ]);
    mockSessionWorktreeExists.mockResolvedValue(true);
    vi.mocked(deleteSessionPermanently).mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");

    // Select feature-a while broad.
    const checkboxes = screen.getAllByTestId(
      "archived-row-checkbox",
    ) as HTMLInputElement[];
    await fireEvent.click(checkboxes[0]);
    expect(screen.getByText("1 selected")).not.toBeNull();

    // Narrow filter so feature-a is hidden — selection must persist.
    const filterInput = screen.getByTestId(
      "archived-filter-input",
    ) as HTMLInputElement;
    await fireEvent.input(filterInput, { target: { value: "bugfix" } });
    expect(screen.queryByText("feature-a")).toBeNull();
    expect(screen.getByText("1 selected")).not.toBeNull();

    // Bulk delete should still operate on the hidden selected row.
    await fireEvent.click(screen.getByText("Delete"));
    await waitFor(() => {
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledWith(
        "feature-a",
      );
    });
  });

  it("sets the visible select-all checkbox indeterminate state for partial selection", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("feature-b"),
    ]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    const rowCheckboxes = screen.getAllByTestId(
      "archived-row-checkbox",
    ) as HTMLInputElement[];
    const selectAll = screen.getByTestId(
      "archived-select-all",
    ) as HTMLInputElement;
    expect(selectAll.indeterminate).toBe(false);

    await fireEvent.click(rowCheckboxes[0]);

    await waitFor(() => {
      expect(selectAll.checked).toBe(false);
      expect(selectAll.indeterminate).toBe(true);
    });
  });

  it("offers Clear all and Remove all worktrees from the header overflow", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("feature-gone"),
    ]);
    mockSessionWorktreeExists.mockImplementation(
      async (id) => id !== "feature-gone",
    );
    vi.mocked(deleteSessionPermanently).mockResolvedValue(undefined);
    mockRemoveWorktree.mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByTestId("archived-header-menu"));
    expect(screen.getByTestId("archived-header-menu-content")).not.toBeNull();

    await fireEvent.click(screen.getByText("Remove all worktrees"));
    await waitFor(() => {
      expect(mockRemoveWorktree).toHaveBeenCalledTimes(1);
    });
    expect(mockRemoveWorktree).toHaveBeenCalledWith(
      "/repo",
      "/repo/.worktrees/feature-a",
    );

    await fireEvent.click(screen.getByTestId("archived-header-menu"));
    await fireEvent.click(screen.getByText("Clear all history"));
    await waitFor(() => {
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledWith(
        "feature-a",
      );
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledWith(
        "feature-gone",
      );
    });
  });

  it("guards bulk handlers against re-entry while a request is in flight", async () => {
    mockListArchivedSessions.mockResolvedValue([
      makeArchived("feature-a"),
      makeArchived("feature-b"),
    ]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    let resolveFirstDelete!: () => void;
    vi.mocked(deleteSessionPermanently)
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            resolveFirstDelete = resolve;
          }),
      )
      .mockResolvedValue(undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    const checkboxes = screen.getAllByTestId(
      "archived-row-checkbox",
    ) as HTMLInputElement[];
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);

    const deleteBtn = screen
      .getByText("Delete")
      .closest("button") as HTMLButtonElement;
    await fireEvent.click(deleteBtn);
    // While pending, the button should disable and a second click must not
    // fire another backend call.
    await waitFor(() => {
      expect(deleteBtn.disabled).toBe(true);
    });
    await fireEvent.click(deleteBtn);
    expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledTimes(1);

    resolveFirstDelete();
    await waitFor(() => {
      // After settling, the toolbar disappears (selection cleared) — there's
      // no Delete button to re-enable, but the backend was only hit once.
      expect(vi.mocked(deleteSessionPermanently)).toHaveBeenCalledTimes(2);
    });
  });

  it("closes the header overflow menu when the pane collapses", async () => {
    mockListArchivedSessions.mockResolvedValue([makeArchived("feature-a")]);
    mockSessionWorktreeExists.mockResolvedValue(true);

    const { rerender } = render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByTestId("archived-header-menu"));
    expect(screen.getByTestId("archived-header-menu-content")).not.toBeNull();

    await rerender({ collapsed: true });
    expect(screen.queryByTestId("archived-header-menu-content")).toBeNull();

    await rerender({ collapsed: false });
    // Menu should NOT pop back open just because we re-expanded.
    expect(screen.queryByTestId("archived-header-menu-content")).toBeNull();
  });

  it("disables bulk Restore when none of the selected rows have a worktree on disk", async () => {
    mockListArchivedSessions.mockResolvedValue([makeArchived("feature-gone")]);
    mockSessionWorktreeExists.mockResolvedValue(false);

    render(ArchivedSessionsList, { collapsed: false });

    await screen.findByText("feature-gone");
    const checkboxes = screen.getAllByTestId(
      "archived-row-checkbox",
    ) as HTMLInputElement[];
    await fireEvent.click(checkboxes[0]);

    const restoreBtn = screen
      .getAllByText("Restore")[0]
      .closest("button") as HTMLButtonElement;
    expect(restoreBtn.disabled).toBe(true);
  });

  it("notifies the parent after a single archived session is restored", async () => {
    const archived = makeArchived("feature-a");
    const restored: Session = {
      ...archived,
      archived: false,
      endedAt: null,
      status: "disconnected",
    };
    const onrestore = vi.fn().mockResolvedValue(undefined);
    mockListArchivedSessions.mockResolvedValue([archived]);
    mockSessionWorktreeExists.mockResolvedValue(true);
    mockListSessions.mockResolvedValueOnce([restored]);
    vi.mocked(restoreSession).mockResolvedValue(undefined);

    render(ArchivedSessionsList, { collapsed: false, onrestore });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByText("Restore"));

    await waitFor(() => {
      expect(vi.mocked(restoreSession)).toHaveBeenCalledWith("feature-a");
      expect(onrestore).toHaveBeenCalledWith("feature-a");
    });
    expect(screen.queryByText("feature-a")).toBeNull();
  });

  it("keeps the restored active row and shows an error when reconnect after restore fails", async () => {
    const archived = makeArchived("feature-a");
    const restored: Session = {
      ...archived,
      archived: false,
      endedAt: null,
      status: "disconnected",
    };
    const onrestore = vi.fn().mockRejectedValue(new Error("spawn failed"));
    mockListArchivedSessions.mockResolvedValue([archived]);
    mockSessionWorktreeExists.mockResolvedValue(true);
    mockListSessions.mockResolvedValueOnce([restored]);
    vi.mocked(restoreSession).mockResolvedValue(undefined);

    render(ArchivedSessionsList, { collapsed: false, onrestore });

    await screen.findByText("feature-a");
    await fireEvent.click(screen.getByText("Restore"));

    await waitFor(() => {
      expect(onrestore).toHaveBeenCalledWith("feature-a");
      expect(screen.getByTestId("archived-bulk-error").textContent).toContain(
        "Restored session, but failed to reconnect",
      );
    });
    expect(screen.queryByText("feature-a")).toBeNull();
  });
});
