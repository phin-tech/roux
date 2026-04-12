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
- **Shell terminals** — open shell panes alongside Claude for running commands
- **Command palette** — quick access to every action via ++cmd+k++
- **Layout persistence** — pane layouts survive app restarts; shell panes respawn automatically
- **Projects** — tag sessions with projects to organize related work across repos and worktrees
- **Project notes** — per-project plain-text notes sidebar (++cmd+b++) shared across all sessions in a project
- **CLI bridge** — `roux-cli` for scripting: split panes, create sessions, run commands, send text, and focus panes from the terminal

## Next steps

- [Install Roux](install.md)
- [Getting started](getting-started.md)
- [Keyboard shortcuts](keyboard-shortcuts.md)
