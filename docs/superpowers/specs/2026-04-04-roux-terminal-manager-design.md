# Roux — Multi-Session Claude Code Terminal Manager

**Date:** 2026-04-04
**Status:** Draft
**Stack:** Tauri v2 + Svelte 5 + Bits UI + Tailwind + xterm.js

## Overview

Roux is a desktop application for managing multiple concurrent Claude Code terminal sessions with native git worktree integration. It provides a vertical-tabbed interface where each tab is an isolated Claude Code instance running in its own PTY, optionally in its own git worktree.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Window                         │
├──────────┬──────────────────────────────────────────────┤
│          │                                              │
│  Vertical│         Active Terminal (xterm.js)           │
│  Session │                                              │
│  Tabs    │  $ claude                                    │
│          │  Claude Code v2.1.92                         │
│  ┌────┐  │  > working on feature...                    │
│  │ S1 │  │                                              │
│  ├────┤  │                                              │
│  │ S2 │  │                                              │
│  ├────┤  │                                              │
│  │ S3 │  │                                              │
│  ├────┤  │                                              │
│  │ +  │  │                                              │
│  └────┘  │                                              │
│          │                                              │
│  ⚙ gear │                                              │
├──────────┴──────────────────────────────────────────────┤
│  Status: session name · branch · model · $0.05          │
└─────────────────────────────────────────────────────────┘
```

### Project Structure

```
roux/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs           # Tauri app entry, setup, state
│   │   ├── pty.rs            # PTY session management
│   │   ├── session.rs        # Session state, persistence
│   │   ├── worktree.rs       # Git worktree operations
│   │   ├── settings.rs       # Settings read/write
│   │   └── ipc.rs            # Tauri command handlers
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── lib/
│   │   ├── components/
│   │   │   ├── Layout.svelte
│   │   │   ├── SessionTabs.svelte
│   │   │   ├── SessionCard.svelte
│   │   │   ├── Terminal.svelte
│   │   │   ├── StatusBar.svelte
│   │   │   ├── NewSessionDialog.svelte
│   │   │   └── SettingsPanel.svelte
│   │   └── stores/
│   │       ├── sessions.ts
│   │       └── settings.ts
│   ├── App.svelte
│   ├── main.ts
│   └── app.css               # Tailwind base
├── package.json
├── svelte.config.js
├── tailwind.config.js
├── vite.config.ts
└── tsconfig.json
```

## Rust Backend

### PTY Manager

Manages a `HashMap<String, PtySession>` mapping session IDs to live PTY processes.

```rust
struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send>,
    size: (u16, u16),  // cols, rows
}
```

- Uses the `portable-pty` crate for cross-platform PTY handling.
- Each PTY spawns a reader thread that reads output bytes and emits them to the frontend via Tauri events.
- Output is emitted as raw bytes to the frontend. The reader thread also scans for OSC escape sequences to detect status changes (see Status Detection), but does not modify or intercept the output stream.

### Tauri Commands (Frontend → Backend)

| Command | Parameters | Returns | Purpose |
|---|---|---|---|
| `create_session` | `repo_path: String, name: String, worktree_path: Option<String>, branch: Option<String>` | `Session` | Atomic session creation. If `branch` is provided and `worktree_path` is None, creates a worktree first (using `worktreeBasePath` from settings), then spawns claude in it. If worktree creation succeeds but claude spawn fails, the worktree is automatically cleaned up. |
| `kill_session` | `id: String` | `()` | Terminate claude process (SIGTERM on Unix, TerminateProcess on Windows), clean up PTY. |
| `write_to_session` | `id: String, data: Vec<u8>` | `()` | Send keystrokes/input to a session's PTY. |
| `resize_session` | `id: String, cols: u16, rows: u16` | `()` | Resize PTY dimensions. |
| `list_sessions` | — | `Vec<Session>` | Return all session metadata. |
| `get_settings` | — | `Settings` | Read current settings. |
| `update_settings` | `settings: Settings` | `()` | Write settings to disk, emit `settings-changed` event. |
| `create_worktree` | `repo_path: String, branch: String` | `String` | Run `git worktree add`, return worktree path. |
| `remove_worktree` | `worktree_path: String` | `()` | Run `git worktree remove`. |
| `list_worktrees` | `repo_path: String` | `Vec<Worktree>` | Run `git worktree list --porcelain`, return parsed list. |

### Tauri Events (Backend → Frontend)

| Event | Payload | Purpose |
|---|---|---|
| `pty-output:{id}` | `String` (base64-encoded bytes) | Stream terminal output to the frontend. Base64 is used because Tauri event payloads are JSON-serialized; the frontend decodes to `Uint8Array` before writing to xterm.js. |
| `session-status:{id}` | `{ status: String, model: Option<String>, cost: Option<f64> }` | Emitted when the backend detects a status change via OSC parsing. Updates session metadata. |
| `session-exit:{id}` | `{ code: Option<i32> }` | Notify when a claude process exits. |
| `settings-changed` | `Settings` | Broadcast settings update to all frontend components. |

### Git Worktree Integration

Git operations shell out to the `git` CLI rather than using `git2` crate, for simplicity and to match the user's git configuration (hooks, credentials, aliases).

```rust
struct Worktree {
    path: String,
    branch: String,
    is_main: bool,  // true if this is the main working tree
}
```

**Worktree lifecycle:**
- `create_worktree(repo, branch)` → runs `git worktree add <path> <branch>` (existing branch) or `git worktree add -b <branch> <path>` (new branch — auto-detected by checking if branch exists). The worktree path is determined by `settings.worktreeBasePath`:
  - If set: `{worktreeBasePath}/{repo_name}-{sanitized_branch}` (branch names sanitized: `/` → `-`, invalid path chars stripped)
  - If null: git default (adjacent to repo)
  - Collision handling: if target path exists, append `-2`, `-3`, etc.
- `remove_worktree(path)` → runs `git worktree remove <path>`
- `list_worktrees(repo)` → runs `git worktree list --porcelain`, parses output

## Data Model

### Session

```typescript
interface Session {
  id: string                // UUID
  name: string              // user-defined label
  repoRoot: string          // the main git repository path
  worktreePath: string      // where claude actually runs (may equal repoRoot)
  branch: string            // git branch for this session
  isWorktree: boolean       // true if Roux created the worktree via git worktree add
  status: 'idle' | 'thinking' | 'generating' | 'error' | 'disconnected'
  model: string | null       // parsed from Claude Code's OSC title sequence, e.g. "Opus 4.6 (1M)". Null until first detection.
  cost: number | null        // parsed from Claude Code's OSC title sequence. Null until first detection.
  createdAt: number         // unix timestamp
}
```

### Settings

```typescript
interface RouxSettings {
  // Layout
  tabPosition: 'left' | 'right'
  tabWidth: number                    // pixels, default 260

