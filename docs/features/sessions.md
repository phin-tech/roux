# Sessions

A **session** is a running agent/shell workflow attached to a specific project directory (and optionally a git worktree). Roux can run many independent sessions at once.

## Creating a session

- ++cmd+n++ opens the new-session dialog.
- Pick a project directory and optionally a git worktree.
- Optionally paste a GitHub PR URL to auto-resolve the review branch and local checkout target.
- A new pane is created with Claude Code running inside it.

## Projects

Sessions can be tagged with a **project**. Projects group related sessions across repos and worktrees so notes, documents, and defaults are shared between them.

Project records can also store repo lists, reusable session blueprints, project prompts, and context paths for spawned PTYs. See [Projects](projects.md).

## Sidebar and grouping

The session sidebar has a couple of useful modes:

- Sessions can be grouped by **repository** or **project**.
- ++cmd+"\\"++ collapses the full sidebar into a slim rail of session dots instead of hiding session switching entirely.
- The native **View** menu exposes the same sidebar toggle and session-grouping controls as the command palette.

## Session persistence

Session metadata (project, worktree, working directory) is persisted. When you relaunch Roux:

- **Startup restore** creates the session and its primary pane in disconnected state.
- **Reconnect** restores the full saved pane tree (splits + shell panes), then reconnects the primary agent pane.
- Shell pane restore uses the last known live working directory when available.

This is intentional: active agent processes are not silently resumed on launch.

## Closing a session

Closing the session terminates the Claude process and archives the session record instead of immediately hard-deleting it. Roux takes care not to kill unrelated `node` processes — only the session it owns.

## Sessions History

Closed sessions move into **Sessions History**. Open it from the command palette with **Toggle Sessions History**, or use the default leader chord ++cmd+; t s++.

The history pane shows two sections:

- **Active** — your currently live sessions, with their current status.
- **History** — archived sessions, sorted by most recently closed.

For an archived session you can:

- **Restore** it to the active list if its worktree still exists on disk.
- Open that session's **Notes**.
- **Show worktree** in your file manager.
- **Clean worktree** to remove the checkout but keep the history row.
- **Delete forever** to remove the archived record itself.

If you clean or manually remove a worktree, Restore is disabled for that archived row because Roux no longer has a checkout to reconnect.

See [Worktrees](worktrees.md) for how sessions interact with git worktrees.
