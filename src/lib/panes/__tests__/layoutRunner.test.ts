import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

// ── Mocks ───────────────────────────────────────────────────────────────────

vi.mock("$lib/tauri", () => ({
  spawnShell: vi.fn().mockResolvedValue(undefined),
  killPty: vi.fn().mockResolvedValue(undefined),
  upsertPaneRecord: vi.fn().mockResolvedValue(undefined),
  removePaneRecord: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/logging", () => ({
  log: vi.fn(),
  logError: vi.fn(),
}));

vi.mock("$lib/panes/terminals", () => {
  const initTerminal = vi.fn();
  const attachPtyListeners = vi.fn().mockResolvedValue(undefined);
  return {
    initTerminal,
    attachPtyListeners,
    connectPaneTerminal: vi.fn(async (paneId: string, onExit?: unknown) => {
      initTerminal(paneId);
      return attachPtyListeners(paneId, onExit);
    }),
  };
});

vi.mock("$lib/panes/profileRunner", () => ({
  runProfileInPane: vi.fn().mockResolvedValue(undefined),
}));

// Mock actions for closePane (used in secondary leaf onExit callbacks)
vi.mock("$lib/panes/actions", () => ({
  closePane: vi.fn(),
}));

// ── Imports (after mocks) ───────────────────────────────────────────────────

import { applyLayoutToSession } from "../layoutRunner";
import { spawnShell, killPty } from "$lib/tauri";
import { initTerminal, attachPtyListeners } from "$lib/panes/terminals";
import { runProfileInPane } from "$lib/panes/profileRunner";
import {
  resetProfileRegistry,
  setUserProfiles,
} from "../profiles";
import type { SpawnProfile } from "../profiles";
import { paneInstances, resetInstances, getInstance } from "../instances";
import { sessionLayouts, resetLayouts } from "../layout";
import { focusedPaneId, resetFocus } from "../focus";
import type { LayoutSpec, LayoutPaneNode, Session } from "$lib/bindings";

// ── Helpers ─────────────────────────────────────────────────────────────────

function stubProfile(id: string, extras: Partial<SpawnProfile> = {}): SpawnProfile {
  return {
    id,
    name: id,
    source: "user",
    ...extras,
  };
}

function stubSession(id: string): Session {
  return {
    id,
    name: "test-session",
    repoRoot: "/tmp/repo",
    worktreePath: "/tmp/repo",
    branch: "main",
    isWorktree: false,
    status: "idle",
    model: null,
    cost: null,
    createdAt: Date.now(),
  };
}

function leafNode(
  profileId: string,
  opts: { name?: string; size?: number; nono_profile?: string; nono_allow_dirs?: string[] } = {},
): LayoutPaneNode {
  return {
    kind: "leaf",
    profile_ref: { kind: "registered", id: profileId },
    name: opts.name ?? null,
    size: opts.size ?? null,
    cwd: null,
    nono_profile: opts.nono_profile ?? null,
    nono_allow_dirs: opts.nono_allow_dirs ?? null,
  };
}

function inlineLeafNode(
  profile: SpawnProfile,
  opts: { name?: string; size?: number; nono_profile?: string; nono_allow_dirs?: string[] } = {},
): LayoutPaneNode {
  return {
    kind: "leaf",
    profile_ref: { kind: "inline", profile },
    name: opts.name ?? null,
    size: opts.size ?? null,
    cwd: null,
    nono_profile: opts.nono_profile ?? null,
    nono_allow_dirs: opts.nono_allow_dirs ?? null,
  };
}

function splitNode(
  direction: "horizontal" | "vertical",
  children: LayoutPaneNode[],
  size?: number,
): LayoutPaneNode {
  return {
    kind: "split",
    direction,
    children,
    size: size ?? null,
  };
}

function layoutSpec(root: LayoutPaneNode, id = "test-layout"): LayoutSpec {
  return {
    id,
    name: "Test Layout",
    source: "builtin",
    root,
  };
}

// ── Setup / teardown ────────────────────────────────────────────────────────

