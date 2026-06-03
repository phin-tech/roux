import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/svelte";
import type { WorktrunkMetadata, Worktree } from "$lib/types";

vi.mock("$lib/tauri", () => ({
  listWorktrees: vi.fn(),
}));

import SessionWorktrunkChips from "../SessionWorktrunkChips.svelte";
import {
  _resetWorktreeMetadataForTests,
  upsertWorktreeMetadata,
} from "$lib/stores/worktreeMetadata";

function makeMeta(
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
    devServerUrl: null,
    mainState: null,
    ciStatus: null,
    ciUrl: null,
    ciStale: false,
    ...overrides,
  };
}

function seed(path: string, meta: WorktrunkMetadata | null) {
  const wt: Worktree = {
    path,
    branch: "feat",
    isMain: false,
    worktrunk: meta,
  };
  upsertWorktreeMetadata([wt]);
}

describe("SessionWorktrunkChips", () => {
  beforeEach(() => {
    _resetWorktreeMetadataForTests();
  });

  afterEach(() => {
    _resetWorktreeMetadataForTests();
  });

  it("renders nothing when the worktree has no metadata", () => {
    const { queryByTestId, container } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/unknown" },
    });
    expect(queryByTestId("session-wt-dirty")).toBeNull();
    expect(queryByTestId("session-wt-ahead-behind")).toBeNull();
    expect(queryByTestId("session-wt-locked")).toBeNull();
    expect(queryByTestId("session-wt-dev-server")).toBeNull();
    expect(container.textContent?.trim()).toBe("");
  });

  it("renders nothing when worktreePath is null or empty", () => {
    const { container: c1 } = render(SessionWorktrunkChips, {
      props: { worktreePath: null },
    });
    expect(c1.textContent?.trim()).toBe("");
    const { container: c2 } = render(SessionWorktrunkChips, {
      props: { worktreePath: "" },
    });
    expect(c2.textContent?.trim()).toBe("");
  });

  it("shows a dirty dot when metadata.dirty", () => {
    seed("/wt/a", makeMeta({ dirty: true }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/a" },
    });
    expect(getByTestId("session-wt-dirty")).toBeDefined();
  });

  it("shows ahead and behind counts", () => {
    seed("/wt/b", makeMeta({ ahead: 4, behind: 2 }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/b" },
    });
    const n = getByTestId("session-wt-ahead-behind");
    expect(n.textContent).toContain("↑4");
    expect(n.textContent).toContain("↓2");
  });

  it("shows lock icon with reason in title", () => {
    seed("/wt/c", makeMeta({ locked: true, lockReason: "release cut" }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/c" },
    });
    expect(getByTestId("session-wt-locked").getAttribute("title")).toContain(
      "release cut",
    );
  });

  it("renders a green CI chip on passed", () => {
    seed("/wt/ci-pass", makeMeta({ ciStatus: "passed" }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/ci-pass" },
    });
    const el = getByTestId("session-wt-ci");
    expect(el.className).toContain("text-green");
    expect(el.querySelector("svg")).not.toBeNull();
    expect(el.getAttribute("title")).toContain("passed");
  });

  it("renders a red CI chip on failed", () => {
    seed("/wt/ci-fail", makeMeta({ ciStatus: "failed" }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/ci-fail" },
    });
    const el = getByTestId("session-wt-ci");
    expect(el.className).toContain("text-red");
    expect(el.querySelector("svg")).not.toBeNull();
  });

  it("hides the CI chip when status == 'no-ci'", () => {
    seed("/wt/ci-none", makeMeta({ ciStatus: "no-ci" }));
    const { queryByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/ci-none" },
    });
    expect(queryByTestId("session-wt-ci")).toBeNull();
  });

  it("renders dev-server link when URL is set", () => {
    seed("/wt/d", makeMeta({ devServerUrl: "http://localhost:3000" }));
    const { getByTestId } = render(SessionWorktrunkChips, {
      props: { worktreePath: "/wt/d" },
    });
    const a = getByTestId("session-wt-dev-server") as HTMLAnchorElement;
    expect(a.getAttribute("href")).toBe("http://localhost:3000");
    expect(a.getAttribute("target")).toBe("_blank");
  });
});
