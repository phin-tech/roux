# AGENTS.md

## Project Overview

Roux is a Tauri 2 + Svelte 5 desktop app for managing multiple Claude Code terminal sessions in one native window.

The app combines:
- split panes and stacked tabs
- persistent Claude and shell terminals
- git worktree support
- a Unix socket / CLI bridge
- project notes, docs, tasks, and watches

This codebase has real filesystem, process, and terminal side effects. Defensive reasoning is the correct default.

## Build & Run

```bash
npm install          # install frontend deps
task dev             # start frontend + Tauri
npm run test         # run tests once
npm run test:watch   # run tests in watch mode
npm run check        # Svelte type check
```

## Architecture

- **Frontend**: Svelte 5 with runes (`$state`, `$derived`, `$effect`, `$props`), Tailwind CSS 4, xterm.js
- **Backend**: Rust with Tauri 2, portable-pty for PTY management
- **State**: Svelte writable stores in `src/lib/stores/`
- **Pane tree**: `SplitNode` recursive union in `src/lib/stores/panes.ts`
- **Terminal registry**: `src/lib/panes/terminalRegistry.ts` keeps xterm instances alive across re-mounts
- **Commands**: frontend command registration in `src/lib/commands/index.ts`
- **Backend commands**: Tauri commands are registered from `src-tauri/src/main.rs`
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

- Svelte 5 runes only; do not introduce legacy `$:` reactivity
- Props via `$props()` and typed `Props` interfaces
- Prefer immutable updates over in-place mutation
- Keep pane-tree invariants intact; same-direction splits should flatten
- When wiring xterm callbacks, capture stable non-reactive copies where needed
- Tests live in adjacent `__tests__/` directories and use Vitest

## Backend Conventions

- Prefer typed internal errors over `Result<_, String>`
- Keep Tauri command handlers thin; push logic into normal Rust functions/modules
- Be careful with PTY, socket, process, and filesystem lifecycles
- Avoid broad process-kill commands; Claude Code is a Node app, so never blindly kill `node` / `node.exe`
- Do not use `git add .`; stage files explicitly

## Working With The Human

The human is the decision-maker. Your job is to reason clearly, verify against reality, and avoid compounding mistakes.

For this repo, the failure modes are:
- breaking pane-tree invariants
- corrupting persisted session/project/settings state
- leaking PTYs, watchers, or background tasks
- making a wrong filesystem assumption and deleting or moving the wrong thing
- changing Tauri/Rust contracts without updating the frontend

The cost of being wrong is higher than the cost of being slow.

## Rule 0

**Reality does not care about your model. When reality contradicts your model, stop and update the model before proceeding.**

If a command, test, build, or runtime check fails unexpectedly:
1. report the raw failure
2. state your current theory
3. state what you want to try next
4. state what you expect to happen
5. ask the human before taking another risky step

Do not silently retry and do not bury the failure.

## Explicit Reasoning Protocol

Before any action that could fail, write:

```text
DOING: [action]
EXPECT: [specific predicted outcome]
IF YES: [conclusion, next action]
IF NO: [conclusion, next action]
```

After the action, compare prediction to reality:

```text
RESULT: [what actually happened]
MATCHES: [yes/no]
THEREFORE: [conclusion and next action]
```

This matters most for:
- file edits with behavioral impact
- shell commands that mutate state
- git operations
- process management
- tests, builds, and reproductions
- refactors across frontend/backend boundaries

## Epistemic Hygiene

Distinguish clearly between:
- **belief**: what you think is true
- **verification**: what you observed directly

Use language that reflects that difference.

- "I believe X" means unverified theory
- "I verified X" means you observed it in code, output, logs, or tests

If you do not know, say so. "I don't know yet" is better than fake confidence.

## Notice Confusion

If something "should work" but does not, the problem is your model, not reality.

When surprised:
- stop
- identify the false assumption
- say it plainly

Example:

```text
I assumed session cleanup happened when a pane closed, but the PTY manager keeps the process alive until explicit kill. My model of session lifetime was wrong.
```

## Feedback Loops

Work in small batches.

