---
title: Roux
---

# Roux

A desktop terminal multiplexer for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built with Tauri and Svelte.

Roux lets you run multiple Claude Code sessions side-by-side with split panes, stacked tabs, shell terminals, and persistent layouts — all in a single native window.

![Roux screenshot](images/screenshot.jpg)

## Highlights

<div class="grid cards" markdown>

- **Multi-session**

  Run independent Claude Code sessions side-by-side, each with its own git worktree.

- **Split & stacked panes**

  Horizontal and vertical splits with same-direction flattening, plus Zellij-style tab stacking.

- **Shell terminals**

  Open shell panes alongside Claude for running commands without leaving the window.

- **Layouts & persistence**

  KDL-based session templates and pane layouts that survive restarts; shell panes respawn automatically.

- **Session restore**

  Reconnect and restore full split/shell layouts from saved pane state, with a history pane for closed sessions.

- **Command palette & leader mode**

  Reach every action via ++cmd+k++, or Vimish pane/session actions via ++cmd+;++ with inline rename.

- **Configurable keymap**

  Every shortcut lives in an editable KDL file; ships with `default` and `tmux` presets.

- **Projects & Kanban**

  Group sessions across repos, save blueprints, and plan card-based agent work on a daemon-owned board.

- **CLI bridge & MCP**

  Script panes and sessions from the terminal with `roux`, and expose sessions, panes, and notes through `roux mcp`.

</div>

For the full feature set — notes vault, watches, automation hooks, notifications, terminal themes, and more — see the [Features overview](features/index.md).

## Next steps

- [What's New](whats-new.md)
- [Install Roux](install.md)
- [Getting started](getting-started.md)
- [Keyboard shortcuts](keyboard-shortcuts.md)
