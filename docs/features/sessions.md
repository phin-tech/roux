# Sessions

A **session** is a running agent/shell workflow attached to a specific project directory (and optionally a git worktree). Roux can run many independent sessions at once.

## Creating a session

- ++cmd+n++ opens the new-session dialog.
- Pick a project directory and optionally a git worktree.
- Optionally paste a GitHub PR URL to auto-resolve the review branch and local checkout target.
- A new pane is created with Claude Code running inside it.

## Projects

Sessions can be tagged with a **project**. Projects group related sessions across repos and worktrees so notes, documents, and defaults are shared between them.

## Session persistence

Session metadata (project, worktree, working directory) is persisted. When you relaunch Roux:

- **Startup restore** creates the session and its primary pane in disconnected state.
- **Reconnect** restores the full saved pane tree (splits + shell panes), then reconnects the primary agent pane.
- Shell pane restore uses the last known live working directory when available.

This is intentional: active agent processes are not silently resumed on launch.

## Closing a session

Closing the pane (++cmd+w++) terminates the Claude process. Roux takes care not to kill unrelated `node` processes — only the session it owns.

See [Worktrees](worktrees.md) for how sessions interact with git worktrees.
