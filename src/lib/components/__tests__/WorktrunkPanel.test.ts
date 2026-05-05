import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { tick } from "svelte";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { get } from "svelte/store";
import type { WorktrunkDiagnostics } from "$lib/bindings";
import type { Worktree, WorktrunkMetadata } from "$lib/types";

vi.mock("$lib/bindings", () => ({
  commands: {
    cmdWorktrunkDiagnostics: vi.fn(),
    cmdWorktrunkReadLog: vi.fn(),
    cmdDetectWorktrunk: vi.fn(),
    cmdOpenTerminalAt: vi.fn(),
  },
}));

vi.mock("$lib/tauri", () => ({
  listWorktrees: vi.fn(),
  removeWorktree: vi.fn(),
  createWorktree: vi.fn(),
  createSessionShell: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(),
}));

vi.mock("$lib/panes/profiles", () => ({
  resolveProfileRef: vi.fn().mockReturnValue({
    id: "claude",
    nonoProfile: null,
    nonoAllowDirs: null,
  }),
}));
vi.mock("$lib/panes/profileRunner", () => ({
  runProfileInPane: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("$lib/panes/actions", () => ({
  initSessionWithProfile: vi.fn().mockReturnValue("pane-1"),
}));
vi.mock("$lib/panes/terminals", () => ({
  connectPaneTerminal: vi.fn().mockResolvedValue(undefined),
}));

import { commands } from "$lib/bindings";
import {
  listWorktrees,
  removeWorktree,
  createWorktree,
  createSessionShell,
} from "$lib/tauri";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import WorktrunkPanel from "../WorktrunkPanel.svelte";
import { sessionState } from "$lib/stores/sessions";
import type { Session } from "$lib/types";
import { _setWorktrunkDetectionForTests } from "$lib/stores/worktrunkDetection";
import { _resetWorktreeMetadataForTests } from "$lib/stores/worktreeMetadata";

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: "s1",
    name: "sess",
    repoRoot: "/repo",
    worktreePath: "/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: Date.now() / 1000,
    projectId: null,
    isGitRepo: true,
    nameOverride: null,
    primaryPtyId: "s1",
    archived: false,
    endedAt: null,
    ...overrides,
  };
}

function okDiagnostics(data: WorktrunkDiagnostics) {
  return { status: "ok" as const, data };
}

function makeDiagnostics(
  overrides: Partial<WorktrunkDiagnostics> = {},
): WorktrunkDiagnostics {
  return {
    hooks: [],
    config: {
      userPath: "/Users/t/.config/worktrunk/config.toml",
      userExists: true,
      projectPath: "/repo/.config/wt.toml",
      projectExists: false,
    },
    logs: { commandLog: [], hookOutput: [], diagnostic: [] },
    ...overrides,
  };
}

function makeWorktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    path: "/wt/a",
    branch: "feat-a",
    isMain: false,
    worktrunk: null,
    ...overrides,
  };
}

function makeMetadata(
  overrides: Partial<WorktrunkMetadata> = {},
): WorktrunkMetadata {
  return {
    dirty: false,
    ahead: 0,
    behind: 0,
    locked: false,
    lockReason: null,
    prunable: false,
    prunableReason: null,
    isCurrent: false,
    isPrevious: false,
    ciStatus: null,
    ciUrl: null,
    ciStale: false,
    devServerUrl: null,
    mainState: null,
    ...overrides,
  };
}