  // Terminal
  fontSize: number                    // default 14
  fontFamily: string                  // default "Berkeley Mono, JetBrains Mono, monospace"
  lineHeight: number                  // default 1.2
  scrollback: number                  // max scrollback lines, default 5000
  cursorStyle: 'block' | 'underline' | 'bar'
  cursorBlink: boolean

  // Sessions
  defaultProjectPath: string | null
  confirmOnClose: boolean             // default true. Only prompts when session is active (thinking/generating).
  restoreSessionsOnLaunch: boolean    // default true

  // Worktrees
  worktreeBasePath: string | null     // custom worktree location, null = git default
  cleanupWorktreesOnClose: boolean    // auto-remove without prompting, default false

  // Theme
  theme: 'dark'                       // V1: dark only. Field exists for forward-compat; V2 adds 'light' | 'system'.

  // Claude
  defaultModel: string | null
  additionalFlags: string[]           // extra CLI flags passed to every claude invocation
}
```

## Frontend Components

### Layout.svelte

Top-level layout shell. Uses flexbox with configurable `flex-direction` to place the tab sidebar on the left or right. The tab sidebar width is adjustable via a drag handle. Reads `tabPosition` from the settings store.

### SessionTabs.svelte

Vertical scrollable list of `SessionCard` components. A "+" button at the bottom opens `NewSessionDialog`. A gear icon at the bottom opens `SettingsPanel`.

### SessionCard.svelte

Compact card for each session displaying:
- **Status indicator dot** — color-coded:
  - Green: idle
  - Amber (pulsing): thinking
  - Blue (streaming animation): generating
  - Red: error
  - Gray: disconnected
- **Session name** — editable on double-click
- **Branch name** — displayed below the name
- **Project path** — truncated, shown as tooltip on hover
- **Cost** — running total, e.g. "$0.12"
- **Close button** — visible on hover

The active session card has a highlighted background.

### Terminal.svelte

Wraps xterm.js. One xterm.js `Terminal` instance is created per session on first focus. Key behaviors:

- **Attach/detach pattern:** When switching sessions, the active terminal's DOM element is detached from the container and the new session's terminal is attached. No re-rendering — instant switch with full scrollback preserved.
- **Input:** Captures `onData` from xterm.js and sends via `write_to_session` Tauri command.
- **Output:** Listens to `pty-output:{id}` Tauri events and writes raw bytes to the xterm instance.
- **Resize:** Uses `ResizeObserver` on the container element. On resize, calls `xterm.fit()` via the fit addon, then sends new dimensions via `resize_session`.
- **Status updates:** Receives `session-status:{id}` events from the backend (which parses OSC sequences in the PTY reader thread) and updates the session store. The frontend does NOT parse OSC sequences itself.

**xterm.js addons:**
- `@xterm/addon-webgl` — GPU-accelerated rendering
- `@xterm/addon-fit` — auto-fit terminal to container size
- `@xterm/addon-web-links` — clickable URLs in terminal output

### StatusBar.svelte

Bottom bar showing the active session's metadata: session name, branch, model, cost, and status text.

### NewSessionDialog.svelte

Modal dialog (Bits UI `Dialog`) for creating a new session:

1. **Repository picker** — Tauri native directory dialog to select a git repo
2. **Mode selection:**
   - "New worktree" — text input for branch name, Roux creates the worktree
   - "Existing directory" — pick an existing worktree or the main working tree (populated via `list_worktrees`)
3. **Session name** — optional text input, defaults to `{repo}-{branch}`
4. **Create button** — triggers `create_session`

### SettingsPanel.svelte

Slides in as an overlay panel (or replaces the terminal view). Uses Bits UI primitives for all controls. Changes apply immediately with debounced writes to disk.

**V1 settings UI controls:**

| Setting | Control |
|---|---|
| Tab position | Toggle: left / right |
| Worktree base path | Directory picker + text field |
| Cleanup worktrees on close | Toggle |
| Default project path | Directory picker + text field |
| Font size | Number input / slider |
| Font family | Text field |
| Scrollback lines | Number input |
| Confirm on close | Toggle |
| Restore sessions on launch | Toggle |
| Default model | Text field |
| Additional CLI flags | Text field |

**JSON-only settings (no V1 UI):**
- `tabWidth` — adjusted via drag handle
- `lineHeight`, `cursorStyle`, `cursorBlink` — power-user tweaks in JSON
- `theme` — V1 ships dark only

### State Management

Two Svelte stores using `writable`:

**`sessions.ts`:**
```typescript
interface SessionStore {
  sessions: Session[]
  activeSessionId: string | null
}
```

**`settings.ts`:**
```typescript
// Writable<RouxSettings> initialized from get_settings() on app load
// Subscribes to settings-changed events for live updates
```

## Session Lifecycle

### Creating a Session

1. User clicks "+" → `NewSessionDialog` opens
2. User selects a git repo, chooses "new worktree" or "existing directory"
3. Frontend calls `create_session(repo_path, name, worktree_path?, branch?)`
4. Backend handles atomically:
   - If `branch` is provided and no `worktree_path`: creates worktree via `git worktree add -b <branch>` (creates new branch) or `git worktree add <path> <branch>` (existing branch). Uses `worktreeBasePath` from settings. Branch names are sanitized for filesystem safety (slashes → dashes, e.g. `feature/auth` → `feature-auth`).
   - If worktree creation succeeds but claude spawn fails: worktree is automatically removed (rollback)
   - Spawns `claude` (with any `additionalFlags` and `defaultModel` from settings) via PTY in the target directory
5. Session added to store, tab appears, terminal attaches

### Switching Sessions

- User clicks a tab
- Active terminal DOM element is detached
- Selected session's terminal DOM element is attached
- Instant switch — no re-rendering, full scrollback preserved

### Closing a Session

1. User clicks X on a tab (or keyboard shortcut)
2. If `confirmOnClose` is true and session is active (thinking/generating): show confirmation dialog
3. Send SIGTERM to the claude process via `kill_session`
4. If `isWorktree` is true:
   - If `cleanupWorktreesOnClose` is true: automatically run `remove_worktree`
   - Otherwise: prompt "Also remove the worktree?"
5. Remove session from store and persisted state

### Persistence Across Restarts

- Session metadata is saved to `~/.config/roux/sessions.json` on every state change (debounced)
- On launch, if `restoreSessionsOnLaunch` is true:
  - Previous sessions are restored in the tab list with status `disconnected`
  - A "Reconnect" action on each card creates a new session (new ID, fresh PTY) in the same directory, replacing the disconnected entry
  - Scrollback is NOT restored (Claude Code's `/resume` handles conversation continuity)

## Status Detection

Claude Code sets the terminal title via OSC escape sequences that include status information. Roux parses these sequences from the raw PTY output stream:

- OSC sequences follow the pattern `\x1b]0;...\x07` or `\x1b]0;...\x1b\\`
- The title string contains status indicators that map to our status enum
- This avoids any need to parse Claude Code's actual conversation protocol

The parser runs in the Rust backend's PTY reader thread. When a status change, model, or cost update is detected, the backend emits a `session-status:{id}` event with the parsed values. The frontend updates its session store from these events — it never parses OSC sequences itself.

**Parsed fields from OSC title:**
- `status` — mapped from Claude Code's title indicators to `idle | thinking | generating`
- `model` — extracted from the title string (e.g. "Opus 4.6 (1M)")
- `cost` — extracted from the title string (e.g. "$0.12")

If Claude Code changes its title format, only the Rust parser needs updating.

## Technology Choices

| Layer | Choice | Rationale |
|---|---|---|
| Desktop shell | Tauri v2 | Small binary (~2MB), native OS integration, mature ecosystem |
| Frontend framework | Svelte 5 | Reactive, minimal boilerplate, good Tauri integration |
| UI primitives | Bits UI | Headless components, full styling control via Tailwind |
| CSS | Tailwind CSS | Utility-first, fast iteration, consistent design |
| Terminal emulator | xterm.js | Battle-tested (used by VS Code), full ANSI support, WebGL rendering |
| PTY management | `portable-pty` | Cross-platform PTY handling in Rust |
| Git operations | `git` CLI (shelled out) | Respects user's git config, hooks, credentials |
| Config location | `~/.config/roux/` | XDG-compliant on macOS/Linux |

## Deferred to V2+

- **File navigator sidebar** — tree view of the active session's project files
- **Theme system** — light mode, system-follow, custom themes
- **Keybindings customization** — configurable keyboard shortcuts
- **Toolkit / quick actions panel** — buttons for Create PR, Commit & Push, Worktree, etc.
- **Session grouping** — group tabs by project/repo
- **Split terminal view** — view two sessions side by side
