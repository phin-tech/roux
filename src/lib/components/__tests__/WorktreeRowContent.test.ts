import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import WorktreeRowContent from "../WorktreeRowContent.svelte";
import type { Worktree, WorktrunkMetadata } from "$lib/types";

vi.mock("$lib/tauri", async () => {
  const actual =
    await vi.importActual<typeof import("$lib/tauri")>("$lib/tauri");
  return {
    ...actual,
    lookupPrForBranch: vi.fn(),
  };
});

import { lookupPrForBranch } from "$lib/tauri";
import { _resetSessionPrLookupForTests } from "$lib/stores/sessionPrLookup";

function makeMetadata(overrides: Partial<WorktrunkMetadata> = {}): WorktrunkMetadata {
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
    devServerUrl: null,
    mainState: null,
    ciStatus: null,
    ciUrl: null,
    ciStale: false,
    ...overrides,
  };
}

function makeWorktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    path: "/tmp/repo",
    branch: "main",
    isMain: false,
    worktrunk: null,
    ...overrides,
  };
}

describe("WorktreeRowContent", () => {
  beforeEach(() => {
    vi.mocked(lookupPrForBranch).mockReset();
    _resetSessionPrLookupForTests();
  });
  afterEach(() => {
    _resetSessionPrLookupForTests();
  });

  it("renders branch + path when no worktrunk metadata is present", () => {
    const { queryByTestId, container } = render(WorktreeRowContent, {
      props: { wt: makeWorktree({ branch: "feat", path: "/tmp/repo-feat" }) },
    });
    expect(container.textContent).toContain("feat");
    expect(container.textContent).toContain("/tmp/repo-feat");
    // No metadata chips at all.
    expect(queryByTestId("wt-dirty-dot")).toBeNull();
    expect(queryByTestId("wt-ahead-behind")).toBeNull();
    expect(queryByTestId("wt-locked")).toBeNull();
    expect(queryByTestId("wt-prunable")).toBeNull();
    expect(queryByTestId("wt-current-badge")).toBeNull();
  });

  it("shows the main badge when isMain is true", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: { wt: makeWorktree({ isMain: true }) },
    });
    expect(getByTestId("wt-main-badge").textContent).toBe("main");
  });

  it("shows a dirty dot when worktrunk.dirty is true", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ dirty: true }) }),
      },
    });
    expect(getByTestId("wt-dirty-dot")).toBeDefined();
  });

  it("renders ahead/behind counts when nonzero", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ ahead: 3, behind: 1 }) }),
      },
    });
    const node = getByTestId("wt-ahead-behind");
    expect(node.textContent).toContain("↑3");
    expect(node.textContent).toContain("↓1");
    expect(node.getAttribute("title")).toContain("3 ahead");
    expect(node.getAttribute("title")).toContain("1 behind");
  });

  it("hides ahead/behind when both are zero", () => {
    const { queryByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ ahead: 0, behind: 0 }) }),
      },
    });
    expect(queryByTestId("wt-ahead-behind")).toBeNull();
  });

  it("shows a lock icon with reason tooltip when locked", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({ locked: true, lockReason: "release cut" }),
        }),
      },
    });
    const node = getByTestId("wt-locked");
    expect(node.getAttribute("title")).toContain("release cut");
  });

  it("shows a prunable badge with reason tooltip", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({ prunable: true, prunableReason: "gitdir missing" }),
        }),
      },
    });
    const node = getByTestId("wt-prunable");
    expect(node.textContent).toBe("prunable");
    expect(node.getAttribute("title")).toContain("gitdir missing");
  });

  it("marks the current worktree", () => {
    const { getByTestId, queryByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ isCurrent: true }) }),
      },
    });
    expect(getByTestId("wt-current-badge").textContent).toBe("current");
    expect(queryByTestId("wt-previous-badge")).toBeNull();
  });

  it("marks the previous worktree when not current", () => {
    const { getByTestId, queryByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({ isCurrent: false, isPrevious: true }),
        }),
      },
    });
    expect(getByTestId("wt-previous-badge").textContent).toBe("prev");
    expect(queryByTestId("wt-current-badge")).toBeNull();
  });

  it("renders a merged badge when mainState == 'integrated'", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ mainState: "integrated" }) }),
      },
    });
    expect(getByTestId("wt-merged-badge").textContent).toBe("merged");
  });

  it("hides the merged badge for other mainState values", () => {
    const { queryByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ mainState: "diverged" }) }),
      },
    });
    expect(queryByTestId("wt-merged-badge")).toBeNull();
  });

  it("renders a green CI chip when status == 'passed'", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({
            ciStatus: "passed",
            ciUrl: "https://example.com/pr/1",
          }),
        }),
      },
    });
    const a = getByTestId("wt-ci") as HTMLAnchorElement;
    expect(a.className).toContain("text-green");
    expect(a.querySelector("svg")).not.toBeNull();
    expect(a.getAttribute("href")).toBe("https://example.com/pr/1");
    expect(a.getAttribute("title")).toContain("passed");
  });

  it("renders a non-link CI chip when ciUrl is null", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({ ciStatus: "failed", ciUrl: null }),
        }),
      },
    });
    const el = getByTestId("wt-ci");
    expect(el.tagName).toBe("SPAN");
    expect(el.className).toContain("text-red");
    expect(el.querySelector("svg")).not.toBeNull();
  });

  it("hides the CI chip when status == 'no-ci'", () => {
    const { queryByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({ worktrunk: makeMetadata({ ciStatus: "no-ci" }) }),
      },
    });
    expect(queryByTestId("wt-ci")).toBeNull();
  });

  it("marks a stale CI chip with opacity-60 and a tooltip suffix", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({
            ciStatus: "passed",
            ciUrl: "https://example.com",
            ciStale: true,
          }),
        }),
      },
    });
    const a = getByTestId("wt-ci");
    expect(a.className).toContain("opacity-60");
    expect(a.getAttribute("title")).toContain("stale");
  });

  it("triggers a PR lookup once on first hover when repoRoot is set", async () => {
    vi.mocked(lookupPrForBranch).mockResolvedValue(null);
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          branch: "feat-x",
          worktrunk: makeMetadata({
            ciStatus: "passed",
            ciUrl: "https://example.com",
          }),
        }),
        repoRoot: "/repo",
      },
    });
    const chipWrap = getByTestId("wt-ci").parentElement as HTMLElement;
    await fireEvent.mouseEnter(chipWrap);
    await waitFor(() =>
      expect(lookupPrForBranch).toHaveBeenCalledWith("/repo", "feat-x"),
    );
    // Re-hover doesn't refire; the first attempt is sticky.
    await fireEvent.mouseEnter(chipWrap);
    await fireEvent.mouseEnter(chipWrap);
    expect(lookupPrForBranch).toHaveBeenCalledTimes(1);
  });

  it("does not look up when repoRoot is missing or branch is main", async () => {
    vi.mocked(lookupPrForBranch).mockResolvedValue(null);
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          branch: "main",
          isMain: true,
          worktrunk: makeMetadata({
            ciStatus: "passed",
            ciUrl: "https://example.com",
          }),
        }),
        repoRoot: "/repo",
      },
    });
    const chipWrap = getByTestId("wt-ci").parentElement as HTMLElement;
    await fireEvent.mouseEnter(chipWrap);
    expect(lookupPrForBranch).not.toHaveBeenCalled();
  });

  it("renders the popover content once PR data lands", async () => {
    vi.mocked(lookupPrForBranch).mockResolvedValue({
      number: 42,
      title: "Fix things",
      headRef: "feat-x",
      headOwner: "user",
      isCrossRepository: false,
      url: "https://example.com/pr/42",
      repoSlug: "user/repo",
      checks: null,
      checkRuns: [{ name: "build", status: "passing", url: null }],
      reviewDecision: null,
      reviewDetails: [
        { reviewer: "alice", state: "APPROVED", url: null },
      ],
    });
    const { getByTestId, findByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          branch: "feat-x",
          worktrunk: makeMetadata({
            ciStatus: "passed",
            ciUrl: "https://example.com",
          }),
        }),
        repoRoot: "/repo",
      },
    });
    const chipWrap = getByTestId("wt-ci").parentElement as HTMLElement;
    await fireEvent.mouseEnter(chipWrap);
    const popover = await findByTestId("wt-ci-popover");
    expect(popover.textContent).toContain("build");
    expect(popover.textContent).toContain("passing");
    expect(popover.textContent).toContain("alice");
    expect(popover.textContent).toContain("approved");
  });

  it("renders dev-server link when devServerUrl is set", () => {
    const { getByTestId } = render(WorktreeRowContent, {
      props: {
        wt: makeWorktree({
          worktrunk: makeMetadata({ devServerUrl: "http://localhost:3000" }),
        }),
      },
    });
    const a = getByTestId("wt-dev-server") as HTMLAnchorElement;
    expect(a.getAttribute("href")).toBe("http://localhost:3000");
  });
});
