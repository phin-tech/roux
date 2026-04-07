# Pane Management Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate pane instances (model) from layout tree (view) so that Svelte component lifecycle never affects terminal state, eliminating focus/input bugs after split operations.

**Architecture:** Two independent stores: a reactive `paneInstances` map that owns terminal/PTY state with an explicit create/dispose lifecycle, and a `sessionLayouts` tree of pane ID references for spatial arrangement. Components are stateless renderers that attach/detach DOM elements based on visibility.

**Tech Stack:** Svelte 5 (runes), xterm.js, Tauri 2, Vitest

**Spec:** `docs/superpowers/specs/2026-04-07-pane-management-redesign.md`

---

## File Structure

### New files
- `src/lib/panes/instances.ts` — `PaneInstance` type, reactive `paneInstances` store, `createPane`, `disposePane`, `replacePty`, `attachToContainer`, `detachFromContainer`
- `src/lib/panes/focus.ts` — `focusedPaneId` store, `fullscreenPaneId` store, `setLogicalFocus`, `requestDomFocus`
- `src/lib/panes/layout.ts` — `LayoutNode` type, `sessionLayouts` store, all pure tree transforms (insert, remove, navigate, move, resize, stack, fullscreen helpers)
- `src/lib/panes/persistence.ts` — `PaneDescriptor` type, localStorage save/restore for layouts + descriptors
- `src/lib/panes/__tests__/instances.test.ts` — tests for pane instance lifecycle
- `src/lib/panes/__tests__/focus.test.ts` — tests for focus management
- `src/lib/panes/__tests__/layout.test.ts` — tests for layout tree transforms
- `src/lib/panes/__tests__/persistence.test.ts` — tests for layout persistence
- `src/lib/components/PaneShell.svelte` — thin component that renders a pane by looking up its instance

### Modified files
- `src/lib/components/SplitPane.svelte` — simplified to pure layout renderer using `LayoutNode` + `PaneShell`
- `src/lib/panes/actions.ts` — rewrite to use new layout/instances APIs
- `src/lib/commands/index.ts` — update imports to new modules
- `src/lib/queries/index.ts` — update imports to new modules
- `src/lib/stores/sessions.ts` — update imports (focusTick removed, use setLogicalFocus)
- `src/lib/sessions/close.ts` — use new `closeSessionPanes` from layout module
- `src/lib/sessions/reconnect.ts` — use `replacePty` from instances module
- `src/lib/tasks/runner.ts` — update imports to new modules

### Deleted files
- `src/lib/stores/panes.ts` — replaced by `panes/layout.ts`, `panes/instances.ts`, `panes/focus.ts`, `panes/persistence.ts`
- `src/lib/panes/terminalRegistry.ts` — replaced by `panes/instances.ts` + `panes/focus.ts`
- `src/lib/panes/commandPaneRegistry.ts` — command state moves into `PaneInstance`
- `src/lib/components/Terminal.svelte` — replaced by `PaneShell.svelte`
- `src/lib/components/ShellTerminal.svelte` — replaced by `PaneShell.svelte`

### Deleted test files (rewritten)
- `src/lib/stores/__tests__/panes.test.ts` → `src/lib/panes/__tests__/layout.test.ts`
- `src/lib/stores/__tests__/panes-stack.test.ts` → merged into `layout.test.ts`
- `src/lib/stores/__tests__/panes-move.test.ts` → merged into `layout.test.ts`
- `src/lib/stores/__tests__/panes-move-direction.test.ts` → merged into `layout.test.ts`
- `src/lib/stores/__tests__/panes-fullscreen-resize.test.ts` → merged into `layout.test.ts`
- `src/lib/stores/__tests__/panes-rename.test.ts` → `src/lib/panes/__tests__/instances.test.ts`
- `src/lib/panes/__tests__/actions.test.ts` → rewritten for new API

---

## Task 1: Pane Instance Store (`instances.ts`)

**Files:**
- Create: `src/lib/panes/instances.ts`
- Test: `src/lib/panes/__tests__/instances.test.ts`

- [ ] **Step 1: Write failing tests for createPane and disposePane**

```typescript
// src/lib/panes/__tests__/instances.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  paneInstances,
  createPane,
  disposePane,
  resetInstances,
} from "../instances";

describe("pane instances", () => {
  beforeEach(() => {
    resetInstances();
  });

  it("createPane adds an instance to the store", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    const instances = get(paneInstances);
    expect(instances.has(id)).toBe(true);
    const inst = instances.get(id)!;
    expect(inst.type).toBe("shell");
    expect(inst.ptyId).toBe("pty-1");
  });

  it("createPane accepts an explicit id", () => {
    const id = createPane({ id: "my-pane", type: "claude", ptyId: "s1" });
    expect(id).toBe("my-pane");
    expect(get(paneInstances).has("my-pane")).toBe(true);
  });

  it("createPane stores optional metadata", () => {
    const id = createPane({
      type: "command",
      ptyId: "pty-2",
      name: "test cmd",
      command: "npm test",
      workingDir: "/tmp",
    });
    const inst = get(paneInstances).get(id)!;
    expect(inst.name).toBe("test cmd");
    expect(inst.command).toBe("npm test");
    expect(inst.workingDir).toBe("/tmp");
  });

  it("createPane for markdown has no terminal", () => {
    const id = createPane({ type: "markdown", ptyId: "", docPath: "/tmp/a.md" });
    const inst = get(paneInstances).get(id)!;
    expect(inst.terminal).toBeNull();
    expect(inst.fitAddon).toBeNull();
  });

  it("disposePane removes the instance", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    disposePane(id);
    expect(get(paneInstances).has(id)).toBe(false);
  });

  it("disposePane is idempotent", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    disposePane(id);
    disposePane(id); // no error
    expect(get(paneInstances).has(id)).toBe(false);
  });

  it("disposePane cleans up unlisteners", () => {
    const id = createPane({ type: "shell", ptyId: "pty-1" });
    let cleaned = false;
    const inst = get(paneInstances).get(id)!;
    inst.unlisteners.push(() => { cleaned = true; });
    disposePane(id);
    expect(cleaned).toBe(true);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/instances.test.ts`
Expected: FAIL — module `../instances` does not exist

- [ ] **Step 3: Implement instances.ts**

