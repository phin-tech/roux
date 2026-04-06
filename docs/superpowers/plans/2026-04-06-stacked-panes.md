# Stacked Panes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Zellij-style stacked panes so any split node can toggle into a tabbed view where children appear as collapsed title bars with only the active child expanded.

**Architecture:** Add `stacked?: boolean` and `activeIndex?: number` to the existing split variant of `SplitNode`. Rendering in `SplitPane.svelte` branches on `node.stacked` to show collapsed/expanded view. Navigation (`alt+j`/`alt+k`) becomes tab-cycling inside stacks. Toggle via `cmd+shift+s` with upward cycling.

**Tech Stack:** Svelte 5, TypeScript, Vitest, Tailwind CSS

**Spec:** `docs/superpowers/specs/2026-04-06-stacked-panes-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/lib/stores/panes.ts` | Modify | Add `stacked`/`activeIndex` to type, add `toggleStack`/`setActiveStackIndex`/`getStackLabel` functions, update `removePaneFromTree` to clamp `activeIndex`, update `navigatePane` for stack awareness |
| `src/lib/stores/__tests__/panes.test.ts` | Modify | Tests for toggle, activeIndex clamping, auto-unstack, navigation in stacks |
| `src/lib/stores/__tests__/panes-stack.test.ts` | Create | Dedicated test file for stacked pane behavior |
| `src/lib/components/SplitPane.svelte` | Modify | Add stacked rendering branch with collapsed title bars and expanded active child |
| `src/lib/commands/index.ts` | Modify | Register `pane.toggle-stack` command with `cmd+shift+s` |

---

### Task 1: Extend the SplitNode Type

**Files:**
- Modify: `src/lib/stores/panes.ts:15-17`

- [ ] **Step 1: Update the SplitNode type**

In `src/lib/stores/panes.ts`, change the split variant from:

```typescript
export type SplitNode =
  | { kind: "pane"; pane: Pane }
  | { kind: "split"; direction: SplitDirection; children: SplitNode[] };
```

to:

```typescript
export type SplitNode =
  | { kind: "pane"; pane: Pane }
  | { kind: "split"; direction: SplitDirection; children: SplitNode[];
      stacked?: boolean; activeIndex?: number };
```

- [ ] **Step 2: Run existing tests to verify nothing breaks**

Run: `npx vitest run src/lib/stores/__tests__/panes.test.ts`
Expected: All existing tests PASS (the new fields are optional, so no existing code is affected)

- [ ] **Step 3: Commit**

```bash
git add src/lib/stores/panes.ts
git commit -m "feat(panes): add stacked and activeIndex fields to SplitNode split variant"
```

---

### Task 2: Add toggleStack and setActiveStackIndex Store Functions

**Files:**
- Modify: `src/lib/stores/panes.ts`
- Create: `src/lib/stores/__tests__/panes-stack.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/lib/stores/__tests__/panes-stack.test.ts`:

