# Roux

![Roux screenshot](docs/images/screenshot.jpg)

A desktop terminal multiplexer for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built with Tauri and Svelte.

Roux lets you run multiple Claude Code sessions side-by-side with split panes, stacked tabs, shell terminals, and persistent layouts -- all in a single native window.

## Features

- **Multi-session** -- Run independent Claude Code sessions, each with its own git worktree
- **Split panes** -- Horizontal and vertical splits with same-direction flattening
- **Stacked panes** -- Zellij-style tab stacking where collapsed title bars show inactive panes
- **Shell terminals** -- Open shell panes alongside Claude for running commands
- **Command palette** -- Quick access to all actions via the primary shortcut (`cmd+k` on macOS, `ctrl+k` on Windows/Linux)
- **Layout persistence** -- Pane layouts survive app restarts; shell panes respawn automatically
- **Themes** -- Multiple built-in color schemes
- **Projects** -- Tag sessions with projects to organize related work across repos and worktrees
- **Project notes** -- Per-project plain-text notes sidebar (`cmd+b`) shared across all sessions in a project
- **Command panes** -- Run shell commands in dedicated panes with rerun support
- **Task runner** -- Run predefined commands from configuration files
- **Document viewer** -- Open markdown files in dedicated panes
- **CLI** -- `roux-cli` for scripting: split panes, create sessions, run commands, send text, and focus panes from the terminal via the local Roux command channel

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
task test             # full frontend + Rust gate
npm run test          # frontend tests only
npm run test:watch    # frontend watch mode
npm run check         # svelte type check
```

### Windows

Native Windows x64 builds are supported with a per-user NSIS installer:

```powershell
task windows:build
```

See [docs/windows-build.md](docs/windows-build.md) for prerequisites and runtime notes.

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