```typescript
// src/lib/panes/instances.ts
import { writable, get } from "svelte/store";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { Channel } from "@tauri-apps/api/core";
import type { PtyOutputPayload } from "$lib/tauri";

export interface PaneInstance {
  id: string;
  type: "claude" | "shell" | "command" | "markdown";
  ptyId: string;
  terminal: Terminal | null;
  fitAddon: FitAddon | null;
  outputChannel: Channel<PtyOutputPayload> | null;
  unlisteners: UnlistenFn[];
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
  // Command pane runtime state
  commandStatus?: "running" | "succeeded" | "failed";
  commandExitCode?: number | null;
  commandStartedAt?: number;
  elapsedTimer?: ReturnType<typeof setInterval> | null;
}

export const paneInstances = writable<Map<string, PaneInstance>>(new Map());

export interface CreatePaneOpts {
  id?: string;
  type: "claude" | "shell" | "command" | "markdown";
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
}

export function createPane(opts: CreatePaneOpts): string {
  const id = opts.id ?? crypto.randomUUID();
  // Terminal creation is deferred to avoid importing xterm in tests.
  // The real terminal is created by initTerminal() after createPane().
  const instance: PaneInstance = {
    id,
    type: opts.type,
    ptyId: opts.ptyId,
    terminal: null,
    fitAddon: null,
    outputChannel: null,
    unlisteners: [],
    name: opts.name,
    workingDir: opts.workingDir,
    command: opts.command,
    docPath: opts.docPath,
  };
  paneInstances.update((map) => {
    map.set(id, instance);
    return new Map(map);
  });
  return id;
}

export function disposePane(id: string) {
  const map = get(paneInstances);
  const instance = map.get(id);
  if (!instance) return;
  for (const unlisten of instance.unlisteners.splice(0)) unlisten();
  if (instance.elapsedTimer) clearInterval(instance.elapsedTimer);
  instance.terminal?.dispose();
  paneInstances.update((m) => {
    m.delete(id);
    return new Map(m);
  });
}

export function replacePty(paneId: string, newPtyId: string) {
  paneInstances.update((map) => {
    const instance = map.get(paneId);
    if (!instance) return map;
    for (const unlisten of instance.unlisteners.splice(0)) unlisten();
    instance.outputChannel = null;
    instance.ptyId = newPtyId;
    return new Map(map);
  });
}

export function updateInstance(paneId: string, fields: Partial<PaneInstance>) {
  paneInstances.update((map) => {
    const instance = map.get(paneId);
    if (!instance) return map;
    Object.assign(instance, fields);
    return new Map(map);
  });
}

export function getInstance(paneId: string): PaneInstance | undefined {
  return get(paneInstances).get(paneId);
}

/** Attach a terminal's DOM element to a container. Purely visual. */
export function attachToContainer(paneId: string, container: HTMLDivElement) {
  const instance = get(paneInstances).get(paneId);
  if (!instance?.terminal) return;
  if (!instance.terminal.element) {
    // Suppress WebKit focus theft during initial open
    const origFocus = HTMLTextAreaElement.prototype.focus;
    HTMLTextAreaElement.prototype.focus = function () {};
    try {
      instance.terminal.open(container);
    } finally {
      HTMLTextAreaElement.prototype.focus = origFocus;
    }
  } else if (!container.contains(instance.terminal.element)) {
    container.appendChild(instance.terminal.element);
  }
  instance.fitAddon?.fit();
}

/** Remove a terminal's DOM element from its container. */
export function detachFromContainer(paneId: string) {
  const instance = get(paneInstances).get(paneId);
  if (instance?.terminal?.element?.parentElement) {
    instance.terminal.element.remove();
  }
}

/** Reset store — for tests only. */
export function resetInstances() {
  paneInstances.set(new Map());
}
```

Note: The `Terminal` and `FitAddon` types are referenced but terminal creation is deferred — tests don't import xterm. The actual terminal + addon creation will happen in `initTerminal()` which is called by the command/session layer after `createPane()`. We'll add that in Task 5 when wiring things up.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/instances.test.ts`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/instances.ts src/lib/panes/__tests__/instances.test.ts
git commit -m "feat(panes): add pane instance store with create/dispose/replacePty"
```

---

## Task 2: Focus Store (`focus.ts`)

**Files:**
- Create: `src/lib/panes/focus.ts`
- Test: `src/lib/panes/__tests__/focus.test.ts`

- [ ] **Step 1: Write failing tests for focus management**

```typescript
// src/lib/panes/__tests__/focus.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  focusedPaneId,
  fullscreenPaneId,
  setLogicalFocus,
  toggleFullscreen,
  resetFocus,
} from "../focus";
import { paneInstances, createPane, resetInstances } from "../instances";

describe("focus", () => {
  beforeEach(() => {
    resetInstances();
    resetFocus();
  });

  it("setLogicalFocus updates focusedPaneId", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");
    expect(get(focusedPaneId)).toBe("p1");
  });

  it("setLogicalFocus(null) clears focus", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");
    setLogicalFocus(null);
    expect(get(focusedPaneId)).toBeNull();
  });

  it("toggleFullscreen sets and clears fullscreenPaneId", () => {
    createPane({ id: "p1", type: "shell", ptyId: "pty-1" });
    setLogicalFocus("p1");

    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBe("p1");

    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBeNull();
  });

  it("toggleFullscreen does nothing without focus", () => {
    toggleFullscreen();
    expect(get(fullscreenPaneId)).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/focus.test.ts`
Expected: FAIL — module `../focus` does not exist

- [ ] **Step 3: Implement focus.ts**

```typescript
// src/lib/panes/focus.ts
import { writable, get } from "svelte/store";
import { paneInstances } from "./instances";

export const focusedPaneId = writable<string | null>(null);
export const fullscreenPaneId = writable<string | null>(null);

/**
 * Set logical focus: which pane owns keyboard input.
 * Updates disableStdin on all terminals. Does NOT call terminal.focus() —
 * that must only happen from pointer event handlers via requestDomFocus().
 */
export function setLogicalFocus(paneId: string | null) {
  focusedPaneId.set(paneId);
  const instances = get(paneInstances);
  for (const [id, instance] of instances) {
    if (!instance.terminal) continue;
    instance.terminal.options.disableStdin = id !== paneId;
  }
}

/**
 * Request DOM focus on a pane's terminal.
 * Only call from pointer event handlers (mousedown, click).
 */
export function requestDomFocus(paneId: string) {
  const instances = get(paneInstances);
  instances.get(paneId)?.terminal?.focus();
}

/** Toggle fullscreen for the focused pane. */
export function toggleFullscreen() {
  const focused = get(focusedPaneId);
  if (!focused) return;
  const current = get(fullscreenPaneId);
  fullscreenPaneId.set(current === focused ? null : focused);
}

/** Reset stores — for tests only. */
export function resetFocus() {
  focusedPaneId.set(null);
  fullscreenPaneId.set(null);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/focus.test.ts`
Expected: All 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/focus.ts src/lib/panes/__tests__/focus.test.ts
git commit -m "feat(panes): add focus store with logical/DOM focus split"
```

---

## Task 3: Layout Tree Store (`layout.ts`)

The largest task — all pure tree transforms. No terminal or PTY logic.

**Files:**
- Create: `src/lib/panes/layout.ts`
- Test: `src/lib/panes/__tests__/layout.test.ts`

- [ ] **Step 1: Write failing tests for LayoutNode types and basic operations**

```typescript
// src/lib/panes/__tests__/layout.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  removeLeaf,
  firstLeafId,
  collectLeafIds,
  hasSplitPanes,
  containsPaneId,
  resetLayouts,
  type LayoutNode,
} from "../layout";

function getLayout(sessionId: string): LayoutNode {
  return get(sessionLayouts).get(sessionId)!;
}

function treeShape(node: LayoutNode): any {
  if (node.kind === "leaf") return node.paneId;
  return { dir: node.direction, children: node.children.map(treeShape) };
}

