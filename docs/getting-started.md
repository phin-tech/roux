# Getting Started

This page walks you from a fresh install to your first Claude Code session in Roux.

## Prerequisites

- A working [Claude Code](https://docs.anthropic.com/en/docs/claude-code) install on your machine.

Projects can be any directory on disk. A git repository is only required if you want to use [worktrees](features/worktrees.md).

## First launch

On first launch, Roux opens to a single empty pane with the session sidebar on the left and the native menu bar available on supported platforms.

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

## Sidebar and session history

- ++cmd+"\\"++ collapses the session sidebar into a slim rail of session dots; press it again to expand.
- Closing a session archives it into **Sessions History** instead of immediately deleting the record.
- Open **Toggle Sessions History** from the command palette, or use the default leader chord ++cmd+; t s++, to restore closed sessions, open their notes, or permanently delete the history entry.

## Opening a shell

From the command palette (++cmd+k++), run **New shell pane** to open a regular shell alongside Claude. Shell panes persist across restarts and respawn automatically.

## Agent notifications

Open **Settings -> Notifications** to check whether agent notifications are wired up.

- **Claude Code** uses Roux's existing hook installer. Choose **Configure** or **Reinstall** if the hooks are missing or stale.
- **Codex** uses `~/.codex/config.toml`. Choose **Preview** to inspect the TOML Roux will write, then **Configure** to set `[tui].notification_condition = "always"`.

The global OS notification toggle only controls macOS/desktop fan-out. Roux still keeps in-app notifications and unread badges available.

## Kanban board

Use the Kanban board to turn a written task into a daemon-owned agent run.

- **Start** creates a new run, links it to a daemon session/PTY, and moves the
  card to **In Progress**.
- **Open terminal** attaches to the latest linked run/session without creating a
  new run.
- Blocked decision prompts appear on the card and detail view with numbered
  choices. Picking a choice writes that value back to the session.

See [Kanban Board](features/kanban.md) for run history, decision timeout, and
deletion behavior.

## Reconnect and restore

On launch, restored sessions appear disconnected by design. Click **Reconnect** on a session card to restore its saved pane layout (including shell splits) and reconnect the main agent pane.

## Working with commands

When working with shell terminals or agent TUIs, use the multiline editor to prepare input before sending it:

- ++ctrl+g++ toggles the editor for the focused terminal pane.
- Select terminal text, then press ++ctrl+g++ to reopen that text in the editor.
- ++cmd+shift+e++ toggles the editor from anywhere in the app.
- ++cmd+shift+v++ opens the editor with clipboard contents.
- ++cmd+enter++ sends the editor text and keeps the editor open for follow-up edits.

See [Multiline Editor](features/editor.md) for full details, including local editing keys, command corrections, and context chips.

## Next steps

- [Panes](features/panes.md) — splits, stacks, focus
- [Sessions](features/sessions.md) — Claude and shell lifetimes
- [Notifications](features/notifications.md) — in-app inbox, OS notifications, and agent setup
- [Multiline Editor](features/editor.md) — docked terminal input editor
- [Keyboard shortcuts](keyboard-shortcuts.md)
