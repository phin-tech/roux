# Getting Started

This page walks you from a fresh install to your first Claude Code session in Roux.

## Prerequisites

- A working [Claude Code](https://docs.anthropic.com/en/docs/claude-code) install on your machine.
- Any projects you want to open should be normal git repositories on disk.

## First launch

When you open Roux for the first time you'll see an empty window with a top bar and a single empty pane.

## Creating your first session

1. Press ++cmd+n++ to open the **New session** dialog.
2. Pick a project directory. Roux will remember it for next time.
3. Optionally choose a git worktree to isolate this session's working copy.
4. Confirm to spawn a Claude Code session in a new pane.

## Splitting and stacking

- ++cmd+d++ splits the current pane horizontally.
- ++cmd+shift+d++ splits vertically.
- ++cmd+shift+s++ toggles stacking so inactive panes collapse into Zellij-style title bars.
- ++cmd+w++ closes the current pane.

Roux automatically flattens consecutive splits in the same direction, so your layout stays clean.

## Opening a shell

From the command palette (++cmd+k++), run **New shell pane** to open a regular shell alongside Claude. Shell panes persist across restarts and respawn automatically.

## Next steps

- [Panes](features/panes.md) — splits, stacks, focus
- [Sessions](features/sessions.md) — Claude and shell lifetimes
- [Keyboard shortcuts](keyboard-shortcuts.md)
