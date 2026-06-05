import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NewSessionDialog from "../NewSessionDialog.svelte";
import { DEFAULT_SETTINGS } from "$lib/types";
import { settings } from "$lib/stores/settings";
import { resetProfileRegistry, setUserProfiles } from "$lib/panes/profiles";
import { checkIsGitRepo, listWorktrees, workItemStart } from "$lib/tauri";

if (typeof Element !== "undefined" && !Element.prototype.animate) {
  Element.prototype.animate = () =>
    ({
      cancel() {},
      play() {},
      pause() {},
      finished: Promise.resolve(),
      onfinish: null,
      currentTime: 0,
      playState: "finished",
      addEventListener() {},
      removeEventListener() {},
    }) as unknown as Animation;
}

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));
vi.mock("$lib/tauri", () => ({
  checkGhInstalled: vi.fn().mockResolvedValue(false),
  checkIsGitRepo: vi.fn().mockResolvedValue(false),
  cloneRepo: vi.fn(),
  createSessionShell: vi.fn(),
  detachPty: vi.fn().mockResolvedValue(undefined),
  documentAttach: vi.fn(),
  documentGet: vi.fn(),
  documentList: vi.fn(),
  fetchPrBranch: vi.fn(),
  gitInit: vi.fn(),
  killPty: vi.fn().mockResolvedValue(undefined),
  killSession: vi.fn(),
  listAllPtys: vi.fn().mockResolvedValue([]),
  listSessions: vi.fn().mockResolvedValue([]),
  listWorktrees: vi.fn().mockResolvedValue([]),
  lookupPr: vi.fn(),
  readFromSession: vi.fn(),
  spawnShell: vi.fn().mockResolvedValue(undefined),
  workItemCreate: vi.fn(),
  workItemDecisionsList: vi.fn().mockResolvedValue([]),
  workItemDecisionResolve: vi.fn(),
  workItemDelete: vi.fn(),
  workItemList: vi.fn().mockResolvedValue([]),
  workItemMove: vi.fn(),
  workItemPlan: vi.fn(),
  workItemReviewAccept: vi.fn(),
  workItemRunsList: vi.fn().mockResolvedValue([]),
  workItemRunStop: vi.fn(),
  workItemStart: vi.fn(),
  workItemUpdate: vi.fn(),
  writeToSession: vi.fn(),
}));
vi.mock("$lib/bindings", () => ({
  commands: {
    cmdDetectWorktrunk: vi.fn().mockResolvedValue({
      binaryPath: null,
      version: null,
      hasConfig: false,
    }),
  },
}));
vi.mock("$lib/panes/openSession", () => ({
  openSessionById: vi.fn().mockResolvedValue("opened"),
}));

const profiles = [
  {
    id: "claude",
    name: "Claude",
    setupCommand: null,
    startupCommand: "claude",
    startupBehavior: "autoRun",
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "claude",
    source: "user",
  },
  {
    id: "codex",
    name: "Codex",
    setupCommand: null,
    startupCommand: "codex",
    startupBehavior: "autoRun",
    env: null,
    cwdOverride: null,
    icon: null,
    provider: "codex",
    source: "user",
  },
] as const;

describe("NewSessionDialog profile defaults", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetProfileRegistry();
    setUserProfiles([...profiles]);
    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "claude" });
  });

  it("reseeds the selected profile from the current default agent when opened", async () => {
    const onclose = vi.fn();
    const { rerender } = render(NewSessionDialog, { visible: false, onclose });

    settings.set({ ...DEFAULT_SETTINGS, defaultAgentProfile: "codex" });
    await rerender({ visible: true, onclose });

    await waitFor(() => {
      expect(
        (
          document.getElementById(
            "new-session-profile",
          ) as HTMLInputElement | null
        )?.value,
      ).toBe("Codex (user)");
    });
  });

  it("lets the daemon choose a work-item worktree when no target is explicitly selected", async () => {
    vi.mocked(checkIsGitRepo).mockResolvedValue(true);
    vi.mocked(listWorktrees).mockResolvedValue([
      {
        path: "/repos/test-repo",
        branch: "main",
        isMain: true,
        worktrunk: null,
      },
      {
        path: "/repos/test-repo.worktrees/codex-stack-retry-observer",
        branch: "codex/stack-retry-observer",
        isMain: false,
        worktrunk: null,
      },
    ]);
    vi.mocked(workItemStart).mockResolvedValue({
      item: {
        id: "wi-1",
        projectId: null,
        parentId: null,
        title: "Add more tests",
        body: null,
        status: "doing",
        repoPath: "/repos/test-repo",
        agentProfile: "claude",
        baseBranch: "main",
        worktreePath: "/repos/test-repo.worktrees/roux-card-wi1-add-more-tests",
        branch: "roux/card-wi1-add-more-tests",
        fetchFirst: false,
        startError: null,
        sessionId: "sess-1",
        provider: null,
        externalId: null,
        externalUrl: null,
        sortOrder: 0,
        pinnedPrUrl: null,
        cost: null,
        createdAt: 0,
        updatedAt: 0,
      },
      run: {
        id: "run-1",
        workItemId: "wi-1",
        kind: "implementation",
        sessionId: "sess-1",
        ptyId: "sess-1",
        provider: "claude",
        profileId: "claude",
        status: "running",
        worktreePath: "/repos/test-repo.worktrees/roux-card-wi1-add-more-tests",
        branch: "roux/card-wi1-add-more-tests",
        cost: null,
        createdAt: 0,
        startedAt: 0,
        endedAt: null,
        updatedAt: 0,
      },
      session: {
        id: "sess-1",
        name: "Add more tests",
        repoRoot: "/repos/test-repo",
        worktreePath: "/repos/test-repo.worktrees/roux-card-wi1-add-more-tests",
        branch: "roux/card-wi1-add-more-tests",
        isWorktree: true,
        status: "idle",
        model: null,
        cost: null,
        createdAt: 0,
      },
    });
    settings.set({
      ...DEFAULT_SETTINGS,
      defaultAgentProfile: "claude",
      defaultProjectPath: "/repos/test-repo",
    });

    render(NewSessionDialog, {
      visible: true,
      onclose: vi.fn(),
      workItemStart: { itemId: "wi-1", title: "Add more tests" },
    });

    await waitFor(() => expect(screen.getByDisplayValue("Add more tests")).toBeTruthy());
    await fireEvent.click(screen.getByRole("button", { name: "Start Task" }));

    await waitFor(() =>
      expect(workItemStart).toHaveBeenCalledWith(
        "wi-1",
        expect.objectContaining({
          name: "Add more tests",
          worktreePath: null,
          branch: null,
        }),
      ),
    );
  });
});
