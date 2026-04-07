# OSC 7 Shell CWD Tracking & Restore

## Problem

Shell panes always restart in the session's worktree path, even if the user had `cd`'d elsewhere. The shell's working directory is lost on app restart.

## Solution

Capture OSC 7 escape sequences (emitted by modern shells to report cwd changes) in shell terminals. Store the cwd on the pane, update the tab label to show the directory name, and use the stored cwd when restoring shell panes on app restart.

## Changes

### 1. Add `updatePaneWorkingDir` to `src/lib/stores/panes.ts`

A function that updates both `workingDir` and `name` on a pane in the tree:

```typescript
export function updatePaneWorkingDir(sessionId: string, paneId: string, cwd: string) {
  const dirName = cwd.split("/").pop() || cwd;
  paneTrees.update((trees) => {
    const tree = trees.get(sessionId);
    if (!tree) return trees;
    trees.set(sessionId, updatePaneFields(tree, paneId, { workingDir: cwd, name: dirName }));
    return new Map(trees);
  });
}
```

Uses a helper similar to the existing `setPaneNameInTree` but updates multiple fields. The pane tree subscription already debounces saves to localStorage, so this persists automatically.

### 2. Register OSC 7 handler in `src/lib/components/ShellTerminal.svelte`

When the terminal is first created (inside the `!term.element` branch in `onMount`), register an OSC 7 parser handler:

```typescript
term.parser.registerOscHandler(7, (data) => {
  // OSC 7 format: file://hostname/path/to/dir
  try {
    const url = new URL(data);
    const cwd = decodeURIComponent(url.pathname);
    updatePaneWorkingDir(sessionId, paneId, cwd);
  } catch {
    // Not a valid URL — some shells emit just the path
    if (data.startsWith("/")) {
      updatePaneWorkingDir(sessionId, paneId, data);
    }
  }
  return false; // allow other handlers to also process
});
```

This requires adding `sessionId` as a prop to `ShellTerminal` (currently it only receives `ptyId` and `paneId`).

### 3. Use stored cwd when restoring shells in `src/App.svelte`

Change the shell spawn call from:

```typescript
spawnShell(pane.ptyId, s.worktreePath)
```

to:

```typescript
spawnShell(pane.ptyId, pane.workingDir ?? s.worktreePath)
```

## What stays the same

- `Pane` type already has `workingDir?: string` — no type change needed
- Layout persistence via `paneTrees` subscription — already saves all pane fields
- No Rust changes needed — OSC 7 is parsed on the frontend
- `getStackLabel` already uses `pane.name` — tab labels update automatically
