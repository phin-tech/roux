# Getting Started

This page walks you from a fresh install to your first Claude Code session in Roux.

## Prerequisites

- A working [Claude Code](https://docs.anthropic.com/en/docs/claude-code) install on your machine.

Projects can be any directory on disk. A git repository is only required if you want to use [worktrees](features/worktrees.md).

## First launch

When you open Roux for the first time you'll see an empty window with a top bar and a single empty pane.

## Creating your first session

1. Press ++cmd+n++ to open the **New session** dialog.
2. Pick a project directory (or use repo-root quick-pick results if configured in Settings).
3. Optionally paste a GitHub PR URL to prepare a PR review session.
4. Optionally choose a git worktree to isolate this session's working copy.
5. Confirm to spawn a Claude Code session in a new pane.

## Splitting and stacking

- ++cmd+d++ splits the current pane horizontally.
- ++cmd+shift+d++ splits vertically.
- ++cmd+shift+s++ toggles stacking so inactive panes collapse into Zellij-style title bars.
- ++cmd+w++ closes the current pane.

Roux automatically flattens consecutive splits in the same direction, so your layout stays clean.

## Opening a shell

From the command palette (++cmd+k++), run **New shell pane** to open a regular shell alongside Claude. Shell panes persist across restarts and respawn automatically.

## Reconnect and restore

On launch, restored sessions appear disconnected by design. Click **Reconnect** on a session card to restore its saved pane layout (including shell splits) and reconnect the main agent pane.

## Next steps

- [Panes](features/panes.md) — splits, stacks, focus
- [Sessions](features/sessions.md) — Claude and shell lifetimes
- [Keyboard shortcuts](keyboard-shortcuts.md)