describe("layout tree", () => {
  beforeEach(() => {
    resetLayouts();
  });

  describe("initSessionLayout", () => {
    it("creates a single leaf for a new session", () => {
      initSessionLayout("s1", "s1-main");
      const tree = getLayout("s1");
      expect(tree.kind).toBe("leaf");
      if (tree.kind === "leaf") expect(tree.paneId).toBe("s1-main");
    });

    it("does not reinitialize if already exists", () => {
      initSessionLayout("s1", "s1-main");
      const tree1 = getLayout("s1");
      initSessionLayout("s1", "s1-other");
      const tree2 = getLayout("s1");
      expect(tree1).toEqual(tree2);
    });
  });

  describe("insertLeaf", () => {
    it("splits a leaf into two children", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      const tree = getLayout("s1");
      expect(tree.kind).toBe("split");
      expect(treeShape(tree)).toEqual({
        dir: "h",
        children: ["s1-main", "shell-1"],
      });
    });

    it("flattens same-direction splits into siblings", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "h", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: ["s1-main", "shell-1", "shell-2"],
      });
    });

    it("nests different-direction splits", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "v", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: [
          "s1-main",
          { dir: "v", children: ["shell-1", "shell-2"] },
        ],
      });
    });
  });

  describe("removeLeaf", () => {
    it("collapses a split back to a single leaf", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1");
        t = removeLeaf(t, "shell-1")!;
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toBe("s1-main");
    });

    it("returns null when removing the only leaf", () => {
      const result = removeLeaf({ kind: "leaf", paneId: "p1" }, "p1");
      expect(result).toBeNull();
    });

    it("preserves other children when removing from a 3-child split", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "h", "shell-2");
        t = removeLeaf(t, "shell-1")!;
        m.set("s1", t);
        return new Map(m);
      });
      expect(treeShape(getLayout("s1"))).toEqual({
        dir: "h",
        children: ["s1-main", "shell-2"],
      });
    });
  });

  describe("helpers", () => {
    it("firstLeafId returns the leftmost leaf", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(firstLeafId(getLayout("s1"))).toBe("s1-main");
    });

    it("collectLeafIds returns all leaf IDs", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        let t = getLayout("s1");
        t = insertLeaf(t, "s1-main", "h", "shell-1");
        t = insertLeaf(t, "shell-1", "v", "shell-2");
        m.set("s1", t);
        return new Map(m);
      });
      expect(collectLeafIds(getLayout("s1")).sort()).toEqual(
        ["s1-main", "shell-1", "shell-2"].sort()
      );
    });

    it("hasSplitPanes returns false for single leaf", () => {
      initSessionLayout("s1", "s1-main");
      expect(hasSplitPanes("s1")).toBe(false);
    });

    it("hasSplitPanes returns true for split", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      expect(hasSplitPanes("s1")).toBe(true);
    });

    it("containsPaneId searches recursively", () => {
      initSessionLayout("s1", "s1-main");
      sessionLayouts.update((m) => {
        m.set("s1", insertLeaf(getLayout("s1"), "s1-main", "h", "shell-1"));
        return new Map(m);
      });
      const tree = getLayout("s1");
      expect(containsPaneId(tree, "shell-1")).toBe(true);
      expect(containsPaneId(tree, "nope")).toBe(false);
    });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: FAIL — module `../layout` does not exist

- [ ] **Step 3: Implement layout.ts — types, store, and basic operations**

```typescript
// src/lib/panes/layout.ts
import { writable, get } from "svelte/store";

export type SplitDirection = "h" | "v";

export type LayoutNode =
  | { kind: "leaf"; paneId: string }
  | {
      kind: "split";
      direction: SplitDirection;
      children: LayoutNode[];
      sizes?: number[];
      stacked?: boolean;
      activeIndex?: number;
    };

export const sessionLayouts = writable<Map<string, LayoutNode>>(new Map());

// ── Init ──────────────────────────────────────────────────

export function initSessionLayout(sessionId: string, mainPaneId: string) {
  sessionLayouts.update((m) => {
    if (m.has(sessionId)) return m;
    m.set(sessionId, { kind: "leaf", paneId: mainPaneId });
    return new Map(m);
  });
}

// ── Pure tree transforms ──────────────────────────────────

export function insertLeaf(
  node: LayoutNode,
  targetId: string | null,
  direction: SplitDirection,
  newPaneId: string
): LayoutNode {
  if (node.kind === "leaf") {
    if (!targetId || node.paneId === targetId) {
      return {
        kind: "split",
        direction,
        children: [node, { kind: "leaf", paneId: newPaneId }],
      };
    }
    return node;
  }
  // Flatten same-direction: if target is direct child, insert as sibling
  if (node.direction === direction) {
    const idx = node.children.findIndex(
      (c) => c.kind === "leaf" && (c.paneId === targetId || !targetId)
    );
    if (idx !== -1) {
      const children = [...node.children];
      children.splice(idx + 1, 0, { kind: "leaf", paneId: newPaneId });
      return { ...node, children };
    }
  }
  return {
    ...node,
    children: node.children.map((c) =>
      insertLeaf(c, targetId, direction, newPaneId)
    ),
  };
}

export function removeLeaf(
  node: LayoutNode,
  paneId: string
): LayoutNode | null {
  if (node.kind === "leaf") {
    return node.paneId === paneId ? null : node;
  }
  const mapped = node.children.map((c) => removeLeaf(c, paneId));
  const remaining = mapped.filter((c): c is LayoutNode => c !== null);
  if (remaining.length === 0) return null;
  if (remaining.length === 1) return remaining[0];
  // Clamp activeIndex for stacked splits
  const activeIndex = node.stacked
    ? Math.min(node.activeIndex ?? 0, remaining.length - 1)
    : node.activeIndex;
  // Adjust sizes
  let sizes = node.sizes;
  if (sizes && sizes.length === node.children.length) {
    const kept = sizes.filter((_, i) => mapped[i] !== null);
    const total = kept.reduce((a, b) => a + b, 0);
    sizes = total > 0 ? kept.map((s) => s / total) : undefined;
  }
  return { ...node, children: remaining, activeIndex, sizes };
}

// ── Helpers ───────────────────────────────────────────────

export function firstLeafId(node: LayoutNode): string {
  if (node.kind === "leaf") return node.paneId;
  return firstLeafId(node.children[0]);
}

export function lastLeafId(node: LayoutNode): string {
  if (node.kind === "leaf") return node.paneId;
  return lastLeafId(node.children[node.children.length - 1]);
}

export function collectLeafIds(node: LayoutNode): string[] {
  if (node.kind === "leaf") return [node.paneId];
  return node.children.flatMap(collectLeafIds);
}

export function containsPaneId(node: LayoutNode, paneId: string): boolean {
  if (node.kind === "leaf") return node.paneId === paneId;
  return node.children.some((c) => containsPaneId(c, paneId));
}

export function hasSplitPanes(sessionId: string): boolean {
  const tree = get(sessionLayouts).get(sessionId);
  if (!tree) return false;
  return tree.kind === "split";
}

/** Reset store — for tests only. */
export function resetLayouts() {
  sessionLayouts.set(new Map());
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/layout.ts src/lib/panes/__tests__/layout.test.ts
git commit -m "feat(panes): add layout tree store with insert/remove/helpers"
```

---

## Task 4: Layout Navigation, Move, Resize, Stack

Port the existing tree transform algorithms from `src/lib/stores/panes.ts` into `layout.ts`, operating on `LayoutNode` with pane IDs.

**Files:**
- Modify: `src/lib/panes/layout.ts`
- Modify: `src/lib/panes/__tests__/layout.test.ts`

- [ ] **Step 1: Write failing tests for navigate**

```typescript
// Append to src/lib/panes/__tests__/layout.test.ts

import {
  // ... existing imports ...
  navigatePane,
  getStackLabel,
  toggleStack,
  setActiveStackIndex,
  movePaneInDirection,
  movePane,
  resizePane,
  type LayoutNode,
  type Direction,
  type DropSide,
} from "../layout";
import { focusedPaneId, setLogicalFocus, resetFocus } from "../focus";
import { paneInstances, createPane, resetInstances } from "../instances";

// Add to beforeEach in the top-level describe:
// resetFocus();
// resetInstances();

describe("navigatePane", () => {
  beforeEach(() => {
    resetLayouts();
    resetFocus();
    resetInstances();
  });

  it("navigates right in a horizontal split", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p1");

    navigatePane("s1", "right");
    expect(get(focusedPaneId)).toBe("p2");
  });

  it("navigates left in a horizontal split", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p2");

    navigatePane("s1", "left");
    expect(get(focusedPaneId)).toBe("p1");
  });

  it("does nothing at the edge of a split", () => {
    initSessionLayout("s1", "p1");
    sessionLayouts.update((m) => {
      m.set("s1", insertLeaf(getLayout("s1"), "p1", "h", "p2"));
      return new Map(m);
    });
    setLogicalFocus("p2");

    navigatePane("s1", "right");
    expect(get(focusedPaneId)).toBe("p2");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: FAIL — `navigatePane` is not exported

- [ ] **Step 3: Implement navigation in layout.ts**

Add to `src/lib/panes/layout.ts`:

```typescript
import { focusedPaneId, setLogicalFocus } from "./focus";