```typescript
import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneTrees,
  focusedPaneId,
  initSessionPanes,
  addSplit,
  toggleStack,
  setActiveStackIndex,
  type SplitNode,
  type Pane,
} from "../panes";

function getTree(sessionId: string): SplitNode {
  return get(paneTrees).get(sessionId)!;
}

function asSplit(node: SplitNode) {
  if (node.kind !== "split") throw new Error("Expected split node");
  return node;
}

describe("stacked panes", () => {
  beforeEach(() => {
    paneTrees.set(new Map());
    focusedPaneId.set(null);
  });

  describe("toggleStack", () => {
    it("stacks the immediate parent split of the focused pane", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");

      toggleStack("s1");

      const tree = asSplit(getTree("s1"));
      expect(tree.stacked).toBe(true);
      expect(tree.activeIndex).toBe(0);
    });

    it("cycling: second press stacks the next ancestor split", () => {
      // Build: root horizontal split -> [claude, vertical split -> [shell-1, shell-2]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

      // Focus shell-1, toggle once -> stacks the vertical split (shell-1's parent)
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      const innerSplit = asSplit(asSplit(getTree("s1")).children[1]);
      expect(innerSplit.stacked).toBe(true);

      // Toggle again -> unstacks inner, stacks the root horizontal split
      toggleStack("s1");

      const root = asSplit(getTree("s1"));
      expect(root.stacked).toBe(true);
      // Inner should be unstacked
      const inner2 = asSplit(root.children[1]);
      expect(inner2.stacked).toBeFalsy();
    });

    it("cycling: third press unstacks everything", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("shell-1");

      toggleStack("s1"); // stack inner
      toggleStack("s1"); // stack outer
      toggleStack("s1"); // unstack all

      const root = asSplit(getTree("s1"));
      expect(root.stacked).toBeFalsy();
    });

    it("does nothing when focused pane has no parent split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");

      toggleStack("s1"); // root is a single pane, no split to stack

      const tree = getTree("s1");
      expect(tree.kind).toBe("pane");
    });

    it("sets activeIndex to the child containing the focused pane", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("shell-1");

      toggleStack("s1");

      const tree = asSplit(getTree("s1"));
      expect(tree.stacked).toBe(true);
      // shell-1 is child index 1 (claude is 0, shell-1 is 1, shell-2 is 2)
      expect(tree.activeIndex).toBe(1);
    });
  });

  describe("setActiveStackIndex", () => {
    it("sets the active index on the nearest stacked ancestor", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 1);

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
    });

    it("clamps index to valid range", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 99);

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1); // clamped to last valid index
    });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: FAIL — `toggleStack` and `setActiveStackIndex` not exported

- [ ] **Step 3: Implement toggleStack**

Add to `src/lib/stores/panes.ts`, after the `renamePane` function (after line 183):

```typescript
// ── Stacked panes ──────────────────────────────────────────

/** Build the path of parent split nodes from root to the node containing targetId. */
function buildSplitPath(
  node: SplitNode,
  targetId: string,
  path: (SplitNode & { kind: "split" })[]
): boolean {
  if (node.kind === "pane") return node.pane.id === targetId;
  for (const child of node.children) {
    path.push(node);
    if (buildSplitPath(child, targetId, path)) return true;
    path.pop();
  }
  return false;
}

/** Find which child index of a split contains the given pane ID. */
function childIndexContaining(split: SplitNode & { kind: "split" }, paneId: string): number {
  for (let i = 0; i < split.children.length; i++) {
    if (findPaneInTree(split.children[i], paneId)) return i;
  }
  return 0;
}

/** Toggle stacking on the tree, cycling through ancestor splits. */
function toggleStackInTree(root: SplitNode, focusedId: string): SplitNode {
  const splitPath: (SplitNode & { kind: "split" })[] = [];
  if (!buildSplitPath(root, focusedId, splitPath)) return root;
  if (splitPath.length === 0) return root; // focused pane is the root, no split to stack

  // Find the lowest stacked ancestor (if any)
  let lowestStackedIdx = -1;
  for (let i = splitPath.length - 1; i >= 0; i--) {
    if (splitPath[i].stacked) {
      lowestStackedIdx = i;
      break;
    }
  }

  if (lowestStackedIdx === -1) {
    // Nothing stacked yet — stack the immediate parent
    const target = splitPath[splitPath.length - 1];
    return setStackedOnNode(root, target, true, childIndexContaining(target, focusedId));
  } else if (lowestStackedIdx > 0) {
    // Something is stacked but there's a higher ancestor — unstack current, stack higher
    let result = setStackedOnNode(root, splitPath[lowestStackedIdx], false, undefined);
    const higher = splitPath[lowestStackedIdx - 1];
    result = setStackedOnNode(result, higher, true, childIndexContaining(higher, focusedId));
    return result;
  } else {
    // Already stacked at root level — unstack everything
    return setStackedOnNode(root, splitPath[0], false, undefined);
  }
}