describe("WorktrunkPanel", () => {
  beforeEach(() => {
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockReset();
    vi.mocked(commands.cmdWorktrunkReadLog).mockReset();
    vi.mocked(listWorktrees).mockReset().mockResolvedValue([]);
    vi.mocked(removeWorktree).mockReset().mockResolvedValue(undefined);
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
  });

  afterEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: null,
      version: null,
      probed: false,
    });
  });

  it("shows an empty state when no session is active", () => {
    const { getByText } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    expect(
      getByText(/Open a session to view its worktrunk state/i),
    ).toBeDefined();
  });

  it("renders Hooks rows after switching to the Hooks tab", async () => {
    const s = makeSession();
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce(
      okDiagnostics(
        makeDiagnostics({
          hooks: [
            {
              source: "project",
              configPath: "/repo/.config/wt.toml",
              name: "post-start",
              command: "npm run dev",
            },
          ],
        }),
      ),
    );

    const { findAllByTestId, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });

    // Default tab is "worktrees" now; click Hooks to see hook rows.
    await fireEvent.click(getByTestId("worktrunk-tab-hooks"));
    const rows = await findAllByTestId("worktrunk-hook-row");
    expect(rows.length).toBe(1);
    expect(rows[0].textContent).toContain("post-start");
    expect(rows[0].textContent).toContain("npm run dev");
  });

  it("switches tabs and opens a log entry reader on row click", async () => {
    const s = makeSession();
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce(
      okDiagnostics(
        makeDiagnostics({
          logs: {
            commandLog: [
              {
                file: "trace.log",
                path: "/repo/.git/wt/logs/trace.log",
                size: 1234,
                modifiedAt: Math.floor(Date.now() / 1000),
              },
            ],
            hookOutput: [],
            diagnostic: [],
          },
        }),
      ),
    );
    vi.mocked(commands.cmdWorktrunkReadLog).mockResolvedValueOnce({
      status: "ok",
      data: "line 1\nline 2\n",
    });

    const { getByTestId, findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });

    await fireEvent.click(getByTestId("worktrunk-tab-commandLog"));
    await tick();
    const row = (await findAllByTestId("worktrunk-log-row"))[0];
    await fireEvent.click(row);
    const content = await findAllByTestId("worktrunk-reader-content");
    expect(content[0].textContent).toContain("line 1");
    expect(commands.cmdWorktrunkReadLog).toHaveBeenCalledWith(
      "/repo",
      "/repo/.git/wt/logs/trace.log",
    );
  });

  it("surfaces a typed error when diagnostics fail", async () => {
    const s = makeSession();
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce({
      status: "error",
      error: "wt not detected",
    });

    const { findByText, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    // The diagnostics error only shows on the hooks/logs tabs; the
    // worktrees tab has its own error surface. Switch to Hooks.
    await fireEvent.click(getByTestId("worktrunk-tab-hooks"));
    const err = await findByText(/Failed to load diagnostics/i);
    expect(err.textContent).toContain("wt not detected");
  });

  it("shows a short owner/repo label derived from a github-rooted path", async () => {
    const s = makeSession({
      repoRoot: "/Users/sam/src/github.com/acme-corp/backend",
      isWorktree: false,
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce(
      okDiagnostics(makeDiagnostics()),
    );
    const { findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const label = await findByTestId("worktrunk-repo-label");
    expect(label.textContent?.trim()).toBe("acme-corp/backend");
    const strip = await findByTestId("worktrunk-repo-strip");
    // Full path is still available on hover.
    expect(strip.getAttribute("title")).toBe(
      "/Users/sam/src/github.com/acme-corp/backend",
    );
  });

  it("falls back to the last two segments when no forge host is in the path", async () => {
    const s = makeSession({
      repoRoot: "/home/dev/code/myteam/myrepo",
      isWorktree: false,
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce(
      okDiagnostics(makeDiagnostics()),
    );
    const { findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const label = await findByTestId("worktrunk-repo-label");
    expect(label.textContent?.trim()).toBe("myteam/myrepo");
  });

  it("omits the repo strip entirely when no session is active", () => {
    const { queryByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    expect(queryByTestId("worktrunk-repo-strip")).toBeNull();
  });

  it("links to worktrunk.dev hook docs in the empty hooks state", async () => {
    const s = makeSession();
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValueOnce(
      okDiagnostics(makeDiagnostics({ hooks: [] })),
    );

    const { findByTestId, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(getByTestId("worktrunk-tab-hooks"));
    const link = (await findByTestId(
      "worktrunk-hooks-docs-link",
    )) as HTMLAnchorElement;
    expect(link.getAttribute("href")).toBe("https://worktrunk.dev/hook/");
    expect(link.getAttribute("target")).toBe("_blank");
  });

  it("renders nothing extra in the panel header when wt is not detected", () => {
    _setWorktrunkDetectionForTests({
      binaryPath: null,
      version: null,
      probed: true,
    });
    const { container } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    expect(container.textContent).toContain("Worktrunk");
    // No version badge when detection has no version.
    expect(container.textContent).not.toMatch(/0\.44\.0/);
  });
});

describe("WorktrunkPanel Worktrees tab", () => {
  let confirmSpy: ReturnType<typeof vi.spyOn>;
  // Snapshot the real `navigator.clipboard` so the bulk-copy test
  // (which swaps in a vi.fn for `writeText`) doesn't leak the stub
  // into later tests.
  let originalClipboard: typeof navigator.clipboard | undefined;

  beforeEach(() => {
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockReset();
    vi.mocked(commands.cmdWorktrunkReadLog).mockReset();
    vi.mocked(commands.cmdOpenTerminalAt).mockReset();
    vi.mocked(listWorktrees).mockReset();
    vi.mocked(removeWorktree).mockReset();
    vi.mocked(revealItemInDir).mockReset();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
    confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    originalClipboard = navigator.clipboard;
  });

  afterEach(() => {
    confirmSpy.mockRestore();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    if (originalClipboard !== undefined) {
      Object.assign(navigator, { clipboard: originalClipboard });
    }
  });

  it("fetches and renders the worktree list for the session's repo", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({
        path: "/project",
        branch: "main",
        isMain: true,
      }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });

    const rows = await findAllByTestId("worktrunk-worktree-row");
    expect(rows.length).toBe(2);
    expect(listWorktrees).toHaveBeenCalledWith("/project");
  });

  it("filters worktrees by branch or path and clears the filter", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
      makeWorktree({
        path: "/tmp/client-special",
        branch: "topic",
        isMain: false,
      }),
    ]);

    const { findAllByTestId, findByTestId, queryAllByTestId } = render(
      WorktrunkPanel,
      {
        props: { visible: true, onclose: () => {} },
      },
    );
    await findAllByTestId("worktrunk-worktree-row");
    const input = (await findByTestId(
      "worktrunk-filter-input",
    )) as HTMLInputElement;

    await fireEvent.input(input, { target: { value: "feat-a" } });
    await tick();
    expect(queryAllByTestId("worktrunk-worktree-row").length).toBe(1);
    expect(queryAllByTestId("worktrunk-worktree-row")[0].textContent).toContain(
      "feat-a",
    );

    await fireEvent.input(input, { target: { value: "client-special" } });
    await tick();
    expect(queryAllByTestId("worktrunk-worktree-row").length).toBe(1);
    expect(queryAllByTestId("worktrunk-worktree-row")[0].textContent).toContain(
      "topic",
    );

    await fireEvent.click(await findByTestId("worktrunk-filter-clear"));
    await tick();
    expect(queryAllByTestId("worktrunk-worktree-row").length).toBe(3);
  });

  it("shows a filtered empty state when no worktrees match", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
    ]);

    const { findAllByTestId, findByTestId, queryAllByTestId } = render(
      WorktrunkPanel,
      {
        props: { visible: true, onclose: () => {} },
      },
    );
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.input(await findByTestId("worktrunk-filter-input"), {
      target: { value: "does-not-exist" },
    });
    await tick();

    expect(queryAllByTestId("worktrunk-worktree-row").length).toBe(0);
    expect((await findByTestId("worktrunk-filter-empty")).textContent).toContain(
      'No worktrees match "does-not-exist"',
    );
  });

  it("selects and clears removable worktrees from row checkboxes", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);

    const { findByTestId, queryByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const checkbox = (await findByTestId(
      "worktrunk-row-checkbox",
    )) as HTMLInputElement;

    await fireEvent.click(checkbox);
    expect((await findByTestId("worktrunk-bulk-toolbar")).textContent).toContain(
      "1 selected",
    );

    await fireEvent.click(await findByTestId("worktrunk-bulk-clear"));
    await tick();
    expect(queryByTestId("worktrunk-bulk-toolbar")).toBeNull();
    expect(checkbox.checked).toBe(false);
  });

  it("select-all-visible selects only removable visible worktrees", async () => {
    const active = makeSession({
      id: "active",
      repoRoot: "/project",
      worktreePath: "/project-active",
      isWorktree: true,
      branch: "feat-active",
    });
    sessionState.set({ sessions: [active], activeSessionId: active.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-keep",
        branch: "keep",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-active",
        branch: "feat-active",
        isMain: false,
      }),
    ]);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));

    expect((await findByTestId("worktrunk-bulk-toolbar")).textContent).toContain(
      "1 selected",
    );
  });

  it("labels select-all by removable visible worktrees", async () => {
    const active = makeSession({
      id: "active",
      repoRoot: "/project",
      worktreePath: "/project-active",
      isWorktree: true,
      branch: "feat-active",
    });
    sessionState.set({ sessions: [active], activeSessionId: active.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-keep",
        branch: "keep",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-active",
        branch: "feat-active",
        isMain: false,
      }),
    ]);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");

    expect((await findByTestId("worktrunk-select-all")).parentElement?.textContent).toContain(
      "Select removable",
    );

    await fireEvent.input(await findByTestId("worktrunk-filter-input"), {
      target: { value: "project" },
    });
    await tick();

    expect((await findByTestId("worktrunk-select-all")).parentElement?.textContent).toContain(
      "Select 1 removable match",
    );
  });

  it("quick-selects visible merged and prunable worktrees from the header menu", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-merged",
        branch: "merged-a",
        isMain: false,
        worktrunk: makeMetadata({ mainState: "integrated" }),
      }),
      makeWorktree({
        path: "/project-prunable",
        branch: "prunable-a",
        isMain: false,
        worktrunk: makeMetadata({ prunable: true }),
      }),
      makeWorktree({
        path: "/project-other",
        branch: "other-a",
        isMain: false,
        worktrunk: makeMetadata({ mainState: "diverged" }),
      }),
    ]);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");

    await fireEvent.click(await findByTestId("worktrunk-worktrees-header-menu"));
    await fireEvent.click(await findByTestId("worktrunk-select-merged"));
    expect((await findByTestId("worktrunk-bulk-toolbar")).textContent).toContain(
      "1 selected",
    );

    await fireEvent.click(await findByTestId("worktrunk-worktrees-header-menu"));
    await fireEvent.click(await findByTestId("worktrunk-select-prunable"));
    expect((await findByTestId("worktrunk-bulk-toolbar")).textContent).toContain(
      "2 selected",
    );
  });

  it("bulk removes selected worktrees without deleting branches", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-feat-a",
          branch: "feat-a",
          isMain: false,
        }),
        makeWorktree({
          path: "/project-feat-b",
          branch: "feat-b",
          isMain: false,
        }),
      ])
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
      ]);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(2));
    expect(removeWorktree).toHaveBeenNthCalledWith(
      1,
      "/project",
      "/project-feat-a",
      false,
      false,
    );
    expect(removeWorktree).toHaveBeenNthCalledWith(
      2,
      "/project",
      "/project-feat-b",
      false,
      false,
    );
  });

  it("bulk removes selected worktrees and branches", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({
          path: "/project-feat-a",
          branch: "feat-a",
          isMain: false,
        }),
      ])
      .mockResolvedValueOnce([]);

    const { findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(await findByTestId("worktrunk-row-checkbox"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove-and-branch"));

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(1));
    expect(removeWorktree).toHaveBeenCalledWith(
      "/project",
      "/project-feat-a",
      true,
      false,
    );
  });

  it("skips bulk remove calls when the user cancels confirmation", async () => {
    confirmSpy.mockReturnValueOnce(false);
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);

    const { findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(await findByTestId("worktrunk-row-checkbox"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    expect(removeWorktree).not.toHaveBeenCalled();
  });

  it("surfaces partial failures from bulk remove", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-feat-a",
          branch: "feat-a",
          isMain: false,
        }),
        makeWorktree({
          path: "/project-feat-b",
          branch: "feat-b",
          isMain: false,
        }),
      ])
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-feat-b",
          branch: "feat-b",
          isMain: false,
        }),
      ]);
    vi.mocked(removeWorktree).mockImplementation(async (_repo, path) => {
      if (path === "/project-feat-b") throw new Error("locked");
    });

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(2));
    const err = await findByTestId("worktrunk-bulk-error");
    expect(err.textContent).toContain("1 succeeded, 1 failed");
    expect(err.textContent).toContain("feat-b");
    expect(err.textContent).toContain("locked");
  });

  it("offers to force-delete dirty worktrees after bulk remove leaves them behind", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-clean",
          branch: "clean",
          isMain: false,
        }),
        makeWorktree({
          path: "/project-dirty",
          branch: "dirty",
          isMain: false,
        }),
      ])
      // After phase 1: only the dirty one remains.
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-dirty",
          branch: "dirty",
          isMain: false,
        }),
      ])
      // After phase 2 force: dirty also gone.
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
      ]);
    vi.mocked(removeWorktree).mockImplementation(
      async (_repo, path, _alsoBranch, force) => {
        if (path === "/project-dirty" && !force) {
          throw new Error(
            "worktree has uncommitted changes: ✗ Cannot remove worktree: dirty has uncommitted changes",
          );
        }
      },
    );

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(3));
    // Phase 1: both targets attempted with force=false.
    expect(removeWorktree).toHaveBeenNthCalledWith(
      1,
      "/project",
      "/project-clean",
      false,
      false,
    );
    expect(removeWorktree).toHaveBeenNthCalledWith(
      2,
      "/project",
      "/project-dirty",
      false,
      false,
    );
    // Phase 2: dirty re-attempted with force=true after the second confirm.
    expect(removeWorktree).toHaveBeenNthCalledWith(
      3,
      "/project",
      "/project-dirty",
      false,
      true,
    );
    // Two confirms: initial bulk delete + force-delete prompt.
    expect(confirmSpy).toHaveBeenCalledTimes(2);
    expect(confirmSpy.mock.calls[1]?.[0]).toContain("uncommitted changes");
    expect(confirmSpy.mock.calls[1]?.[0]).toContain("dirty");
  });

  it("skips force-delete when the user declines the dirty prompt", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-dirty",
        branch: "dirty",
        isMain: false,
      }),
    ]);
    vi.mocked(removeWorktree).mockImplementation(async () => {
      throw new Error("worktree has uncommitted changes: dirty");
    });
    // First confirm = initial bulk delete (allow); second = force prompt (deny).
    confirmSpy.mockReturnValueOnce(true).mockReturnValueOnce(false);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(1));
    expect(removeWorktree).toHaveBeenCalledWith(
      "/project",
      "/project-dirty",
      false,
      false,
    );
    const err = await findByTestId("worktrunk-bulk-error");
    expect(err.textContent).toContain("skipped (uncommitted changes)");
    expect(err.textContent).toContain("dirty");
  });

  it("offers to force-delete a single worktree from the kebab menu when it's dirty", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValue([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-dirty",
        branch: "dirty",
        isMain: false,
      }),
    ]);
    vi.mocked(removeWorktree).mockImplementation(
      async (_repo, _path, _alsoBranch, force) => {
        if (!force) {
          throw new Error("worktree has uncommitted changes: dirty");
        }
      },
    );

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-menu"]',
      ) as HTMLButtonElement,
    );
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-remove"]',
      ) as HTMLButtonElement,
    );

    await waitFor(() => expect(removeWorktree).toHaveBeenCalledTimes(2));
    expect(removeWorktree).toHaveBeenNthCalledWith(
      1,
      "/project",
      "/project-dirty",
      false,
      false,
    );
    expect(removeWorktree).toHaveBeenNthCalledWith(
      2,
      "/project",
      "/project-dirty",
      false,
      true,
    );
    // Initial confirm + force-delete confirm.
    expect(confirmSpy).toHaveBeenCalledTimes(2);
    expect(confirmSpy.mock.calls[1]?.[0]).toContain("uncommitted changes");
  });

  it("does not refresh the stale repo after bulk remove if the active repo changes", async () => {
    const project = makeSession({
      id: "project",
      repoRoot: "/project",
      worktreePath: "/project",
      isWorktree: false,
    });
    const other = makeSession({
      id: "other",
      repoRoot: "/other",
      worktreePath: "/other",
      isWorktree: false,
    });
    sessionState.set({ sessions: [project, other], activeSessionId: project.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockImplementation(async (repo) => {
      if (repo === "/project") {
        return [
          makeWorktree({
            path: "/project-feat-a",
            branch: "feat-a",
            isMain: false,
          }),
        ];
      }
      if (repo === "/other") {
        return [makeWorktree({ path: "/other", branch: "main", isMain: true })];
      }
      return [];
    });
    vi.mocked(removeWorktree).mockImplementation(async () => {
      sessionState.set({ sessions: [project, other], activeSessionId: other.id });
    });

    const { findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(await findByTestId("worktrunk-row-checkbox"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-remove"));

    await waitFor(() => {
      const projectCalls = vi
        .mocked(listWorktrees)
        .mock.calls.filter(([repo]) => repo === "/project");
      expect(projectCalls.length).toBe(1);
    });
  });

  it("bulk-copies selected worktree paths newline-separated to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-feat-b",
        branch: "feat-b",
        isMain: false,
      }),
    ]);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-more"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-copy-paths"));

    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText).toHaveBeenCalledWith("/project-feat-a\n/project-feat-b");
    expect(await findByTestId("worktrunk-bulk-copied-flash")).toBeDefined();
  });

  it("bulk-reveals selected worktrees in Finder without confirm under the threshold", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-feat-b",
        branch: "feat-b",
        isMain: false,
      }),
    ]);
    vi.mocked(revealItemInDir).mockResolvedValue(undefined);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-more"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-reveal"));

    await waitFor(() => expect(revealItemInDir).toHaveBeenCalledTimes(2));
    expect(revealItemInDir).toHaveBeenNthCalledWith(1, "/project-feat-a");
    expect(revealItemInDir).toHaveBeenNthCalledWith(2, "/project-feat-b");
    // Threshold is 5, so two items must not have prompted.
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it("prompts before bulk-revealing more than the threshold of worktrees", async () => {
    confirmSpy.mockReturnValueOnce(false);
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      ...Array.from({ length: 6 }, (_, i) =>
        makeWorktree({
          path: `/project-feat-${i}`,
          branch: `feat-${i}`,
          isMain: false,
        }),
      ),
    ]);
    vi.mocked(revealItemInDir).mockResolvedValue(undefined);

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-more"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-reveal"));

    expect(confirmSpy).toHaveBeenCalledTimes(1);
    expect(confirmSpy.mock.calls[0]?.[0]).toContain("Reveal 6 worktrees");
    expect(revealItemInDir).not.toHaveBeenCalled();
  });

  it("bulk-opens selected worktrees in terminal under the threshold", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-feat-b",
        branch: "feat-b",
        isMain: false,
      }),
    ]);
    vi.mocked(commands.cmdOpenTerminalAt).mockResolvedValue({
      status: "ok",
      data: null,
    });

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-more"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-open-terminal"));

    await waitFor(() =>
      expect(commands.cmdOpenTerminalAt).toHaveBeenCalledTimes(2),
    );
    expect(commands.cmdOpenTerminalAt).toHaveBeenNthCalledWith(
      1,
      "/project-feat-a",
    );
    expect(commands.cmdOpenTerminalAt).toHaveBeenNthCalledWith(
      2,
      "/project-feat-b",
    );
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it("surfaces partial failures from bulk open-in-terminal", async () => {
    const s = makeSession({ repoRoot: "/project", isWorktree: false });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
      makeWorktree({
        path: "/project-feat-b",
        branch: "feat-b",
        isMain: false,
      }),
    ]);
    vi.mocked(commands.cmdOpenTerminalAt).mockImplementation(async (path) => {
      if (path === "/project-feat-b") {
        return { status: "error", error: "no terminal" };
      }
      return { status: "ok", data: null };
    });

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    await fireEvent.click(await findByTestId("worktrunk-select-all"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-more"));
    await fireEvent.click(await findByTestId("worktrunk-bulk-open-terminal"));

    const err = await findByTestId("worktrunk-bulk-error");
    expect(err.textContent).toContain("1 succeeded, 1 failed");
    expect(err.textContent).toContain("feat-b");
    expect(err.textContent).toContain("no terminal");
  });

  it("disables Remove for the main worktree (inside the kebab menu)", async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
    ]);

    const { findAllByTestId, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    // Open the kebab on the only row.
    const kebabs = await findAllByTestId("worktrunk-worktree-menu");
    await fireEvent.click(kebabs[0]);
    const removeBtn = getByTestId(
      "worktrunk-worktree-remove",
    ) as HTMLButtonElement;
    expect(removeBtn.disabled).toBe(true);
    expect(removeBtn.getAttribute("title")).toContain(
      "Cannot remove the main worktree",
    );
  });

  it("disables Remove for a worktree that has an active Roux session", async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project-feat-a",
      isWorktree: true,
      branch: "feat-a",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    // Open this row's kebab specifically.
    const kebab = featRow.querySelector(
      '[data-testid="worktrunk-worktree-menu"]',
    ) as HTMLButtonElement;
    await fireEvent.click(kebab);
    const removeBtn = featRow.querySelector(
      '[data-testid="worktrunk-worktree-remove"]',
    ) as HTMLButtonElement;
    expect(removeBtn.disabled).toBe(true);
    expect(removeBtn.getAttribute("title")).toContain(
      "A Roux session is active",
    );
  });

  it("calls removeWorktree(repo, path, false) on Remove from the kebab menu", async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-feat-a",
          branch: "feat-a",
          isMain: false,
        }),
      ])
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
      ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    const kebab = featRow.querySelector(
      '[data-testid="worktrunk-worktree-menu"]',
    ) as HTMLButtonElement;
    await fireEvent.click(kebab);
    const removeBtn = featRow.querySelector(
      '[data-testid="worktrunk-worktree-remove"]',
    ) as HTMLButtonElement;
    await fireEvent.click(removeBtn);
    expect(confirmSpy).toHaveBeenCalled();
    expect(removeWorktree).toHaveBeenCalledWith(
      "/project",
      "/project-feat-a",
      false,
      false,
    );
  });

  it("skips the remove call when the user cancels the confirm dialog", async () => {
    confirmSpy.mockReturnValueOnce(false);
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-menu"]',
      ) as HTMLButtonElement,
    );
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-remove"]',
      ) as HTMLButtonElement,
    );
    expect(removeWorktree).not.toHaveBeenCalled();
  });

  it("calls removeWorktree with also_branch=true via the kebab menu", async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
        makeWorktree({
          path: "/project-feat-a",
          branch: "feat-a",
          isMain: false,
        }),
      ])
      .mockResolvedValueOnce([
        makeWorktree({ path: "/project", branch: "main", isMain: true }),
      ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-menu"]',
      ) as HTMLButtonElement,
    );
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-remove-and-branch"]',
      ) as HTMLButtonElement,
    );
    expect(removeWorktree).toHaveBeenCalledWith(
      "/project",
      "/project-feat-a",
      true,
      false,
    );
  });

  it("surfaces a remove error inline without wiping the list", async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      makeWorktree({ path: "/project", branch: "main", isMain: true }),
      makeWorktree({
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
      }),
    ]);
    vi.mocked(removeWorktree).mockRejectedValueOnce(
      "worktree is locked (wt): user lock",
    );

    const { findAllByTestId, findByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const rows = await findAllByTestId("worktrunk-worktree-row");
    const featRow = rows[1];
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-menu"]',
      ) as HTMLButtonElement,
    );
    await fireEvent.click(
      featRow.querySelector(
        '[data-testid="worktrunk-worktree-remove"]',
      ) as HTMLButtonElement,
    );
    const errBanner = await findByTestId("worktrunk-worktrees-error");
    expect(errBanner.textContent).toContain("Failed to remove feat-a");
    expect(errBanner.textContent).toContain("locked");
  });
});

