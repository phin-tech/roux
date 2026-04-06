# Stacked Panes Design

## Overview

Add Zellij-style stacked panes to the existing split tree layout. Any split node can toggle into "stacked" mode where its children are displayed as vertically collapsed title bars with only the active child expanded, rather than laid out side-by-side.

## Data Model

Add two optional fields to the existing split variant in `SplitNode`:

```typescript
export type SplitNode =
  | { kind: "pane"; pane: Pane }
  | { kind: "split"; direction: SplitDirection; children: SplitNode[];
      stacked?: boolean; activeIndex?: number };
```

- `stacked` defaults to `false`/`undefined` (normal layout behavior).
- `activeIndex` (0-based) tracks which child is expanded when stacked.
- When a child is removed and `activeIndex` is out of bounds, clamp it to the last valid index.
- Unstacking sets `stacked = false` -- direction and children are preserved, layout returns to normal.

No Tauri backend changes required. This is purely a frontend layout concern.

## Rendering

When `node.kind === "split" && node.stacked`, `SplitPane.svelte` renders a stacked view instead of the normal flex layout:

```
+-------------------------+
| > Pane 1                |  <- collapsed: single-line title bar
+-------------------------+
| v Pane 2                |  <- active: title bar + full content
|                         |
|       content           |
|                         |
+-------------------------+
| > P3 | P4               |  <- collapsed: label from child leaf names
+-------------------------+
```

- Collapsed children render as a single-line title bar, using the same styling as existing pane title bars.
- The active child (`children[activeIndex]`) gets all remaining vertical space (`flex-grow: 1`).
- Clicking a collapsed title bar sets `activeIndex` to that child.
- If a collapsed child is a sub-split, its label is derived from its leaf pane names joined with `|`.

## Keybindings

### Toggle: `cmd+shift+s`

Cycling behavior:

1. First press: stacks the immediate parent split of the focused pane.
2. Second press: if the parent is already stacked, walk up the tree and stack the next ancestor split (unstacking the previous one).
3. Third press: unstacks everything (back to normal).

### `cmd+k` menu

Adds a "Stack/Unstack" option that lists ancestor splits, letting the user target a specific level directly.

## Navigation

### Inside a stack

- `alt+j` / `alt+k` cycle `activeIndex` down/up through the stacked tabs.
- `alt+h` / `alt+l` navigate out of the stack to adjacent panes (spatial navigation, unchanged from current behavior).
- Clicking a collapsed title bar activates that tab.

### Focus behavior

- When a tab is activated, focus moves to the first leaf pane inside that child.
- When a stacked child becomes collapsed and it contained the focused pane, focus moves to the newly active child's first leaf pane.

## Edge Cases

- **Auto-unstack:** If a stack drops to 1 child (after removing a pane), automatically set `stacked = false`.
- **Splitting inside a stack:** `cmd+d` / `cmd+shift+d` on a focused pane inside a stacked child splits that child normally. The stack tab now contains a sub-split.
- **Drag-and-drop:** Dragging a pane onto a stacked area adds it as a new tab (appended to children). Dragging a pane out removes it from the stack.
- **Serialization:** `stacked` and `activeIndex` serialize naturally as properties on the split node. No special handling needed.

## Files to Modify

- `src/lib/stores/panes.ts` -- data model, toggle logic, navigation changes
- `src/lib/components/SplitPane.svelte` -- stacked rendering mode
- `src/lib/commands/index.ts` -- new keybindings (`cmd+shift+s`, `cmd+k` stack option, `alt+j`/`alt+k` adaptation)