/** Set stacked/activeIndex on a specific split node in the tree (matched by reference). */
function setStackedOnNode(
  node: SplitNode,
  target: SplitNode & { kind: "split" },
  stacked: boolean,
  activeIndex: number | undefined
): SplitNode {
  if (node === target) {
    return { ...node, stacked: stacked || undefined, activeIndex: stacked ? (activeIndex ?? 0) : undefined };
  }
  if (node.kind === "pane") return node;
  const newChildren = node.children.map((c) => setStackedOnNode(c, target, stacked, activeIndex));
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}

export function toggleStack(sessionId: string) {
  const focused = get(focusedPaneId);
  if (!focused) return;

  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;
    trees.set(sessionId, toggleStackInTree(tree, focused));
    return new Map(trees);
  });
}

export function setActiveStackIndex(sessionId: string, index: number) {
  const focused = get(focusedPaneId);
  if (!focused) return;

  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;

    const splitPath: (SplitNode & { kind: "split" })[] = [];
    if (!buildSplitPath(tree, focused, splitPath)) return trees;

    // Find nearest stacked ancestor
    const stacked = [...splitPath].reverse().find((s) => s.stacked);
    if (!stacked) return trees;

    const clamped = Math.max(0, Math.min(index, stacked.children.length - 1));
    trees.set(sessionId, setStackedOnNode(tree, stacked, true, clamped));
    return new Map(trees);
  });
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/stores/panes.ts src/lib/stores/__tests__/panes-stack.test.ts
git commit -m "feat(panes): add toggleStack and setActiveStackIndex with cycling behavior"
```

---

### Task 3: Add getStackLabel Utility and Auto-Unstack on Remove

**Files:**
- Modify: `src/lib/stores/panes.ts`
- Modify: `src/lib/stores/__tests__/panes-stack.test.ts`

- [ ] **Step 1: Write the failing tests**

Add to `src/lib/stores/__tests__/panes-stack.test.ts`:

```typescript
import {
  // ... existing imports ...
  removePane,
  getStackLabel,
} from "../panes";

// Add these test blocks inside the existing describe("stacked panes"):

  describe("getStackLabel", () => {
    it("returns pane name for a leaf node", () => {
      const node: SplitNode = { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "", name: "my shell" } };
      expect(getStackLabel(node)).toBe("my shell");
    });

    it("returns pane type label when no name", () => {
      const node: SplitNode = { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "" } };
      expect(getStackLabel(node)).toBe("shell");
    });

    it("joins leaf names with | for splits", () => {
      const node: SplitNode = {
        kind: "split",
        direction: "horizontal",
        children: [
          { kind: "pane", pane: { id: "p1", type: "shell", ptyId: "", name: "P1" } },
          { kind: "pane", pane: { id: "p2", type: "claude", ptyId: "" } },
        ],
      };
      expect(getStackLabel(node)).toBe("P1 | claude");
    });
  });

  describe("auto-unstack on remove", () => {
    it("auto-unstacks when stack drops to 1 child", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      // Verify stacked
      expect(asSplit(getTree("s1")).stacked).toBe(true);

      // Remove one child — should collapse to single pane (no split at all)
      removePane("s1", "shell-1");

      const tree = getTree("s1");
      expect(tree.kind).toBe("pane");
    });

    it("clamps activeIndex when removing a child from a stacked split with 3+ children", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("shell-2");
      toggleStack("s1");

      // activeIndex should be 2 (shell-2)
      expect(asSplit(getTree("s1")).activeIndex).toBe(2);

      // Remove shell-2 — activeIndex should clamp to 1
      removePane("s1", "shell-2");

      const tree = asSplit(getTree("s1"));
      expect(tree.stacked).toBe(true);
      expect(tree.activeIndex).toBe(1);
      expect(tree.children).toHaveLength(2);
    });
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: FAIL — `getStackLabel` not exported, auto-unstack not implemented

- [ ] **Step 3: Implement getStackLabel**

Add to `src/lib/stores/panes.ts`:

