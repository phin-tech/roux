# Sessions

A **session** is a running Claude Code process attached to a specific project directory (and optionally a git worktree). Roux can run many independent sessions at once.

## Creating a session

- ++cmd+n++ opens the new-session dialog.
- Pick a project directory and optionally a git worktree.
- A new pane is created with Claude Code running inside it.

## Projects

Sessions can be tagged with a **project**. Projects group related sessions across repos and worktrees so notes, documents, and defaults are shared between them.

## Session persistence

Session metadata (project, worktree, working directory) is persisted. When you relaunch Roux:

- **Shell panes** are automatically respawned.
- **Claude session panes** are recreated as empty panes so you can explicitly start a new Claude process.

This is intentional: Claude Code sessions are not safe to silently resume.

## Closing a session

Closing the pane (++cmd+w++) terminates the Claude process. Roux takes care not to kill unrelated `node` processes — only the session it owns.

See [Worktrees](worktrees.md) for how sessions interact with git worktrees.