export type Direction = "left" | "right" | "up" | "down";
export type DropSide = "left" | "right" | "top" | "bottom";

type PathEntry = {
  parent: LayoutNode & { kind: "split" };
  childIndex: number;
};

const directionAxis: Record<Direction, SplitDirection> = {
  left: "h", right: "h", up: "v", down: "v",
};
const directionStep: Record<Direction, number> = {
  left: -1, right: 1, up: -1, down: 1,
};

function buildPath(
  node: LayoutNode,
  targetId: string,
  path: PathEntry[]
): boolean {
  if (node.kind === "leaf") return node.paneId === targetId;
  for (let i = 0; i < node.children.length; i++) {
    path.push({ parent: node, childIndex: i });
    if (buildPath(node.children[i], targetId, path)) return true;
    path.pop();
  }
  return false;
}

export function navigatePane(sessionId: string, direction: Direction) {
  const tree = get(sessionLayouts).get(sessionId);
  if (!tree) return;
  const focused = get(focusedPaneId);
  if (!focused) return;

  const path: PathEntry[] = [];
  if (!buildPath(tree, focused, path)) return;

  const axis = directionAxis[direction];
  const step = directionStep[direction];

  // Check stacked ancestors first — up/down navigates tabs
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent } = path[i];
    if (!parent.stacked || axis !== "v") continue;
    const nextIndex = (parent.activeIndex ?? 0) + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) return;
    sessionLayouts.update((m) => {
      const root = m.get(sessionId);
      if (!root) return m;
      m.set(sessionId, updateSplitByRef(root, parent, { activeIndex: nextIndex }));
      return new Map(m);
    });
    setLogicalFocus(firstLeafId(parent.children[nextIndex]));
    return;
  }

  // Normal spatial navigation
  for (let i = path.length - 1; i >= 0; i--) {
    const { parent, childIndex } = path[i];
    if (parent.direction !== axis) continue;
    const nextIndex = childIndex + step;
    if (nextIndex < 0 || nextIndex >= parent.children.length) continue;
    const target = parent.children[nextIndex];
    const newFocus = step > 0 ? firstLeafId(target) : lastLeafId(target);
    setLogicalFocus(newFocus);
    return;
  }
}