```typescript
/** Generate a display label for a SplitNode (used for collapsed stack tabs). */
export function getStackLabel(node: SplitNode): string {
  if (node.kind === "pane") {
    return node.pane.name ?? node.pane.type;
  }
  const panes: Pane[] = [];
  collectPanes(node, panes);
  return panes.map((p) => p.name ?? p.type).join(" | ");
}
```

- [ ] **Step 4: Update removePaneFromTree to handle stacked splits**

In `src/lib/stores/panes.ts`, replace the existing `removePaneFromTree` function (lines 115-125):

```typescript
function removePaneFromTree(node: SplitNode, paneId: string): SplitNode | null {
  if (node.kind === "pane") {
    return node.pane.id === paneId ? null : node;
  }
  const remaining = node.children
    .map((c) => removePaneFromTree(c, paneId))
    .filter((c): c is SplitNode => c !== null);
  if (remaining.length === 0) return null;
  if (remaining.length === 1) {
    // Auto-unstack: collapsing to 1 child means the split dissolves
    return remaining[0];
  }
  // Clamp activeIndex if this is a stacked split
  const activeIndex = node.stacked
    ? Math.min(node.activeIndex ?? 0, remaining.length - 1)
    : node.activeIndex;
  return { ...node, children: remaining, activeIndex };
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: All PASS

- [ ] **Step 6: Run all pane tests to verify no regressions**

Run: `npx vitest run src/lib/stores/__tests__/panes.test.ts src/lib/stores/__tests__/panes-rename.test.ts src/lib/stores/__tests__/panes-move.test.ts src/lib/panes/__tests__/actions.test.ts`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src/lib/stores/panes.ts src/lib/stores/__tests__/panes-stack.test.ts
git commit -m "feat(panes): add getStackLabel, auto-unstack on remove, activeIndex clamping"
```

---

### Task 4: Update navigatePane for Stack Awareness

**Files:**
- Modify: `src/lib/stores/panes.ts`
- Modify: `src/lib/stores/__tests__/panes-stack.test.ts`

- [ ] **Step 1: Write the failing tests**

Add to `src/lib/stores/__tests__/panes-stack.test.ts`:

```typescript
import {
  // ... existing imports ...
  navigatePane,
} from "../panes";

// Add inside describe("stacked panes"):

  describe("navigation in stacks", () => {
    it("alt+j (down) cycles to next tab in a stacked split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      addSplit("s1", "horizontal", { id: "shell-2", type: "shell", ptyId: "pty-2" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      // activeIndex is 0 (s1-main). Navigate down should go to index 1
      navigatePane("s1", "down");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
      expect(get(focusedPaneId)).toBe("shell-1");
    });

    it("alt+k (up) cycles to previous tab in a stacked split", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("s1-main");
      toggleStack("s1");

      setActiveStackIndex("s1", 1);
      focusedPaneId.set("shell-1");

      navigatePane("s1", "up");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(0);
      expect(get(focusedPaneId)).toBe("s1-main");
    });

    it("does not cycle past the last tab", () => {
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      // Already at last tab (index 1), navigate down should do nothing
      navigatePane("s1", "down");

      const tree = asSplit(getTree("s1"));
      expect(tree.activeIndex).toBe(1);
    });

    it("alt+h/alt+l navigate out of the stack spatially", () => {
      // Build: root horizontal -> [claude pane, stacked vertical -> [shell-1, shell-2]]
      initSessionPanes("s1");
      focusedPaneId.set("s1-main");
      addSplit("s1", "horizontal", { id: "shell-1", type: "shell", ptyId: "pty-1" });
      focusedPaneId.set("shell-1");
      addSplit("s1", "vertical", { id: "shell-2", type: "shell", ptyId: "pty-2" });

      // Stack the vertical split (shell-1's parent)
      focusedPaneId.set("shell-1");
      toggleStack("s1");

      // Navigate left should go to the claude pane (spatial, not tab)
      navigatePane("s1", "left");
      expect(get(focusedPaneId)).toBe("s1-main");
    });
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: FAIL — navigation doesn't account for stacks yet

- [ ] **Step 3: Update navigatePane**

Replace the `navigatePane` function in `src/lib/stores/panes.ts` (lines 278-302):

```typescript
export function navigatePane(sessionId: string, direction: Direction) {
  const tree = get(paneTrees).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const path: PathEntry[] = [];
  if (!buildPath(tree, focused, path)) return;

  const axis = directionAxis[direction];
  const step = directionStep[direction];

  // Check for stacked ancestor — up/down navigates tabs within stacks
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (!parent.stacked) continue;

    // Only up/down cycles stack tabs (stacks are vertical visually)
    if (axis !== "vertical") continue;

    const nextIndex = (parent.activeIndex ?? 0) + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) return;

    // Update activeIndex and focus the first pane in the new active child
    paneTrees.update((trees) => {
      const root = trees.get(sessionId);
      if (!root) return trees;
      trees.set(sessionId, setStackedOnNode(root, parent, true, nextIndex));
      return new Map(trees);
    });
    const target = parent.children[nextIndex];
    focusedPaneId.set(firstPaneId(target));
    return;
  }

  // No stacked ancestor matched — fall through to normal spatial navigation
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;
    const nextIndex = childIndex + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) continue;
    const target = parent.children[nextIndex];
    const newFocus = step > 0 ? firstPaneId(target) : lastPaneId(target);
    focusedPaneId.set(newFocus);
    return;
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/lib/stores/__tests__/panes-stack.test.ts`
Expected: All PASS

- [ ] **Step 5: Run all pane tests**

Run: `npx vitest run src/lib/stores/__tests__/`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/lib/stores/panes.ts src/lib/stores/__tests__/panes-stack.test.ts
git commit -m "feat(panes): make navigatePane cycle tabs in stacked splits with alt+j/k"
```

