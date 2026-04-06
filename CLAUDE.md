# CLAUDE.md

## Project Overview

Roux is a Tauri 2 + Svelte 5 desktop app that manages multiple Claude Code terminal sessions with split panes, stacked tabs, and shell terminals.

## Build & Run

```bash
npm install          # install frontend deps
task dev             # start dev server (frontend + Tauri)
npm run test         # run tests once
npm run test:watch   # tests in watch mode
npm run check        # svelte type checking
```

## Architecture

- **Frontend**: Svelte 5 with runes (`$state`, `$derived`, `$effect`, `$props`), Tailwind CSS 4, xterm.js for terminals
- **Backend**: Rust with Tauri 2, portable-pty for PTY management
- **State**: Svelte writable stores in `src/lib/stores/` -- no external state library
- **Pane tree**: `SplitNode` is a recursive union type (`pane | split`) in `src/lib/stores/panes.ts`
- **Terminal registry**: `src/lib/panes/terminalRegistry.ts` keeps xterm instances alive across component re-mounts
- **Commands**: Registered in `src/lib/commands/index.ts`, executed via command palette or keybindings
- **Layout persistence**: Pane layouts saved to localStorage, restored on session reopen

## Key Files

- `src/lib/stores/panes.ts` -- Pane tree data model, splits, stacking, navigation, persistence
- `src/lib/stores/sessions.ts` -- Session management (create, remove, status)
- `src/lib/components/SplitPane.svelte` -- Recursive pane renderer (split, stacked, leaf)
- `src/lib/components/Terminal.svelte` -- Claude Code terminal (main session terminal)
- `src/lib/components/ShellTerminal.svelte` -- Shell terminal pane
- `src/lib/components/CommandPane.svelte` -- Command execution pane
- `src/lib/commands/index.ts` -- All registered commands and keybindings
- `src-tauri/src/pty.rs` -- PTY spawning and management (Rust)
- `src-tauri/src/session.rs` -- Session persistence (Rust)

## Conventions

- Svelte 5 runes only -- no legacy `$:` reactive statements
- Props via `$props()` with TypeScript `interface Props`
- Reactive props in xterm callbacks must be captured as non-reactive copies (see `capturedSessionId` pattern in Terminal.svelte) to avoid stale parent prop access
- Immutable tree updates -- always spread to create new objects, never mutate in place
- Tests in `__tests__/` directories adjacent to source, using Vitest
- Same-direction splits flatten into siblings (no nested binary trees)
