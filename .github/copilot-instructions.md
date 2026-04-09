# Copilot Instructions

## Project Overview

Roux is a Tauri 2 + Svelte 5 desktop app for managing multiple Claude Code terminal sessions in one native window. It combines split panes and stacked tabs, persistent Claude and shell terminals, git worktree support, a Unix socket / CLI bridge, and project notes, docs, tasks, and watches.

This codebase has real filesystem, process, and terminal side effects. Be defensive in reasoning and suggestions.

## Build & Run

```bash
npm install          # install frontend deps
task dev             # start frontend + Tauri
npm run test         # run tests once
npm run test:watch   # run tests in watch mode
npm run check        # Svelte type check
```

Always run `npm install` before building. Tests use Vitest. Rust code lives in `src-tauri/` and can be checked with `cargo check` or `cargo build` from that directory.

## Architecture

- **Frontend**: Svelte 5 with runes (`$state`, `$derived`, `$effect`, `$props`), Tailwind CSS 4, xterm.js
- **Backend**: Rust with Tauri 2, portable-pty for PTY management
- **State**: Svelte writable stores in `src/lib/stores/`
- **Pane tree**: `SplitNode` recursive union in `src/lib/stores/panes.ts`
- **Terminal registry**: `src/lib/panes/terminalRegistry.ts` keeps xterm instances alive across re-mounts
- **Commands**: frontend command registration in `src/lib/commands/index.ts`
- **Backend commands**: Tauri commands registered from `src-tauri/src/main.rs`
- **Persistence**: layouts in localStorage, sessions/settings/projects handled in Rust

## Key Files

- `src/lib/stores/panes.ts` - pane tree data model, splits, stacking, persistence
- `src/lib/stores/sessions.ts` - session management on the frontend
- `src/lib/components/SplitPane.svelte` - recursive pane renderer
- `src/lib/components/Terminal.svelte` - Claude Code terminal pane
- `src/lib/components/ShellTerminal.svelte` - shell terminal pane
- `src/lib/commands/index.ts` - command palette and keybindings
- `src-tauri/src/main.rs` - Tauri bootstrap and command registration
- `src-tauri/src/pty.rs` - PTY spawning and lifecycle
- `src-tauri/src/session.rs` - session persistence
- `src-tauri/src/watches.rs` - watch execution and state
- `src-tauri/src/worktree.rs` - git worktree operations
- `src-tauri/src/socket.rs` - local socket protocol / CLI bridge

## Frontend Conventions

- Always use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`). Never use legacy `$:` reactivity.
- Define props via `$props()` with typed `Props` interfaces.
- Prefer immutable updates over in-place mutation.
- Keep pane-tree invariants intact; same-direction splits must flatten.
- When wiring xterm callbacks, capture stable non-reactive copies where needed.
- Tests live in adjacent `__tests__/` directories and use Vitest.

## Backend Conventions

- Prefer typed internal errors over `Result<_, String>`.
- Keep Tauri command handlers thin; push logic into normal Rust functions/modules.
- Be careful with PTY, socket, process, and filesystem lifecycles.
- Avoid broad process-kill commands; Claude Code is a Node app, so never blindly kill `node` / `node.exe`.
- Never use `git add .`; always stage files explicitly.

## Critical Invariants

Do not break these:
- **Pane tree structure**: same-direction splits must flatten; the recursive `SplitNode` union must stay consistent.
- **Terminal registry lifecycle**: xterm instances must survive re-mounts via the registry.
- **PTY lifecycle**: PTYs must not leak; they require explicit cleanup.
- **Tauri command contracts**: changing a Rust command signature requires updating the frontend caller and potentially the CLI/socket protocol.
- **Persisted data shapes**: changes to session, project, or settings structures must account for migration of existing data.

## Testing

- TDD is the primary development method. Prefer red-green-refactor.
- Start by writing or identifying a failing test when the behavior can be tested.
- Make the smallest change that gets the test green.
- Run `npm run test` to verify frontend changes, `npm run check` for Svelte type checking.

## Before Changing Tauri Commands

Always identify:
1. Which frontend caller depends on the command
2. Which persisted data shape might change
3. Whether the CLI/socket protocol also depends on it

## Code Style

- Do not extract abstractions prematurely; prefer concrete code until there are multiple real examples.
- Do not add unnecessary error handling or fallbacks for impossible scenarios.
- Do not introduce new stores, services, or helpers just for cleanliness — wait for the third real repetition.