---

### Task 5: Render Stacked Panes in SplitPane.svelte

**Files:**
- Modify: `src/lib/components/SplitPane.svelte`
- Modify: `src/lib/stores/panes.ts` (import `getStackLabel`)

- [ ] **Step 1: Add stacked rendering branch**

In `src/lib/components/SplitPane.svelte`, add the import for the new functions at line 7:

```typescript
import { focusedPaneId, renamePane, setActiveStackIndex, getStackLabel, type SplitNode } from "$lib/stores/panes";
```

Then replace the `{:else}` block (lines 131-141) with:

```svelte
{:else if node.stacked}
  <!-- Stacked view: collapsed title bars + one expanded child -->
  <div class="flex flex-col flex-1 min-h-0 min-w-0">
    {#each node.children as child, i}
      {#if i === (node.activeIndex ?? 0)}
        <!-- Active child: clickable title bar + full content -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="flex items-center h-7 shrink-0 select-none border-b border-hairline/50 px-2.5 gap-2 cursor-pointer bg-bg-surface/60"
          onclick={() => setActiveStackIndex(sessionId, i)}
        >
          <span class="text-[10px] text-text-muted/60 shrink-0">&#x25BE;</span>
          <span class="text-[11px] text-text-secondary font-mono truncate">{getStackLabel(child)}</span>
        </div>
        <div class="flex-1 min-h-0 min-w-0">
          <SplitPane node={child} {sessionId} {sessionActive} />
        </div>
      {:else}
        <!-- Collapsed child: just a title bar -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="flex items-center h-7 shrink-0 select-none border-b border-hairline/50 px-2.5 gap-2 cursor-pointer hover:bg-bg-surface/30 transition-colors"
          onclick={() => setActiveStackIndex(sessionId, i)}
        >
          <span class="text-[10px] text-text-muted/60 shrink-0">&#x25B8;</span>
          <span class="text-[11px] text-text-muted font-mono truncate">{getStackLabel(child)}</span>
        </div>
      {/if}
    {/each}
  </div>
{:else}
  <div
    class="flex flex-1 min-h-0 min-w-0 gap-1"
    class:flex-row={node.direction === "horizontal"}
    class:flex-col={node.direction === "vertical"}
  >
    {#each node.children as child}
      <SplitPane node={child} {sessionId} {sessionActive} />
    {/each}
  </div>
{/if}
```

