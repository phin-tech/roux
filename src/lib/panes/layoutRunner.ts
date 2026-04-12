import { get } from "svelte/store";
import type { LayoutSpec, LayoutPaneNode, Session } from "$lib/bindings";
import type { SpawnProfile, SpawnProfileRef } from "./profiles";
import { profileRegistry } from "./profiles";
import { createPane } from "./instances";
import { sessionLayouts, type LayoutNode, type SplitDirection } from "./layout";
import { setLogicalFocus } from "./focus";
import { spawnShell, killPty } from "$lib/tauri";
import { runProfileInPane } from "./profileRunner";

// ── Public types ────────────────────────────────────────────────────────────

export type LayoutApplyResult =
  | { ok: true; mainPaneId: string; warnings: string[] }
  | { ok: false; error: LayoutApplyError };

export type LayoutApplyError =
  | { kind: "missingProfile"; profileId: string; paneName?: string }
  | { kind: "spawnFailed"; paneName?: string; cause: string }
  | { kind: "empty" };

// ── Internal types ──────────────────────────────────────────────────────────

interface LeafInfo {
  paneId: string;
  ptyId: string;
  profile: SpawnProfile;
  spawnProfileRef: SpawnProfileRef;
  name: string | undefined;
  isFirst: boolean;
  nonoProfile: string | undefined;
  nonoAllowDirs: string[] | undefined;
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/**
 * Walk the layout tree in DFS order and collect all leaf nodes with their
 * resolved profiles, pane IDs, and PTY IDs.
 *
 * The first leaf reuses the session's primary PTY (`session.id`) and pane
 * ID (`${session.id}-main`). Every subsequent leaf gets fresh UUIDs.
 *
 * Returns null + error if a registered profile reference cannot be resolved.
 */
function collectLeaves(
  node: LayoutPaneNode,
  sessionId: string,
): { leaves: LeafInfo[] } | { error: LayoutApplyError } {
  const leaves: LeafInfo[] = [];
  const registry = get(profileRegistry);

  function walk(n: LayoutPaneNode): LayoutApplyError | null {
    if (n.kind === "leaf") {
      // Resolve the profile
      const ref = n.profile_ref;
      let profile: SpawnProfile;
      let spawnProfileRef: SpawnProfileRef;

      if (ref.kind === "registered") {
        const resolved = registry.get(ref.id);
        if (!resolved) {
          return {
            kind: "missingProfile",
            profileId: ref.id,
            paneName: n.name ?? undefined,
          };
        }
        profile = resolved;
        spawnProfileRef = { kind: "registered", id: ref.id };
      } else {
        // inline
        profile = ref.profile;
        spawnProfileRef = { kind: "inline", profile: ref.profile };
      }

      // Nono resolution: layout leaf wins, profile is fallback
      let nonoProfile: string | undefined;
      let nonoAllowDirs: string[] | undefined;

      const leafNono = n.nono_profile;
      const profileNono = profile.nonoProfile;

      if (leafNono) {
        nonoProfile = leafNono;
        // Merge allow_dirs: leaf dirs + profile dirs (union)
        const leafDirs = n.nono_allow_dirs ?? [];
        const profileDirs = profile.nonoAllowDirs ?? [];
        const merged = [...new Set([...leafDirs, ...profileDirs])];
        nonoAllowDirs = merged.length > 0 ? merged : undefined;
      } else if (profileNono) {
        nonoProfile = profileNono;
        nonoAllowDirs = profile.nonoAllowDirs?.length ? profile.nonoAllowDirs : undefined;
      }

      const isFirst = leaves.length === 0;
      const paneId = isFirst ? `${sessionId}-main` : crypto.randomUUID();
      const ptyId = isFirst ? sessionId : crypto.randomUUID();

      leaves.push({
        paneId,
        ptyId,
        profile,
        spawnProfileRef,
        name: n.name ?? undefined,
        isFirst,
        nonoProfile,
        nonoAllowDirs,
      });
      return null;
    }

    // Split — recurse into children
    for (const child of n.children) {
      const err = walk(child);
      if (err) return err;
    }
    return null;
  }

  const err = walk(node);
  if (err) return { error: err };
  return { leaves };
}

/**
 * Build the frontend `LayoutNode` tree from a `LayoutPaneNode`, substituting
 * leaf pane IDs from the collected leaves (consumed in DFS order via the
 * counter).
 */
function buildLayoutNode(
  node: LayoutPaneNode,
  leaves: LeafInfo[],
  counter: { i: number },
): LayoutNode {
  if (node.kind === "leaf") {
    const leaf = leaves[counter.i++];
    return { kind: "leaf", paneId: leaf.paneId };
  }

  const children = node.children.map((child) =>
    buildLayoutNode(child, leaves, counter),
  );
  const sizes = normalizeSizes(node.children);
  const direction: SplitDirection =
    node.direction === "horizontal" ? "h" : "v";

  const result: LayoutNode = { kind: "split", direction, children };
  if (sizes) result.sizes = sizes;
  return result;
}

/**
 * Normalize size values from layout children into proportional fractions.
 *
 * Rules:
 * - If ALL children have sizes: `total = sum`, then `sizes[i] = child.size / total`.
 * - If NO children have sizes: return undefined (let the renderer use equal splits).
 * - If SOME but not all have sizes: fill missing with 0, then normalize.
 *   This gives explicitly-sized children their share and shrinks unsized ones
 *   to zero — a sensible fallback since the author intentionally set sizes on
 *   some panes but not others.
 */
function normalizeSizes(children: LayoutPaneNode[]): number[] | undefined {
  const hasSome = children.some((c) => c.size != null);

  if (!hasSome) return undefined;

  const raw = children.map((c) => c.size ?? 0);
  const total = raw.reduce((a, b) => a + b, 0);

  if (total === 0) return undefined;

  // All have sizes, or mixed (missing filled with 0): normalize to fractions.
  return raw.map((s) => s / total);
}

// ── Main walker ─────────────────────────────────────────────────────────────

/**
 * Apply a layout to a session. Builds the entire pane tree in one shot:
 * spawns PTYs, creates pane instances, writes the LayoutNode tree, inits
 * terminals, and runs profiles.
 *
 * Atomic on failure: if any PTY spawn fails, already-spawned PTYs are
 * killed and no pane instances or layout entries are written.
 */
export async function applyLayoutToSession(
  session: Session,
  layout: LayoutSpec,
): Promise<LayoutApplyResult> {
  // Step 1+2: Walk the tree, resolve profiles, pre-validate
  const result = collectLeaves(node(layout), session.id);
  if ("error" in result) {
    return { ok: false, error: result.error };
  }

  const { leaves } = result;

  // Step 3: Guard against empty layout
  if (leaves.length === 0) {
    return { ok: false, error: { kind: "empty" } };
  }

  // Step 4: Spawn PTYs for non-first leaves
  const spawned: string[] = [];
  for (const leaf of leaves) {
    if (leaf.isFirst) continue;
    try {
      await spawnShell(leaf.ptyId, session.worktreePath, session.id, leaf.paneId, leaf.nonoProfile, leaf.nonoAllowDirs);
      spawned.push(leaf.ptyId);
    } catch (e) {
      // Step 5: Unwind on failure
      for (const ptyId of spawned) {
        try {
          await killPty(ptyId);
        } catch { /* best-effort cleanup */ }
      }
      return {
        ok: false,
        error: {
          kind: "spawnFailed",
          paneName: leaf.name ?? undefined,
          cause: String(e),
        },
      };
    }
  }

  // Step 6: Create pane instances
  for (const leaf of leaves) {
    createPane({
      id: leaf.paneId,
      type: "shell",
      ptyId: leaf.ptyId,
      name: leaf.name,
      spawnProfileRef: leaf.spawnProfileRef,
      nonoProfile: leaf.nonoProfile,
      nonoAllowDirs: leaf.nonoAllowDirs,
    });
  }

  // Step 7: Build the LayoutNode tree
  const tree = buildLayoutNode(node(layout), leaves, { i: 0 });

  // Step 8: Write the tree atomically
  sessionLayouts.update((m) => {
    const next = new Map(m);
    next.set(session.id, tree);
    return next;
  });

  // Warnings are collected from Step 9 and Step 10 — terminal init
  // failures and profile-run failures are both non-fatal.
  const warnings: string[] = [];

  // Step 9: Initialize terminals
  const { initTerminal, attachPtyListeners } = await import(
    "$lib/panes/terminals"
  );
  const { closePane } = await import("$lib/panes/actions");

  for (const leaf of leaves) {
    try {
      initTerminal(leaf.paneId);
      if (leaf.isFirst) {
        await attachPtyListeners(leaf.paneId);
      } else {
        await attachPtyListeners(leaf.paneId, () => {
          closePane(session.id, leaf.paneId);
        });
      }
    } catch (e) {
      const label = leaf.name ?? leaf.paneId;
      warnings.push(
        `Terminal init failed for pane '${label}': ${e}`,
      );
    }
  }

  // Step 10: Run profiles — collect failures as warnings
  for (const leaf of leaves) {
    try {
      await runProfileInPane(leaf.ptyId, leaf.profile);
    } catch (e) {
      const label = leaf.name ?? leaf.paneId;
      warnings.push(
        `Profile '${leaf.profile.id}' setup failed for pane '${label}': ${e}`,
      );
    }
  }

  // Step 11: Set focus to first leaf
  setLogicalFocus(leaves[0].paneId);

  // Step 12: Return success
  return { ok: true, mainPaneId: leaves[0].paneId, warnings };
}

/** Convenience accessor for the layout root. */
function node(layout: LayoutSpec): LayoutPaneNode {
  return layout.root;
}

/**
 * Resolve the effective nono (profile + allow_dirs) for the first leaf of a
 * layout. Used by the new-session dialog so the session's primary PTY —
 * which is spawned by `createSessionShell` BEFORE `applyLayoutToSession`
 * runs — actually honors nono set on the first leaf (or its profile).
 *
 * Mirrors the leaf-wins-over-profile + union-of-allow_dirs resolution used
 * by `collectLeaves`.
 */
export function resolveFirstLeafNono(
  layout: LayoutSpec,
): { nonoProfile: string | undefined; nonoAllowDirs: string[] | undefined } {
  function findFirstLeaf(n: LayoutPaneNode): LayoutPaneNode | null {
    if (n.kind === "leaf") return n;
    for (const c of n.children) {
      const r = findFirstLeaf(c);
      if (r) return r;
    }
    return null;
  }

  const leaf = findFirstLeaf(layout.root);
  if (!leaf || leaf.kind !== "leaf") {
    return { nonoProfile: undefined, nonoAllowDirs: undefined };
  }

  const registry = get(profileRegistry);
  let profile: SpawnProfile | null = null;
  const ref = leaf.profile_ref;
  if (ref.kind === "registered") {
    profile = registry.get(ref.id) ?? null;
  } else {
    profile = ref.profile;
  }

  const leafNono = leaf.nono_profile;
  const profileNono = profile?.nonoProfile ?? null;

  if (leafNono) {
    const leafDirs = leaf.nono_allow_dirs ?? [];
    const profileDirs = profile?.nonoAllowDirs ?? [];
    const merged = [...new Set([...leafDirs, ...profileDirs])];
    return {
      nonoProfile: leafNono,
      nonoAllowDirs: merged.length > 0 ? merged : undefined,
    };
  } else if (profileNono) {
    return {
      nonoProfile: profileNono,
      nonoAllowDirs: profile?.nonoAllowDirs?.length
        ? profile.nonoAllowDirs
        : undefined,
    };
  }
  return { nonoProfile: undefined, nonoAllowDirs: undefined };
}
