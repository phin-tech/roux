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
- **Collapsible session rail** — shrink the sidebar to a slim strip of session dots without giving up quick session switching
- **Layouts** — KDL-based session templates that define multi-pane setups with spawn profiles
- **Session restore on reconnect** — restores full split/shell layouts from saved pane state
- **Session history** — closed sessions move into a history pane where you can restore them, open their notes, or delete them forever
- **Shell terminals** — open shell panes alongside Claude for running commands
- **Multiline editor** — docked terminal input editor with terminal-selection reinput, command corrections, context chips, and shell-style editing keys
- **Command palette** — quick access to every action via ++cmd+k++
- **Leader mode** — Vimish pane and session actions via ++cmd+;++ with inline pane rename
- **Configurable keymap** — every shortcut lives in a KDL file you can edit; ships with `default` and `tmux` presets, supports sticky/passthrough modes and per-tree HUD timing
- **Layout persistence** — pane layouts survive app restarts; shell panes respawn automatically
- **Session from PR URL** — paste a GitHub PR URL in New Session and Roux prepares the review branch/worktree
- **Independent terminal themes** — keep the terminal palette separate from the app chrome, including user-imported `.itermcolors` themes
- **Native menu bar** — File/Edit/View/Session/Pane/Tools/Window/Help menus on macOS, Windows, and Linux
- **Doctor panel** — inspect and reinstall CLI/hooks/skill integrations from Settings
- **Agent notifications** — configure Claude Code hooks and Codex TUI notifications from Settings
- **Projects** — group sessions across repos, save session blueprints, and inject project context
- **Multi-scoped notes vault** — plain-text notes sidebar (++cmd+b++) with four scopes (global / project / repo / session), backed by an Obsidian-compatible markdown vault. Scriptable from `roux notes <scope> <verb>` and exposed to agents through per-PTY env vars. Experimental.
- **CLI bridge** — `roux` for scripting: split panes, create sessions, run commands, send text, and focus panes from the terminal
- **MCP integration** — expose Roux sessions, panes, latest terminal output, and notes to supported MCP hosts through `roux mcp`

## Next steps

- [What's New](whats-new.md)
- [Install Roux](install.md)
- [Getting started](getting-started.md)
- [Keyboard shortcuts](keyboard-shortcuts.md)