function updateSplitByRef(
  node: LayoutNode,
  target: LayoutNode & { kind: "split" },
  patch: Partial<LayoutNode & { kind: "split" }>
): LayoutNode {
  if (node === target) return { ...node, ...patch } as LayoutNode;
  if (node.kind === "leaf") return node;
  const newChildren = node.children.map((c) =>
    updateSplitByRef(c, target, patch)
  );
  if (newChildren.every((c, i) => c === node.children[i])) return node;
  return { ...node, children: newChildren };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: All tests PASS

- [ ] **Step 5: Add and test stack operations**

Add tests for `toggleStack`, `setActiveStackIndex`, `getStackLabel` to `layout.test.ts`. Port the implementations from `src/lib/stores/panes.ts:352-511` into `layout.ts`, adapting from `SplitNode` (with embedded `Pane`) to `LayoutNode` (with `paneId` references). `getStackLabel` will need to look up pane names from `paneInstances`.

Port these functions:
- `buildSplitPath` — adapt to `LayoutNode`
- `splitAtPath` — adapt to `LayoutNode`
- `ancestorSplitDepths` — adapt to `LayoutNode`
- `childIndexContaining` — use `containsPaneId` with `paneId` instead of `findPaneInTree`
- `setStackedAtDepth` — adapt to `LayoutNode`
- `toggleStackInTree` — adapt to `LayoutNode`
- `toggleStack` — call `setLogicalFocus` instead of `refocusPane`
- `setActiveStackIndex` — call `setLogicalFocus` instead of `refocusPane`
- `getStackLabel` — look up names from `getInstance()` for leaf nodes

- [ ] **Step 6: Run tests, verify pass**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: All tests PASS

- [ ] **Step 7: Add and test move operations**

Add tests for `movePaneInDirection` and `movePane` (drag-and-drop). Port from `src/lib/stores/panes.ts:513-786` into `layout.ts`:

Port these functions:
- `movePaneInTree` — adapt to `LayoutNode` (uses `removeLeaf`, `containsPaneId`)
- `movePaneInDirection` — calls `setLogicalFocus`
- `movePane` (drag-and-drop) — adapt `insertPaneAtTarget` to `LayoutNode`
- `replaceSplitInTree` — adapt to `LayoutNode`

- [ ] **Step 8: Run tests, verify pass**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: All tests PASS

- [ ] **Step 9: Add and test resize**

Port `resizePane` from `src/lib/stores/panes.ts:808-852` into `layout.ts`. Uses same `buildPath` and `updateSplitByRef` helpers.

- [ ] **Step 10: Run all layout tests**

Run: `npm run test -- --run src/lib/panes/__tests__/layout.test.ts`
Expected: All tests PASS

- [ ] **Step 11: Commit**

```bash
git add src/lib/panes/layout.ts src/lib/panes/__tests__/layout.test.ts
git commit -m "feat(panes): add navigate, move, resize, stack to layout tree"
```

---

## Task 5: Layout Persistence (`persistence.ts`)

**Files:**
- Create: `src/lib/panes/persistence.ts`
- Test: `src/lib/panes/__tests__/persistence.test.ts`

- [ ] **Step 1: Write failing tests for save/restore**

```typescript
// src/lib/panes/__tests__/persistence.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  savePaneDescriptors,
  loadPaneDescriptors,
  saveLayout,
  loadLayout,
  stripCommandPanes,
  type PaneDescriptor,
} from "../persistence";
import type { LayoutNode } from "../layout";

// Mock localStorage
const storage = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, val: string) => storage.set(key, val),
  removeItem: (key: string) => storage.delete(key),
});

describe("persistence", () => {
  beforeEach(() => {
    storage.clear();
  });

  it("round-trips a layout tree", () => {
    const tree: LayoutNode = {
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        { kind: "leaf", paneId: "p2" },
      ],
    };
    saveLayout("s1", tree);
    expect(loadLayout("s1")).toEqual(tree);
  });

  it("returns null for unknown session", () => {
    expect(loadLayout("nope")).toBeNull();
  });

  it("round-trips pane descriptors", () => {
    const descs: PaneDescriptor[] = [
      { id: "p1", type: "claude", ptyId: "s1" },
      { id: "p2", type: "shell", ptyId: "pty-2", name: "test" },
    ];
    savePaneDescriptors("s1", descs);
    expect(loadPaneDescriptors("s1")).toEqual(descs);
  });

  it("stripCommandPanes removes command leaves", () => {
    const tree: LayoutNode = {
      kind: "split",
      direction: "h",
      children: [
        { kind: "leaf", paneId: "p1" },
        { kind: "leaf", paneId: "cmd-1" },
      ],
    };
    const descs: PaneDescriptor[] = [
      { id: "p1", type: "claude", ptyId: "s1" },
      { id: "cmd-1", type: "command", ptyId: "pty-cmd", command: "npm test" },
    ];
    const result = stripCommandPanes(tree, descs);
    expect(result.tree).toEqual({ kind: "leaf", paneId: "p1" });
    expect(result.descriptors).toEqual([descs[0]]);
  });

  it("stripCommandPanes returns null for all-command tree", () => {
    const tree: LayoutNode = { kind: "leaf", paneId: "cmd-1" };
    const descs: PaneDescriptor[] = [
      { id: "cmd-1", type: "command", ptyId: "pty-cmd", command: "npm test" },
    ];
    const result = stripCommandPanes(tree, descs);
    expect(result.tree).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/persistence.test.ts`
Expected: FAIL — module `../persistence` does not exist

- [ ] **Step 3: Implement persistence.ts**

```typescript
// src/lib/panes/persistence.ts
import type { LayoutNode } from "./layout";

export interface PaneDescriptor {
  id: string;
  type: "claude" | "shell" | "command" | "markdown";
  ptyId: string;
  name?: string;
  workingDir?: string;
  command?: string;
  docPath?: string;
}

const LAYOUT_KEY = "roux:pane-layouts-v2";
const DESCRIPTOR_KEY = "roux:pane-descriptors";

// ── Layout persistence ────────────────────────────────────

export function saveLayout(sessionId: string, tree: LayoutNode) {
  try {
    const all = loadAllLayouts();
    all[sessionId] = tree;
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {}
}

export function loadLayout(sessionId: string): LayoutNode | null {
  const all = loadAllLayouts();
  return all[sessionId] ?? null;
}

export function clearLayout(sessionId: string) {
  try {
    const all = loadAllLayouts();
    delete all[sessionId];
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {}
}

function loadAllLayouts(): Record<string, LayoutNode> {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

// ── Descriptor persistence ────────────────────────────────

export function savePaneDescriptors(
  sessionId: string,
  descriptors: PaneDescriptor[]
) {
  try {
    const all = loadAllDescriptors();
    all[sessionId] = descriptors;
    localStorage.setItem(DESCRIPTOR_KEY, JSON.stringify(all));
  } catch {}
}

export function loadPaneDescriptors(
  sessionId: string
): PaneDescriptor[] | null {
  const all = loadAllDescriptors();
  return all[sessionId] ?? null;
}

export function clearPaneDescriptors(sessionId: string) {
  try {
    const all = loadAllDescriptors();
    delete all[sessionId];
    localStorage.setItem(DESCRIPTOR_KEY, JSON.stringify(all));
  } catch {}
}

function loadAllDescriptors(): Record<string, PaneDescriptor[]> {
  try {
    const raw = localStorage.getItem(DESCRIPTOR_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

// ── Restore helpers ───────────────────────────────────────

/** Strip command panes from a tree + descriptors (their processes are gone). */
export function stripCommandPanes(
  tree: LayoutNode,
  descriptors: PaneDescriptor[]
): { tree: LayoutNode | null; descriptors: PaneDescriptor[] } {
  const commandIds = new Set(
    descriptors.filter((d) => d.type === "command").map((d) => d.id)
  );
  const stripped = stripLeaves(tree, commandIds);
  return {
    tree: stripped,
    descriptors: descriptors.filter((d) => d.type !== "command"),
  };
}

function stripLeaves(
  node: LayoutNode,
  removeIds: Set<string>
): LayoutNode | null {
  if (node.kind === "leaf") {
    return removeIds.has(node.paneId) ? null : node;
  }
  const remaining = node.children
    .map((c) => stripLeaves(c, removeIds))
    .filter((c): c is LayoutNode => c !== null);
  if (remaining.length === 0) return null;
  if (remaining.length === 1) return remaining[0];
  return { ...node, children: remaining };
}

// ── Debounced auto-save ───────────────────────────────────

let saveTimer: ReturnType<typeof setTimeout> | null = null;

export function scheduleSave(
  layouts: Map<string, LayoutNode>,
  getDescriptors: (sessionId: string) => PaneDescriptor[]
) {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    for (const [sessionId, tree] of layouts) {
      saveLayout(sessionId, tree);
      savePaneDescriptors(sessionId, getDescriptors(sessionId));
    }
  }, 500);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/persistence.test.ts`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/persistence.ts src/lib/panes/__tests__/persistence.test.ts
git commit -m "feat(panes): add layout and descriptor persistence"
```

---

## Task 6: High-Level Pane Actions (wire model + layout)

Rewrite `src/lib/panes/actions.ts` to orchestrate the new stores: create instance, insert into layout, manage focus, dispose on close.

**Files:**
- Rewrite: `src/lib/panes/actions.ts`
- Rewrite: `src/lib/panes/__tests__/actions.test.ts`

- [ ] **Step 1: Write failing tests**

```typescript
// src/lib/panes/__tests__/actions.test.ts
import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import {
  splitPane,
  closePane,
  closeSessionPanes,
  initSession,
} from "../actions";
import { paneInstances, resetInstances } from "../instances";
import { sessionLayouts, resetLayouts, collectLeafIds } from "../layout";
import { focusedPaneId, resetFocus } from "../focus";

describe("pane actions", () => {
  beforeEach(() => {
    resetInstances();
    resetLayouts();
    resetFocus();
  });

  describe("initSession", () => {
    it("creates a claude pane instance and layout", () => {
      initSession("s1");
      const tree = get(sessionLayouts).get("s1");
      expect(tree?.kind).toBe("leaf");
      if (tree?.kind === "leaf") {
        expect(get(paneInstances).has(tree.paneId)).toBe(true);
        const inst = get(paneInstances).get(tree.paneId)!;
        expect(inst.type).toBe("claude");
        expect(inst.ptyId).toBe("s1");
      }
    });
  });

  describe("splitPane", () => {
    it("creates a new pane and inserts into layout", () => {
      initSession("s1");
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const tree = get(sessionLayouts).get("s1")!;
      expect(tree.kind).toBe("split");
      const ids = collectLeafIds(tree);
      expect(ids).toHaveLength(2);
      // Both panes exist in instance store
      for (const id of ids) {
        expect(get(paneInstances).has(id)).toBe(true);
      }
    });

    it("focuses the new pane after split", () => {
      initSession("s1");
      const mainId = get(sessionLayouts).get("s1")!;
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const focused = get(focusedPaneId);
      expect(focused).not.toBeNull();
      // Focused pane should be the new shell, not the main claude
      const inst = get(paneInstances).get(focused!)!;
      expect(inst.type).toBe("shell");
    });
  });

  describe("closePane", () => {
    it("removes pane from layout and disposes instance", () => {
      initSession("s1");
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const tree = get(sessionLayouts).get("s1")!;
      const shellId = collectLeafIds(tree).find(
        (id) => get(paneInstances).get(id)?.type === "shell"
      )!;

      closePane("s1", shellId);

      expect(get(paneInstances).has(shellId)).toBe(false);
      const newTree = get(sessionLayouts).get("s1")!;
      expect(newTree.kind).toBe("leaf");
    });

    it("does not close the main claude pane", () => {
      initSession("s1");
      const mainId = collectLeafIds(get(sessionLayouts).get("s1")!)[0];

      const closed = closePane("s1", mainId);
      expect(closed).toBe(false);
      expect(get(paneInstances).has(mainId)).toBe(true);
    });

    it("moves focus when closing focused pane", () => {
      initSession("s1");
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const focused = get(focusedPaneId)!;
      closePane("s1", focused);

      expect(get(focusedPaneId)).not.toBeNull();
      expect(get(focusedPaneId)).not.toBe(focused);
    });
  });

  describe("closeSessionPanes", () => {
    it("disposes all panes and removes layout", () => {
      initSession("s1");
      splitPane("s1", "h", { type: "shell", ptyId: "pty-1" });

      const ids = collectLeafIds(get(sessionLayouts).get("s1")!);

      closeSessionPanes("s1");

      expect(get(sessionLayouts).has("s1")).toBe(false);
      for (const id of ids) {
        expect(get(paneInstances).has(id)).toBe(false);
      }
    });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test -- --run src/lib/panes/__tests__/actions.test.ts`
Expected: FAIL — `splitPane`, `initSession`, etc. not exported from `../actions`

- [ ] **Step 3: Implement new actions.ts**

```typescript
// src/lib/panes/actions.ts
import { get } from "svelte/store";
import {
  createPane,
  disposePane,
  getInstance,
  type CreatePaneOpts,
} from "./instances";
import {
  sessionLayouts,
  initSessionLayout,
  insertLeaf,
  removeLeaf,
  firstLeafId,
  collectLeafIds,
  type SplitDirection,
} from "./layout";
import { focusedPaneId, setLogicalFocus } from "./focus";

export function initSession(sessionId: string): string {
  const mainPaneId = createPane({
    id: `${sessionId}-main`,
    type: "claude",
    ptyId: sessionId,
  });
  initSessionLayout(sessionId, mainPaneId);
  setLogicalFocus(mainPaneId);
  return mainPaneId;
}

export function splitPane(
  sessionId: string,
  direction: SplitDirection,
  opts: Omit<CreatePaneOpts, "id">
): string | null {
  const newPaneId = createPane(opts);

  let inserted = false;
  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const focused = get(focusedPaneId);
    m.set(sessionId, insertLeaf(tree, focused, direction, newPaneId));
    inserted = true;
    return new Map(m);
  });

  if (!inserted) {
    disposePane(newPaneId);
    return null;
  }

  setLogicalFocus(newPaneId);
  return newPaneId;
}

export function closePane(sessionId: string, paneId: string): boolean {
  const instance = getInstance(paneId);
  if (!instance) return false;

  // Don't close the main claude pane
  if (instance.type === "claude" && instance.id === `${sessionId}-main`) {
    return false;
  }

  sessionLayouts.update((m) => {
    const tree = m.get(sessionId);
    if (!tree) return m;
    const result = removeLeaf(tree, paneId);
    if (result) m.set(sessionId, result);
    else m.delete(sessionId);
    return new Map(m);
  });

  disposePane(paneId);

  if (get(focusedPaneId) === paneId) {
    const tree = get(sessionLayouts).get(sessionId);
    setLogicalFocus(tree ? firstLeafId(tree) : null);
  }

  return true;
}

export function closeFocusedPane(sessionId: string): boolean {
  const paneId = get(focusedPaneId);
  if (!paneId) return false;
  return closePane(sessionId, paneId);
}

export function closeSessionPanes(sessionId: string) {
  const tree = get(sessionLayouts).get(sessionId);
  if (tree) {
    const paneIds = collectLeafIds(tree);
    for (const id of paneIds) disposePane(id);
  }
  sessionLayouts.update((m) => {
    m.delete(sessionId);
    return new Map(m);
  });
  const focused = get(focusedPaneId);
  if (focused && tree && collectLeafIds(tree).includes(focused)) {
    setLogicalFocus(null);
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test -- --run src/lib/panes/__tests__/actions.test.ts`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/actions.ts src/lib/panes/__tests__/actions.test.ts
git commit -m "feat(panes): high-level pane actions wiring model + layout"
```

---

## Task 7: PaneShell Component

**Files:**
- Create: `src/lib/components/PaneShell.svelte`

- [ ] **Step 1: Create PaneShell.svelte**

```svelte
<!-- src/lib/components/PaneShell.svelte -->
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    paneInstances,
    attachToContainer,
    detachFromContainer,
    getInstance,
    updateInstance,
  } from "$lib/panes/instances";
  import {
    focusedPaneId,
    setLogicalFocus,
    requestDomFocus,
  } from "$lib/panes/focus";
  import { closePane } from "$lib/panes/actions";
  import { createResizeScheduler } from "$lib/panes/resizeScheduler";
  import { resizeSession } from "$lib/tauri";
  import { sessionState } from "$lib/stores/sessions";
  import { log } from "$lib/logging";
  import SessionPicker from "./SessionPicker.svelte";
  import LazyMarkdownPane from "./LazyMarkdownPane.svelte";

  interface Props {
    paneId: string;
    sessionId: string;
    visible?: boolean;
  }

  let { paneId, sessionId, visible = true }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let resizeObserver: ResizeObserver | null = null;

  const instance = $derived($paneInstances.get(paneId));
  const isFocused = $derived($focusedPaneId === paneId);
  const session = $derived(
    $sessionState.sessions.find((s) => s.id === sessionId)
  );
  const isDisconnected = $derived(session?.status === "disconnected");

  const resizeScheduler = createResizeScheduler({
    getFitAddon: () => instance?.fitAddon ?? null,
    getPtyId: () => instance?.ptyId ?? "",
    onResize: (ptyId, cols, rows) => {
      resizeSession(ptyId, cols, rows).catch((e) => {
        log(`Resize failed for ${ptyId}: ${e}`);
      });
    },
  });

  let editingName = $state(false);
  let nameInput = $state("");

  function startRenaming(currentName: string) {
    nameInput = currentName;
    editingName = true;
  }

  function commitRename() {
    updateInstance(paneId, { name: nameInput.trim() || undefined });
    editingName = false;
  }

  function handleMouseDown() {
    setLogicalFocus(paneId);
    requestDomFocus(paneId);
  }

  function paneTypeLabel(type: string): string {
    switch (type) {
      case "claude": return "claude";
      case "shell": return "shell";
      case "markdown": return "doc";
      case "command": return "cmd";
      default: return type;
    }
  }

  function canClose(): boolean {
    if (!instance) return false;
    return !(instance.type === "claude" && instance.id === `${sessionId}-main`);
  }

  onMount(() => {
    if (visible && containerEl) {
      attachToContainer(paneId, containerEl);
    }
    if (containerEl) {
      resizeObserver = new ResizeObserver(() => {
        if (visible && instance?.fitAddon) {
          resizeScheduler.schedule();
        }
      });
      resizeObserver.observe(containerEl);
    }
  });

  onDestroy(() => {
    resizeScheduler.cancel();
    resizeObserver?.disconnect();
    detachFromContainer(paneId);
  });

  // Visibility-driven attach/detach
  $effect(() => {
    if (!containerEl) return;
    if (visible) {
      attachToContainer(paneId, containerEl);
      resizeScheduler.schedule();
    } else {
      detachFromContainer(paneId);
    }
  });

  // Refit on focus change
  $effect(() => {
    if (isFocused && visible && instance?.fitAddon) {
      resizeScheduler.schedule();
    }
  });
</script>

{#if instance}
  <div
    class="relative flex flex-col flex-1 min-h-0 min-w-0 overflow-hidden rounded-lg transition-colors {isFocused ? 'bg-bg-surface/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]' : 'bg-bg-deep shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]'}"
  >
    <!-- Title bar -->
    <div
      class="flex h-7 shrink-0 select-none items-center border-b border-hairline/50 px-2.5 gap-2"
      ondblclick={() => startRenaming(instance.name ?? "")}
    >
      <span class="text-[10px] uppercase tracking-wider text-text-muted/60 shrink-0">
        {paneTypeLabel(instance.type)}
      </span>
      {#if editingName}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          class="min-w-0 flex-1 bg-transparent text-[11px] text-text-primary font-mono outline-none placeholder:text-text-muted/40"
          placeholder="name this pane..."
          bind:value={nameInput}
          autofocus
          onblur={() => commitRename()}
          onkeydown={(e) => {
            if (e.key === "Enter") commitRename();
            if (e.key === "Escape") editingName = false;
          }}
        />
      {:else if instance.name}
        <span class="min-w-0 flex-1 truncate text-[11px] text-text-secondary font-mono">
          {instance.name}
        </span>
      {:else}
        <span class="flex-1"></span>
      {/if}
      {#if canClose()}
        <button
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-[12px] leading-none text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent-dim/50"
          onclick={(e) => {
            e.stopPropagation();
            closePane(sessionId, paneId);
          }}
          title="Close pane"
        >
          &times;
        </button>
      {/if}
    </div>

    <!-- Content area -->
    <div class="flex-1 min-h-0 min-w-0">
      {#if instance.type === "claude" && isDisconnected && session}
        <div class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]">
          <SessionPicker
            cwd={session.worktreePath}
            onContinue={() => {/* wired in Task 8 */}}
            onResume={() => {/* wired in Task 8 */}}
            onNew={() => {/* wired in Task 8 */}}
          />
        </div>
      {:else if instance.type === "markdown"}
        <LazyMarkdownPane docPath={instance.docPath ?? ""} />
      {:else if instance.type === "command"}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="relative flex h-full w-full flex-col">
          <div class="flex h-9 shrink-0 select-none items-center gap-2 border-b border-hairline bg-bg-surface/30 px-3">
            <span class="font-mono text-[11px] text-text-secondary truncate flex-1">{instance.command}</span>
            {#if instance.commandStatus === "running"}
              <span class="h-2 w-2 shrink-0 rounded-full bg-accent animate-pulse"></span>
            {:else if instance.commandStatus === "succeeded"}
              <span class="text-[10px] text-green font-mono">exit 0</span>
            {:else if instance.commandStatus === "failed"}
              <span class="text-[10px] text-red font-mono">exit {instance.commandExitCode ?? "?"}</span>
            {/if}
          </div>
          <div
            bind:this={containerEl}
            class="ui-terminal-frame min-h-0 flex-1"
            onmousedown={handleMouseDown}
          ></div>
        </div>
      {:else}
        <!-- claude or shell terminal -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="flex h-full w-full p-2">
          <div
            bind:this={containerEl}
            class="ui-terminal-frame h-full w-full overflow-hidden rounded-[0.95rem]"
            onmousedown={handleMouseDown}
          ></div>
        </div>
      {/if}
    </div>
  </div>
{/if}
```

- [ ] **Step 2: Verify it compiles**

Run: `npm run check`
Expected: No type errors related to PaneShell

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/PaneShell.svelte
git commit -m "feat(panes): add PaneShell component as thin pane renderer"
```

---

## Task 8: Rewrite SplitPane + Wire Everything Up

Replace the old pane system with the new one across all consumers. This is the integration task.

**Files:**
- Rewrite: `src/lib/components/SplitPane.svelte`
- Modify: `src/lib/commands/index.ts`
- Modify: `src/lib/queries/index.ts`
- Modify: `src/lib/stores/sessions.ts`
- Modify: `src/lib/sessions/close.ts`
- Modify: `src/lib/sessions/reconnect.ts`
- Modify: `src/lib/tasks/runner.ts`
- Modify: `src/lib/components/Layout.svelte`
- Modify: `src/lib/components/SessionTabs.svelte`
- Modify: `src/lib/components/NewSessionDialog.svelte`
- Modify: `src/lib/stores/paneDrag.ts`

- [ ] **Step 1: Rewrite SplitPane.svelte**

Replace `src/lib/components/SplitPane.svelte` with the simplified pure renderer. It imports `LayoutNode` from `$lib/panes/layout`, renders leafs via `PaneShell`, passes `visible` through for stacked/fullscreen panes. Port the tab bar rendering for stacked splits from the current component.

Key changes:
- Import `LayoutNode` instead of `SplitNode`
- Import `PaneShell` instead of `Terminal`, `ShellTerminal`, `CommandPane`
- Remove `{#key node.pane.id}` block
- Remove `handlePaneMouseDown` — `PaneShell` handles its own focus
- Keep fullscreen containment logic, reading from `fullscreenPaneId` in `$lib/panes/focus`
- Stack tab labels: use `getInstance(child.paneId)?.name ?? getInstance(child.paneId)?.type` from `$lib/panes/instances` or use `getStackLabel` from `layout.ts`

- [ ] **Step 2: Update commands/index.ts**

Replace imports from `$lib/stores/panes` with new modules:
- `addSplit` → `splitPane` from `$lib/panes/actions`
- `initSessionPanes` → `initSession` from `$lib/panes/actions`
- `navigatePane`, `movePaneInDirection`, `resizePane`, `toggleStack`, `renamePane`, `toggleFullscreen` → from `$lib/panes/layout` and `$lib/panes/focus`
- `closeFocusedPane` → from `$lib/panes/actions`

Update the split commands to use the new `splitPane` API which takes `CreatePaneOpts` without `id` (it auto-generates).

- [ ] **Step 3: Update queries/index.ts**

Replace imports:
- `paneTrees` → `sessionLayouts` from `$lib/panes/layout`
- `focusedPaneId` → from `$lib/panes/focus`
- `hasSplitPanes` → from `$lib/panes/layout`
- `getPane` → `getInstance` from `$lib/panes/instances`
- `activePaneTree()` → read from `sessionLayouts`

- [ ] **Step 4: Update stores/sessions.ts**

Replace imports:
- `focusTick` → remove (no longer needed)
- `focusedPaneId` → from `$lib/panes/focus`
- `listPanes` → `collectLeafIds` from `$lib/panes/layout` + `getInstance` from `$lib/panes/instances`
- `setActiveSession` → use `setLogicalFocus` from `$lib/panes/focus`

- [ ] **Step 5: Update sessions/close.ts**

Replace:
- `removeSessionPanes` → `closeSessionPanes` from `$lib/panes/actions`
- `disposeClaudeTerminal` → remove (handled by `closeSessionPanes` which calls `disposePane`)
- `closeAuxiliaryPanes` → `closeSessionPanes` handles everything

- [ ] **Step 6: Update sessions/reconnect.ts**

Replace:
- `disposeClaudeTerminal` → `replacePty` from `$lib/panes/instances`
- Instead of disposing the entire terminal, call `replacePty(mainPaneId, sessionId)` which keeps the terminal alive but swaps the PTY

- [ ] **Step 7: Update tasks/runner.ts**

Replace:
- `addSplit` → `splitPane` from `$lib/panes/actions`
- `focusedPaneId` → from `$lib/panes/focus`

- [ ] **Step 8: Update Layout.svelte, SessionTabs.svelte, NewSessionDialog.svelte**

Replace:
- `paneTrees` → `sessionLayouts` from `$lib/panes/layout`
- `initSessionPanes` → `initSession` from `$lib/panes/actions`

- [ ] **Step 9: Update stores/paneDrag.ts**

Replace:
- `DropSide` import → from `$lib/panes/layout`

- [ ] **Step 10: Wire PaneShell reconnect callbacks**

In `PaneShell.svelte`, wire up the `SessionPicker` callbacks for claude panes:
- `onContinue` → import `reconnectSession` from `$lib/sessions/reconnect`, call with `["--continue"]`
- `onResume` → call with `["--resume", claudeSessionId]`
- `onNew` → call with no extra flags

After reconnect, re-attach PTY output to the existing terminal instance using `replacePty` + `attachPtyOutput`.

- [ ] **Step 11: Set up persistence subscription**

In `App.svelte` or the appropriate top-level component, subscribe to `sessionLayouts` and call `scheduleSave` from `$lib/panes/persistence`. The `getDescriptors` callback extracts `PaneDescriptor` from `paneInstances` for each leaf ID.

In `initSession`, check for saved layouts and restore them using `loadLayout` + `loadPaneDescriptors`.

- [ ] **Step 12: Run type check**

Run: `npm run check`
Expected: No type errors

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "feat(panes): wire new pane system across all consumers"
```

---

## Task 9: Delete Old Files

**Files:**
- Delete: `src/lib/stores/panes.ts`
- Delete: `src/lib/panes/terminalRegistry.ts`
- Delete: `src/lib/panes/commandPaneRegistry.ts`
- Delete: `src/lib/components/Terminal.svelte`
- Delete: `src/lib/components/ShellTerminal.svelte`
- Delete: `src/lib/stores/__tests__/panes.test.ts`
- Delete: `src/lib/stores/__tests__/panes-stack.test.ts`
- Delete: `src/lib/stores/__tests__/panes-move.test.ts`
- Delete: `src/lib/stores/__tests__/panes-move-direction.test.ts`
- Delete: `src/lib/stores/__tests__/panes-fullscreen-resize.test.ts`
- Delete: `src/lib/stores/__tests__/panes-rename.test.ts`

- [ ] **Step 1: Verify no imports reference old files**

Run: `grep -r 'stores/panes' src/lib/ --include='*.ts' --include='*.svelte' | grep -v '__tests__' | grep -v 'node_modules'`
Expected: No matches (all imports migrated)

Run: `grep -r 'terminalRegistry' src/lib/ --include='*.ts' --include='*.svelte' | grep -v 'node_modules'`
Expected: No matches

Run: `grep -r 'commandPaneRegistry' src/lib/ --include='*.ts' --include='*.svelte' | grep -v 'node_modules'`
Expected: No matches

Run: `grep -r 'Terminal.svelte\|ShellTerminal.svelte' src/lib/ --include='*.ts' --include='*.svelte' | grep -v 'node_modules'`
Expected: No matches

- [ ] **Step 2: Delete old files**

```bash
rm src/lib/stores/panes.ts
rm src/lib/panes/terminalRegistry.ts
rm src/lib/panes/commandPaneRegistry.ts
rm src/lib/components/Terminal.svelte
rm src/lib/components/ShellTerminal.svelte
rm src/lib/stores/__tests__/panes.test.ts
rm src/lib/stores/__tests__/panes-stack.test.ts
rm src/lib/stores/__tests__/panes-move.test.ts
rm src/lib/stores/__tests__/panes-move-direction.test.ts
rm src/lib/stores/__tests__/panes-fullscreen-resize.test.ts
rm src/lib/stores/__tests__/panes-rename.test.ts
```

- [ ] **Step 3: Run all tests**

Run: `npm run test`
Expected: All tests PASS

- [ ] **Step 4: Run type check**

Run: `npm run check`
Expected: No type errors

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(panes): remove old pane store, terminal registry, and component files"
```

---

## Task 10: Terminal Initialization (xterm creation for real panes)

In the test-only world, `createPane` creates instances without real xterm terminals. For production, we need a function that creates the Terminal + addons and attaches PTY listeners.

**Files:**
- Modify: `src/lib/panes/instances.ts`
- Modify: `src/lib/panes/actions.ts`

- [ ] **Step 1: Add initTerminal function to instances.ts**

```typescript
// Add to src/lib/panes/instances.ts
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { settings } from "$lib/stores/settings";
import { getXtermTheme } from "$lib/themes";
import {
  attachPtyOutput,
  createPtyOutputChannel,
  onSessionExit,
  writeToSession,
  type SessionExitPayload,
} from "$lib/tauri";
import { get } from "svelte/store";
import { log } from "$lib/logging";

export function initTerminal(paneId: string) {
  const instance = getInstance(paneId);
  if (!instance || instance.terminal || instance.type === "markdown") return;

  const s = get(settings);
  const terminal = new Terminal({
    fontSize: s.fontSize,
    fontFamily: s.fontFamily,
    lineHeight: s.lineHeight,
    scrollback: s.scrollback,
    cursorStyle: s.cursorStyle as "block" | "underline" | "bar",
    cursorBlink: s.cursorBlink,
    theme: getXtermTheme(s.theme),
    disableStdin: true, // enabled by setLogicalFocus
  });

  const fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);
  try { terminal.loadAddon(new WebglAddon()); } catch {}
  terminal.loadAddon(new WebLinksAddon());

  // Wire input → PTY
  terminal.onData((data) => {
    writeToSession(instance.ptyId, data).catch((e) => {
      log(`Write failed for ${instance.ptyId}: ${e}`);
    });
  });

  paneInstances.update((map) => {
    const inst = map.get(paneId);
    if (inst) {
      inst.terminal = terminal;
      inst.fitAddon = fitAddon;
    }
    return new Map(map);
  });
}

export async function attachPtyListeners(
  paneId: string,
  onExit?: (payload: SessionExitPayload) => void
) {
  const instance = getInstance(paneId);
  if (!instance) return;

  // Exit listener
  if (onExit) {
    const unlisten = await onSessionExit(instance.ptyId, onExit);
    instance.unlisteners.push(unlisten);
  }

  // Output channel
  if (!instance.outputChannel) {
    instance.outputChannel = createPtyOutputChannel((bytes) => {
      instance.terminal?.write(bytes);
    });
  }
  await attachPtyOutput(instance.ptyId, instance.outputChannel);
}
```

- [ ] **Step 2: Update actions to call initTerminal**

In `src/lib/panes/actions.ts`, after `createPane` in `initSession` and `splitPane`, call `initTerminal(paneId)` and then `attachPtyListeners(paneId, exitHandler)`.

For shell panes, the exit handler calls `closePane`.
For claude panes, the exit handler calls `setSessionDisconnected`.
For command panes, the exit handler updates `commandStatus`/`commandExitCode` on the instance.

- [ ] **Step 3: Wire OSC 7 cwd tracking for shell terminals**

After `initTerminal` for shell panes, register the OSC 7 handler on `instance.terminal.parser`:

```typescript
instance.terminal.parser.registerOscHandler(7, (data) => {
  try {
    const url = new URL(data);
    updateInstance(paneId, {
      workingDir: decodeURIComponent(url.pathname),
      name: decodeURIComponent(url.pathname).split("/").pop(),
    });
  } catch {
    if (data.startsWith("/")) {
      updateInstance(paneId, {
        workingDir: data,
        name: data.split("/").pop(),
      });
    }
  }
  return false;
});
```

- [ ] **Step 4: Run type check and tests**

Run: `npm run check && npm run test`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add src/lib/panes/instances.ts src/lib/panes/actions.ts
git commit -m "feat(panes): add terminal initialization and PTY listener wiring"
```

---

## Task 11: Smoke Test

- [ ] **Step 1: Run the full test suite**

Run: `npm run test`
Expected: All tests PASS

- [ ] **Step 2: Run type checking**

Run: `npm run check`
Expected: No type errors

- [ ] **Step 3: Build and run dev**

Run: `task dev`
Expected: App starts. Test these scenarios manually:
1. Create a session — single claude pane renders
2. Split horizontal (Cmd+D) — new shell pane appears, you can type in it
3. Click back on the claude pane — keyboard input routes correctly
4. Split vertical (Cmd+Shift+D) — nested split works
5. Close a pane (Cmd+W) — pane removed, focus moves
6. Navigate panes (Alt+H/J/K/L) — focus moves correctly
7. Toggle stack (Cmd+Shift+S) — stacked view works
8. Close and reopen the app — layout restores from localStorage

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix(panes): smoke test fixes"
```