- Do at most 3 meaningful actions, then checkpoint
- A checkpoint means observable reality: test output, build output, logs, runtime behavior, or direct code inspection
- Thinking, planning, and TODOs are not checkpoints

More than 5 actions without verification means you are probably accumulating unjustified beliefs.

## Testing Protocol

TDD is the primary development method in this repo.

- Prefer red-green-refactor over implementation-first changes
- Start by writing or identifying a failing test when the behavior can be tested
- Make the smallest change that gets the test green
- Refactor only after the test is passing again
- If it is unclear what the correct test surface is, confirm with the human before proceeding

For this repo, "test surface" may mean:
- frontend unit/component tests
- store tests
- Rust unit tests
- integration-level Tauri/backend checks
- manual runtime verification for behavior that is not yet automated

One test at a time. Run it. Read it. Then decide.

Before marking any testing task complete, state:

```text
VERIFY: Ran [exact test/build/check command] - Result: [PASS/FAIL/DID NOT RUN]
```

If it did not run, it is not complete.

For this repo, common verification commands are:
- `npm run test`
- `npm run check`
- targeted Vitest runs
- targeted Rust builds/checks in `src-tauri`

If the correct verification path is ambiguous, ask the human which test or verification source should be treated as authoritative.

## Tauri-Specific Discipline

- Keep command handlers as adapters, not as the main home for business logic
- Be explicit about Rust/TypeScript contract changes
- Treat `AppState`, PTY/session lifecycle, socket handling, and watchers as long-lived system components
- Verify startup/shutdown behavior when changing background services
- Be conservative around filesystem and worktree operations
- Do not guess Tauri behavior when the official docs are available
- If unsure about commands, plugins, config, permissions, app lifecycle, or packaging, check [tauri.app/start](https://tauri.app/start/) first
- Do not be afraid to consult the docs; documented reality beats remembered intuition

Before changing a Tauri command or backend payload, identify:
- which frontend caller depends on it
- which persisted data shape might change
- whether the CLI/socket protocol also depends on it

## Root Cause Discipline

Do not stop at the first visible symptom.

For any bug, ask:
1. what directly failed?
2. why was the system in a state where that failure was possible?
3. what design or invariant allowed that state to exist?

Fixing the surface symptom alone is often not enough.

## Chesterton's Fence

Before removing code, explain why it exists.

Especially in this repo, be careful with:
- pane-tree logic
- xterm lifecycle code
- terminal registry behavior
- PTY buffering / attach behavior
- socket protocol compatibility
- settings migration / persistence behavior

If you cannot explain why something exists, you do not understand it well enough to delete it.

## Premature Abstraction

Prefer concrete code until you have multiple real examples.

- Do not extract frameworks after one example
- Do not introduce a new store/service/helper just because it feels cleaner
- Use the third real repetition as the point to consider abstraction

This is especially important in Svelte component composition and Rust backend service layers.

## Communication

- Be direct and concrete
- Do not say "you're absolutely right"
- Surface contradictions instead of silently choosing an interpretation
- Push back when there is concrete technical evidence that the requested path is unsafe or conflicts with stated goals

If the human's request is ambiguous and the blast radius is meaningful, stop and ask.

## Autonomy Boundaries

Before a significant decision, check:

```text
AUTONOMY CHECK:
- Confident this is what the human wants? [yes/no]
- If wrong, blast radius? [low/medium/high]
- Easily undone? [yes/no]
- Would the human want to know first? [yes/no]
```

Stop and ask the human when:
- requirements are ambiguous
- there are multiple valid approaches with real tradeoffs
- the repo is in unexpected state
- the action is destructive or hard to undo
- you are changing public contracts, stored data, or architectural direction

## Git Discipline

- Never use `git add .`
- Stage files intentionally
- Do not revert unrelated changes you did not make
- Treat worktrees, branches, and local repo state as user data

## Handoff Protocol

When stopping, leave a clear handoff:
1. what is done
2. what is in progress
3. what is blocked
4. what you recommend next
5. which files were touched

Clean handoffs matter because context decays and this repo crosses frontend, Rust backend, and local process state.