- [ ] **Step 2: Update setActiveStackIndex to also move focus**

In `src/lib/stores/panes.ts`, update `setActiveStackIndex` to focus the first pane in the newly active child:

```typescript
export function setActiveStackIndex(sessionId: string, index: number) {
  const focused = get(focusedPaneId);
  if (!focused) return;

  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;

    const splitPath: (SplitNode & { kind: "split" })[] = [];
    if (!buildSplitPath(tree, focused, splitPath)) return trees;

    // Find nearest stacked ancestor
    const stacked = [...splitPath].reverse().find((s) => s.stacked);
    if (!stacked) return trees;

    const clamped = Math.max(0, Math.min(index, stacked.children.length - 1));
    const newTree = setStackedOnNode(tree, stacked, true, clamped);
    trees.set(sessionId, newTree);

    // Move focus to first pane in the new active child
    focusedPaneId.set(firstPaneId(stacked.children[clamped]));

    return new Map(trees);
  });
}
```

- [ ] **Step 3: Verify the app builds**

Run: `npx vite build 2>&1 | tail -5`
Expected: Build succeeds

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/SplitPane.svelte src/lib/stores/panes.ts
git commit -m "feat(panes): render stacked panes with collapsed title bars in SplitPane"
```

---

### Task 6: Register Toggle Stack Keybinding

**Files:**
- Modify: `src/lib/commands/index.ts`

- [ ] **Step 1: Add the toggle-stack command**

In `src/lib/commands/index.ts`, add the import for `toggleStack`:

```typescript
import { addSplit, initSessionPanes, navigatePane, renamePane, toggleStack } from "$lib/stores/panes";
```

Then add the command registration after the `pane.close` block (after line 114):

```typescript
  registry.register({
    id: "pane.toggle-stack",
    label: "Toggle Stack",
    shortcut: "cmd+shift+s",
    category: "Panes",
    available: () => queries.canSplitPane(),
    execute: () => {
      const activeId = queries.activeSessionId();
      if (activeId) toggleStack(activeId);
    },
  });
```

- [ ] **Step 2: Verify the app builds**

Run: `npx vite build 2>&1 | tail -5`
Expected: Build succeeds

- [ ] **Step 3: Run all tests**

Run: `npx vitest run`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/lib/commands/index.ts
git commit -m "feat(panes): register cmd+shift+s keybinding for toggle stack"
```

---

### Task 7: Manual Smoke Test

This task has no automated tests — it's a manual verification checklist.

- [ ] **Step 1: Start the dev server**

Run: `npm run tauri dev` (or equivalent)

- [ ] **Step 2: Verify basic stacking**

1. Open a session, split horizontally (`cmd+d`) twice to get 3 panes
2. Press `cmd+shift+s` — the parent split should stack, showing collapsed title bars
3. Click a collapsed title bar — it should expand and focus
4. Press `cmd+shift+s` again — should unstack back to normal layout

- [ ] **Step 3: Verify nested stacking cycle**

1. Create a nested layout: horizontal split, then vertical split inside one child
2. Focus a pane in the inner split, press `cmd+shift+s` — inner split stacks
3. Press again — outer split stacks, inner unstacks
4. Press again — everything unstacks

- [ ] **Step 4: Verify navigation**

1. Stack a split with 3 children
2. Use `alt+j`/`alt+k` to cycle through tabs
3. Use `alt+h`/`alt+l` to navigate out of the stack to adjacent panes

- [ ] **Step 5: Verify auto-unstack**

1. Stack a split with 2 children
2. Close one pane (`cmd+w`) — should auto-unstack to a single pane

- [ ] **Step 6: Commit final state**

If any fixes were needed during smoke testing, commit them:

```bash
git add -u
git commit -m "fix(panes): smoke test fixes for stacked panes"
```