describe("ActivityRail worktrunk gating", () => {
  beforeEach(() => {
    _setWorktrunkDetectionForTests({
      binaryPath: null,
      version: null,
      probed: false,
    });
  });

  afterEach(() => {
    _setWorktrunkDetectionForTests({
      binaryPath: null,
      version: null,
      probed: false,
    });
  });

  it("does not render the Worktrunk button when wt is not detected", async () => {
    const ActivityRail = (await import("../ActivityRail.svelte")).default;
    const { queryByRole } = render(ActivityRail);
    expect(queryByRole("button", { name: /worktrunk/i })).toBeNull();
  });

  it("renders a Worktrunk button when wt is detected", async () => {
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
    const ActivityRail = (await import("../ActivityRail.svelte")).default;
    const { findByRole } = render(ActivityRail);
    const btn = await findByRole("button", { name: /worktrunk/i });
    expect(btn).toBeDefined();
  });
});

describe("WorktrunkPanel — New worktree form", () => {
  let confirmSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockReset();
    vi.mocked(listWorktrees).mockReset().mockResolvedValue([]);
    vi.mocked(createWorktree).mockReset();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
    confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  afterEach(() => {
    confirmSpy.mockRestore();
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it("opens + submits the new-worktree form and refetches the list", async () => {
    const s = makeSession({ repoRoot: "/project" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees)
      .mockResolvedValueOnce([
        { path: "/project", branch: "main", isMain: true, worktrunk: null },
      ])
      .mockResolvedValueOnce([
        { path: "/project", branch: "main", isMain: true, worktrunk: null },
        {
          path: "/project-feat-x",
          branch: "feat-x",
          isMain: false,
          worktrunk: null,
        },
      ]);
    vi.mocked(createWorktree).mockResolvedValueOnce("/project-feat-x");

    const { findByTestId, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(await findByTestId("worktrunk-new-worktree-open"));
    const input = (await findByTestId(
      "worktrunk-new-worktree-branch",
    )) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "feat-x" } });
    // Pick a "main" base so the payload is distinctive.
    await fireEvent.click(getByTestId("worktrunk-new-worktree-base-main"));
    await fireEvent.click(getByTestId("worktrunk-new-worktree-submit"));

    expect(createWorktree).toHaveBeenCalledWith("/project", "feat-x", {
      startPoint: "main",
      fetchFirst: false,
    });
  });

  it("sends fetchFirst=true when base is origin/main", async () => {
    const s = makeSession({ repoRoot: "/project" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValue([]);
    vi.mocked(createWorktree).mockResolvedValueOnce("/project-feat-y");

    const { findByTestId, getByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await fireEvent.click(await findByTestId("worktrunk-new-worktree-open"));
    const input = (await findByTestId(
      "worktrunk-new-worktree-branch",
    )) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "feat-y" } });
    await fireEvent.click(getByTestId("worktrunk-new-worktree-base-originMain"));
    await fireEvent.click(getByTestId("worktrunk-new-worktree-submit"));

    expect(createWorktree).toHaveBeenCalledWith("/project", "feat-y", {
      startPoint: "origin/main",
      fetchFirst: true,
    });
  });
});

describe("WorktrunkPanel — Focus / New session buttons", () => {
  beforeEach(() => {
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockReset();
    vi.mocked(listWorktrees).mockReset();
    vi.mocked(createSessionShell).mockReset();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
  });

  afterEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  it('shows "Focus" on rows whose worktree has an active session', async () => {
    const s = makeSession({
      id: "main-sess",
      name: "main-sess",
      repoRoot: "/project",
      worktreePath: "/project",
    });
    const feat = makeSession({
      id: "feat-sess",
      repoRoot: "/project",
      worktreePath: "/project-feat-a",
      branch: "feat-a",
      isWorktree: true,
    });
    sessionState.set({
      sessions: [s, feat],
      activeSessionId: s.id,
    });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      { path: "/project", branch: "main", isMain: true, worktrunk: null },
      {
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
        worktrunk: null,
      },
    ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const focusBtns = await findAllByTestId("worktrunk-worktree-focus");
    // Both rows have an active session here (main + feat), so two Focus buttons.
    expect(focusBtns.length).toBe(2);
  });

  it('clicking "Focus" activates the session', async () => {
    const s = makeSession({
      id: "s-a",
      repoRoot: "/project",
      worktreePath: "/project",
    });
    const feat = makeSession({
      id: "s-feat",
      repoRoot: "/project",
      worktreePath: "/project-feat-a",
      branch: "feat-a",
      isWorktree: true,
    });
    sessionState.set({ sessions: [s, feat], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      { path: "/project", branch: "main", isMain: true, worktrunk: null },
      {
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
        worktrunk: null,
      },
    ]);

    const { findAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const focusBtns = await findAllByTestId("worktrunk-worktree-focus");
    await fireEvent.click(focusBtns[1]); // row for feat-a
    expect(get(sessionState).activeSessionId).toBe("s-feat");
  });

  it('shows "New session" when no session owns the worktree', async () => {
    const s = makeSession({
      repoRoot: "/project",
      worktreePath: "/project",
    });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      { path: "/project", branch: "main", isMain: true, worktrunk: null },
      {
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
        worktrunk: null,
      },
    ]);

    const { findAllByTestId, queryAllByTestId } = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    await findAllByTestId("worktrunk-worktree-row");
    const newBtns = queryAllByTestId("worktrunk-worktree-new-session");
    // Only feat-a (no session). Main has a session.
    expect(newBtns.length).toBe(1);
    expect(queryAllByTestId("worktrunk-worktree-focus").length).toBe(1);
  });
});