describe("applyLayoutToSession", () => {
  beforeEach(() => {
    resetProfileRegistry();
    resetInstances();
    resetLayouts();
    resetFocus();
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(killPty).mockReset().mockResolvedValue(undefined);
    vi.mocked(initTerminal).mockReset();
    vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);
    vi.mocked(runProfileInPane).mockReset().mockResolvedValue(undefined);
  });

  // ── Test 1: Single leaf ──────────────────────────────────────────────────

  it("creates a single leaf session from a single-leaf layout", async () => {
    const claude = stubProfile("claude", { startupCommand: "claude" });
    setUserProfiles([claude]);

    const session = stubSession("s1");
    const layout = layoutSpec(leafNode("claude"));

    const result = await applyLayoutToSession(session, layout);

    expect(result).toEqual({
      ok: true,
      mainPaneId: "s1-main",
      warnings: [],
    });

    // sessionLayouts: single leaf
    const tree = get(sessionLayouts).get("s1");
    expect(tree).toEqual({ kind: "leaf", paneId: "s1-main" });

    // paneInstances: one entry
    const inst = getInstance("s1-main");
    expect(inst).toBeDefined();
    expect(inst!.type).toBe("shell");
    expect(inst!.ptyId).toBe("s1");
    expect(inst!.spawnProfileRef).toEqual({ kind: "registered", id: "claude" });

    // spawnShell NOT called (first leaf reuses session PTY)
    expect(spawnShell).not.toHaveBeenCalled();

    // connectPaneTerminal fans out to the init + attach boundary once
    expect(initTerminal).toHaveBeenCalledTimes(1);
    expect(initTerminal).toHaveBeenCalledWith("s1-main");
    expect(attachPtyListeners).toHaveBeenCalledTimes(1);

    // runProfileInPane called with the resolved profile
    expect(runProfileInPane).toHaveBeenCalledTimes(1);
    expect(runProfileInPane).toHaveBeenCalledWith("s1", claude, {
      smolMachineName: null,
    });

    // Focus set to main pane
    expect(get(focusedPaneId)).toBe("s1-main");
  });

  // ── Test 2: Horizontal split ─────────────────────────────────────────────

  it("creates a horizontal split session", async () => {
    const claude = stubProfile("claude", { startupCommand: "claude" });
    const shell = stubProfile("plain-shell");
    setUserProfiles([claude, shell]);

    const session = stubSession("s2");
    const layout = layoutSpec(
      splitNode("horizontal", [leafNode("claude"), leafNode("plain-shell")]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    // Tree shape
    const tree = get(sessionLayouts).get("s2")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("h");
      expect(tree.children).toHaveLength(2);
      expect(tree.children[0].kind).toBe("leaf");
      expect(tree.children[1].kind).toBe("leaf");
    }

    // Two pane instances
    const instances = get(paneInstances);
    const layoutPanes = Array.from(instances.values()).filter(
      (p) => p.ptyId === "s2" || p.spawnProfileRef?.kind === "registered",
    );
    expect(layoutPanes).toHaveLength(2);

    // spawnShell called once (second leaf only)
    expect(spawnShell).toHaveBeenCalledTimes(1);

    // runProfileInPane called twice
    expect(runProfileInPane).toHaveBeenCalledTimes(2);
    expect(runProfileInPane).toHaveBeenCalledWith("s2", claude, {
      smolMachineName: null,
    });
  });

  // ── Test 3: Nested 2x2 split (no flattening) ────────────────────────────

  it("preserves nested 2x2 split structure without flattening", async () => {
    const p = stubProfile("p");
    setUserProfiles([p]);

    const session = stubSession("s3");
    const layout = layoutSpec(
      splitNode("horizontal", [
        splitNode("vertical", [leafNode("p"), leafNode("p")]),
        splitNode("vertical", [leafNode("p"), leafNode("p")]),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    const tree = get(sessionLayouts).get("s3")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("h");
      expect(tree.children).toHaveLength(2);

      // Each child is a v-split with 2 leaves
      for (const child of tree.children) {
        expect(child.kind).toBe("split");
        if (child.kind === "split") {
          expect(child.direction).toBe("v");
          expect(child.children).toHaveLength(2);
          for (const leaf of child.children) {
            expect(leaf.kind).toBe("leaf");
          }
        }
      }
    }

    // 4 leaves total, 3 spawnShell calls (first leaf is free)
    expect(spawnShell).toHaveBeenCalledTimes(3);

    // 4 pane instances
    const instances = get(paneInstances);
    expect(instances.size).toBe(4);
  });

  // ── Test 4: Inline profile refs ──────────────────────────────────────────

  it("passes inline profile refs through untouched", async () => {
    const inlineProfile = stubProfile("custom-inline", {
      source: "inline",
      startupCommand: "echo hello",
    });

    const session = stubSession("s4");
    const layout = layoutSpec(inlineLeafNode(inlineProfile));

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    // runProfileInPane called with the inline profile verbatim
    expect(runProfileInPane).toHaveBeenCalledTimes(1);
    expect(runProfileInPane).toHaveBeenCalledWith("s4", inlineProfile, {
      smolMachineName: null,
    });

    // Pane's spawnProfileRef is inline
    const inst = getInstance("s4-main");
    expect(inst!.spawnProfileRef).toEqual({
      kind: "inline",
      profile: inlineProfile,
    });
  });

  // ── Test 5: Missing profile → error before spawning ──────────────────────

  it("returns missingProfile error before spawning any PTY", async () => {
    // Profile registry is empty — "ghost" does not exist
    const session = stubSession("s5");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("ghost", { name: "My Pane" }),
        leafNode("ghost"),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);

    expect(result).toEqual({
      ok: false,
      error: {
        kind: "missingProfile",
        profileId: "ghost",
        paneName: "My Pane",
      },
    });

    // No spawns happened
    expect(spawnShell).not.toHaveBeenCalled();

    // No pane instances created
    expect(get(paneInstances).size).toBe(0);

    // No layout written
    expect(get(sessionLayouts).has("s5")).toBe(false);
  });

  // ── Test 6: Unwind on spawn failure ──────────────────────────────────────

  it("unwinds already-spawned PTYs on a mid-walk spawn failure", async () => {
    const p = stubProfile("p");
    setUserProfiles([p]);

    const session = stubSession("s6");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("p"),
        leafNode("p"),
        leafNode("p"),
      ]),
    );

    // First spawnShell call (leaf 2) succeeds, second (leaf 3) rejects
    let spawnCallCount = 0;
    vi.mocked(spawnShell).mockImplementation(async () => {
      spawnCallCount++;
      if (spawnCallCount === 2) {
        throw new Error("PTY pool exhausted");
      }
    });

    const result = await applyLayoutToSession(session, layout);

    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error.kind).toBe("spawnFailed");
      if (result.error.kind === "spawnFailed") {
        expect(result.error.cause).toContain("PTY pool exhausted");
      }
    }

    // killPty called once (for the successfully-spawned leaf 2)
    expect(killPty).toHaveBeenCalledTimes(1);

    // No pane instances created (cleanup happened before createPane)
    expect(get(paneInstances).size).toBe(0);

    // No layout written
    expect(get(sessionLayouts).has("s6")).toBe(false);
  });

  // ── Test 7: Profile-run warnings ─────────────────────────────────────────

  it("collects runProfileInPane failures as warnings without tearing down panes", async () => {
    const p1 = stubProfile("p1");
    const p2 = stubProfile("p2");
    const p3 = stubProfile("p3");
    setUserProfiles([p1, p2, p3]);

    const session = stubSession("s7");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("p1", { name: "Left" }),
        leafNode("p2", { name: "Center" }),
        leafNode("p3", { name: "Right" }),
      ]),
    );

    // runProfileInPane rejects on the 2nd call
    let runCount = 0;
    vi.mocked(runProfileInPane).mockImplementation(async () => {
      runCount++;
      if (runCount === 2) {
        throw new Error("write failed");
      }
    });

    const result = await applyLayoutToSession(session, layout);

    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.warnings).toHaveLength(1);
      expect(result.warnings[0]).toContain("p2");
      expect(result.warnings[0]).toContain("Center");
    }

    // All 3 panes exist
    expect(get(paneInstances).size).toBe(3);

    // Layout tree written
    const tree = get(sessionLayouts).get("s7");
    expect(tree).toBeDefined();
    expect(tree!.kind).toBe("split");
  });

  // ── Test 8: Size normalization ───────────────────────────────────────────

  it("normalizes sizes correctly", async () => {
    const p = stubProfile("p");
    setUserProfiles([p]);

    // Helper to apply a layout and extract the root's sizes
    async function applySizes(
      children: LayoutPaneNode[],
    ): Promise<number[] | undefined> {
      resetInstances();
      resetLayouts();
      resetFocus();
      vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
      vi.mocked(runProfileInPane).mockReset().mockResolvedValue(undefined);
      vi.mocked(initTerminal).mockReset();
      vi.mocked(attachPtyListeners).mockReset().mockResolvedValue(undefined);

      const session = stubSession(`sz-${Math.random()}`);
      const layout = layoutSpec(splitNode("horizontal", children));
      await applyLayoutToSession(session, layout);
      const tree = get(sessionLayouts).get(session.id)!;
      return tree.kind === "split" ? tree.sizes : undefined;
    }

    // [size=60, size=40] → [0.6, 0.4]
    expect(
      await applySizes([leafNode("p", { size: 60 }), leafNode("p", { size: 40 })]),
    ).toEqual([0.6, 0.4]);

    // [size=1, size=1] → [0.5, 0.5]
    expect(
      await applySizes([leafNode("p", { size: 1 }), leafNode("p", { size: 1 })]),
    ).toEqual([0.5, 0.5]);

    // [no size, no size] → undefined
    expect(await applySizes([leafNode("p"), leafNode("p")])).toBeUndefined();

    // [size=50, no size] → [1.0, 0.0]
    // When some children have sizes and others don't, missing sizes are
    // treated as 0. The explicitly-sized children get their full proportion.
    expect(
      await applySizes([leafNode("p", { size: 50 }), leafNode("p")]),
    ).toEqual([1.0, 0.0]);
  });

  // ── Test 9: Round-trip with claude_shell built-in layout ─────────────────

  it("round-trip: applies the claude_shell built-in layout", async () => {
    const claude = stubProfile("claude", { startupCommand: "claude" });
    const shell = stubProfile("plain-shell");
    setUserProfiles([claude, shell]);

    const session = stubSession("s9");

    // Construct a LayoutSpec matching claude_shell.kdl:
    //   pane split_direction="horizontal" {
    //       pane profile="claude"      size=60
    //       pane profile="plain-shell" size=40
    //   }
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("claude", { size: 60 }),
        leafNode("plain-shell", { size: 40 }),
      ]),
      "claude_shell",
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    // Final tree: h-split with sizes [0.6, 0.4]
    const tree = get(sessionLayouts).get("s9")!;
    expect(tree.kind).toBe("split");
    if (tree.kind === "split") {
      expect(tree.direction).toBe("h");
      expect(tree.sizes).toEqual([0.6, 0.4]);
      expect(tree.children).toHaveLength(2);
      expect(tree.children[0]).toEqual({
        kind: "leaf",
        paneId: "s9-main",
      });
      // Second leaf has a UUID paneId — just verify shape
      expect(tree.children[1].kind).toBe("leaf");
    }

    // First leaf reuses session PTY → only 1 spawnShell call (for leaf 2)
    expect(spawnShell).toHaveBeenCalledTimes(1);

    // Both leaves get profile commands
    expect(runProfileInPane).toHaveBeenCalledTimes(2);
    expect(runProfileInPane).toHaveBeenCalledWith("s9", claude, {
      smolMachineName: null,
    });
  });

  // ── Nono config tests ─────────────────────────────────────────────────────

  it("passes nono config from layout leaf to spawnShell", async () => {
    const claude = stubProfile("claude", { startupCommand: "claude" });
    setUserProfiles([claude]);

    const session = stubSession("sn1");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("claude"),
        leafNode("claude", { nono_profile: "default" }),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    // spawnShell called once for the second leaf
    expect(spawnShell).toHaveBeenCalledTimes(1);
    expect(spawnShell).toHaveBeenCalledWith(
      expect.any(String),       // ptyId (UUID)
      "/tmp/repo",              // worktreePath
      "sn1",                    // sessionId
      expect.any(String),       // paneId (UUID)
      "default",                // nonoProfile
      undefined,                // nonoAllowDirs
      "claude",                 // profile
    );

    // Pane instance for second leaf has nonoProfile
    const instances = get(paneInstances);
    const secondLeaf = Array.from(instances.values()).find(
      (p) => p.nonoProfile === "default",
    );
    expect(secondLeaf).toBeDefined();
    expect(secondLeaf!.nonoProfile).toBe("default");
  });

  it("uses nono from SpawnProfile when layout leaf has none", async () => {
    const p = stubProfile("p", { nonoProfile: "from-profile", nonoAllowDirs: ["/b"] });
    setUserProfiles([p]);

    const session = stubSession("sn2");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("p"),
        leafNode("p"),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    // spawnShell called for second leaf with profile nono
    expect(spawnShell).toHaveBeenCalledTimes(1);
    expect(spawnShell).toHaveBeenCalledWith(
      expect.any(String),
      "/tmp/repo",
      "sn2",
      expect.any(String),
      "from-profile",
      ["/b"],
      "p",                      // profile
    );
  });

  it("layout nono overrides profile nono", async () => {
    const p = stubProfile("p", { nonoProfile: "profile-default" });
    setUserProfiles([p]);

    const session = stubSession("sn3");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("p"),
        leafNode("p", { nono_profile: "leaf-override" }),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    expect(spawnShell).toHaveBeenCalledTimes(1);
    expect(spawnShell).toHaveBeenCalledWith(
      expect.any(String),
      "/tmp/repo",
      "sn3",
      expect.any(String),
      "leaf-override",
      undefined,
      "p",                      // profile
    );
  });

  it("merges allow_dirs from layout leaf and profile", async () => {
    const p = stubProfile("p", {
      nonoProfile: "merged",
      nonoAllowDirs: ["/b"],
    });
    setUserProfiles([p]);

    const session = stubSession("sn4");
    const layout = layoutSpec(
      splitNode("horizontal", [
        leafNode("p"),
        leafNode("p", { nono_profile: "merged", nono_allow_dirs: ["/a"] }),
      ]),
    );

    const result = await applyLayoutToSession(session, layout);
    expect(result.ok).toBe(true);

    expect(spawnShell).toHaveBeenCalledTimes(1);
    const callArgs = vi.mocked(spawnShell).mock.calls[0];
    expect(callArgs[4]).toBe("merged");
    // Allow dirs should contain both "/a" (from leaf) and "/b" (from profile)
    const dirs = callArgs[5] as string[];
    expect(dirs).toHaveLength(2);
    expect(dirs).toContain("/a");
    expect(dirs).toContain("/b");
  });
});
