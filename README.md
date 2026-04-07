# Roux

A desktop terminal multiplexer for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built with Tauri and Svelte.

Roux lets you run multiple Claude Code sessions side-by-side with split panes, stacked tabs, shell terminals, and persistent layouts -- all in a single native window.

## Features

- **Multi-session** -- Run independent Claude Code sessions, each with its own git worktree
- **Split panes** -- Horizontal and vertical splits with same-direction flattening
- **Stacked panes** -- Zellij-style tab stacking where collapsed title bars show inactive panes
- **Shell terminals** -- Open shell panes alongside Claude for running commands
- **Command palette** -- Quick access to all actions via `cmd+k`
- **Layout persistence** -- Pane layouts survive app restarts; shell panes respawn automatically
- **Themes** -- Multiple built-in color schemes
- **Projects** -- Tag sessions with projects to organize related work across repos and worktrees
- **Project notes** -- Per-project plain-text notes sidebar (`cmd+b`) shared across all sessions in a project
- **Task runner** -- Run predefined commands from configuration files
- **Document viewer** -- Open markdown files in dedicated panes

## Keybindings

| Action | Shortcut |
|---|---|
| Split horizontal | `cmd+d` |
| Split vertical | `cmd+shift+d` |
| Close pane | `cmd+w` |
| Toggle stack | `cmd+shift+s` |
| Focus left | `alt+h` |
| Focus down | `alt+j` |
| Focus up | `alt+k` |
| Focus right | `alt+l` |
| Toggle notes | `cmd+b` |
| Command palette | `cmd+k` |
| New session | `cmd+n` |
| Settings | `cmd+,` |

## Tech Stack

- **Frontend** -- Svelte 5, TypeScript, Tailwind CSS 4, xterm.js
- **Backend** -- Rust, Tauri 2, portable-pty
- **Build** -- Vite

## Development

```bash
npm install
task dev
```

### Tests

```bash
npm run test          # run once
npm run test:watch    # watch mode
npm run check         # svelte type check
```

## Release

Version bump, tag, and push:

```bash
task version BUMP=patch
task version BUMP=minor
task version BUMP=major
```

Build, sign, notarize, and staple on a signing Mac:

```bash
op run --env-file=.env.signing -- task sign
```

Create or update the GitHub release for the current version and upload the signed artifacts:

```bash
task publish
```

Or do signing and publishing together:

```bash
task publish:op
```

See [docs/macos-signing.md](docs/macos-signing.md) for macOS signing and notarization setup.