describe("WorktrunkPanel — right-click context menu", () => {
  beforeEach(() => {
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockReset();
    vi.mocked(commands.cmdOpenTerminalAt).mockReset();
    vi.mocked(listWorktrees).mockReset();
    vi.mocked(revealItemInDir).mockReset();
    sessionState.set({ sessions: [], activeSessionId: null });
    _resetWorktreeMetadataForTests();
    _setWorktrunkDetectionForTests({
      binaryPath: "/opt/homebrew/bin/wt",
      version: "0.44.0",
      probed: true,
    });
  });

  afterEach(() => {
    sessionState.set({ sessions: [], activeSessionId: null });
  });

  async function openMenu() {
    const s = makeSession({ repoRoot: "/project" });
    sessionState.set({ sessions: [s], activeSessionId: s.id });
    vi.mocked(commands.cmdWorktrunkDiagnostics).mockResolvedValue(
      okDiagnostics(makeDiagnostics()),
    );
    vi.mocked(listWorktrees).mockResolvedValueOnce([
      {
        path: "/project-feat-a",
        branch: "feat-a",
        isMain: false,
        worktrunk: null,
      },
    ]);
    const result = render(WorktrunkPanel, {
      props: { visible: true, onclose: () => {} },
    });
    const row = (await result.findAllByTestId("worktrunk-worktree-row"))[0];
    await fireEvent.contextMenu(row);
    return result;
  }

  it("reveals a menu with the three OS actions on right-click", async () => {
    const r = await openMenu();
    expect(await r.findByTestId("worktrunk-context-copy")).toBeDefined();
    expect(await r.findByTestId("worktrunk-context-reveal")).toBeDefined();
    expect(await r.findByTestId("worktrunk-context-terminal")).toBeDefined();
  });

  it('invokes revealItemInDir with the worktree path on "Reveal in Finder"', async () => {
    vi.mocked(revealItemInDir).mockResolvedValueOnce(undefined);
    const r = await openMenu();
    await fireEvent.click(await r.findByTestId("worktrunk-context-reveal"));
    expect(revealItemInDir).toHaveBeenCalledWith("/project-feat-a");
  });

  it('invokes cmdOpenTerminalAt on "Open in terminal"', async () => {
    vi.mocked(commands.cmdOpenTerminalAt).mockResolvedValueOnce({
      status: "ok",
      data: null,
    });
    const r = await openMenu();
    await fireEvent.click(await r.findByTestId("worktrunk-context-terminal"));
    expect(commands.cmdOpenTerminalAt).toHaveBeenCalledWith("/project-feat-a");
  });

  it("shows an inline error when the terminal invocation fails", async () => {
    vi.mocked(commands.cmdOpenTerminalAt).mockResolvedValueOnce({
      status: "error",
      error: "no terminal found",
    });
    const r = await openMenu();
    await fireEvent.click(await r.findByTestId("worktrunk-context-terminal"));
    const err = await r.findByTestId("worktrunk-context-error");
    expect(err.textContent).toContain("no terminal found");
  });
});

void get;
