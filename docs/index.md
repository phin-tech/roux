---
title: Roux
---

# Roux

A desktop terminal multiplexer for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built with Tauri and Svelte.

Roux lets you run multiple Claude Code sessions side-by-side with split panes, stacked tabs, shell terminals, and persistent layouts — all in a single native window.

![Roux screenshot](images/screenshot.jpg)

## Highlights

- **Multi-session** — run independent Claude Code sessions, each with its own git worktree
- **Split panes** — horizontal and vertical splits with same-direction flattening
- **Stacked panes** — Zellij-style tab stacking
- **Layouts** — KDL-based session templates that define multi-pane setups with spawn profiles
- **Session restore on reconnect** — restores full split/shell layouts from saved pane state
- **Shell terminals** — open shell panes alongside Claude for running commands
- **Command palette** — quick access to every action via ++cmd+k++
- **Leader mode** — Vimish pane and session actions via ++cmd+;++ with inline pane rename
- **Configurable keymap** — every shortcut lives in a KDL file you can edit; ships with `default` and `tmux` presets, supports sticky/passthrough modes and per-tree HUD timing
- **Layout persistence** — pane layouts survive app restarts; shell panes respawn automatically
- **Session from PR URL** — paste a GitHub PR URL in New Session and Roux prepares the review branch/worktree
- **Doctor panel** — inspect and reinstall CLI/hooks/skill integrations from Settings
- **Projects** — tag sessions with projects to organize related work across repos and worktrees
- **Multi-scoped notes vault** — plain-text notes sidebar (++cmd+b++) with four scopes (global / project / repo / session), backed by an Obsidian-compatible markdown vault. Scriptable from `roux notes <scope> <verb>` and exposed to agents through per-PTY env vars. Experimental.
- **CLI bridge** — `roux-cli` for scripting: split panes, create sessions, run commands, send text, and focus panes from the terminal

## Next steps

- [What's New](whats-new.md)
- [Install Roux](install.md)
- [Getting started](getting-started.md)
- [Keyboard shortcuts](keyboard-shortcuts.md)
