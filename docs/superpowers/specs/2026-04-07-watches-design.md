# Watches Feature Design

## Overview

Watches are background monitors that track external state and notify the user when something changes. They run in the Rust backend, survive frontend reloads, and surface status through the UI via a dedicated collapsible side pane, session card indicators, and macOS notifications.

## Watch Types

### GitHub Actions
- Polls `gh run view` for a specific run or latest run matching a workflow + branch filter
- Captures structured detail:
  - Run status (queued, in_progress, completed)
  - Conclusion (success, failure, cancelled, etc.)
  - Individual job names + statuses + conclusions
  - Failed step name for broken jobs
  - Run URL (clickable link to open in browser)

### HTTP Health Check
- HTTP GET to a URL on interval
- Checks response status code against expected (default 200)
- Captures response time and status

### Shell Command
- Runs a command via `tokio::process::Command`
- Checks exit code against expected (default 0)
- Captures stdout/stderr output

### Task
- Runs a project task (Taskfile, npm script, Makefile target)
- Uses the same task runner infrastructure as the task sidebar
- Checks exit code, captures output

## Watch Modes

- **Recurring** — runs on a configurable interval (default varies by type: GH Actions 30s, HTTP 60s, shell 30s, task 30s)
- **One-shot** — runs once, notifies on completion, then stops

## Data Model

### Rust

```rust
struct Watch {
    id: String,
    name: String,
    kind: WatchKind,
    mode: WatchMode,
    session_id: Option<String>,  // None = global
    project_id: Option<String>,
    status: WatchStatus,         // Pending, Running, Success, Failure, Error
    last_checked: Option<DateTime<Utc>>,
    last_output: Option<WatchOutput>,
    notify: NotifyConfig,
    created_at: DateTime<Utc>,
}

enum WatchKind {
    GithubAction {
        repo: String,
        run_id_or_filter: RunFilter,  // specific run ID or workflow + branch
        branch: Option<String>,
    },
    HttpHealth {
        url: String,
        expected_status: u16,
    },
    ShellCommand {
        command: String,
        working_dir: Option<String>,
        success_exit_code: i32,
    },
    Task {
        task_runner: String,       // "task", "npm", "make"
        task_name: String,
        working_dir: Option<String>,
    },
}

enum WatchMode {
    Recurring { interval_secs: u64 },
    OneShot,
}

enum WatchOutput {
    GithubRun(GithubRunDetail),
    Plain(String),
}

struct GithubRunDetail {
    run_id: u64,
    status: String,
    conclusion: Option<String>,
    url: String,
    jobs: Vec<GithubJob>,
}

struct GithubJob {
    name: String,
    status: String,
    conclusion: Option<String>,
    failed_step: Option<String>,
}

struct NotifyConfig {
    macos_notification: bool,  // default: true for failures
    on_failure: bool,          // default: true
    on_success: bool,          // default: false
}
```

### Frontend

Mirror of the Rust model in TypeScript, stored in a `watchState` writable store, synced via Tauri `watch-update` events.

## Architecture

### Rust: WatchManager

- Lives in Tauri app state alongside `PtyManager` and `SessionStore`
- Owns `HashMap<String, WatchHandle>` — each handle holds a `tokio::JoinHandle` and a `CancellationToken`
- On watch creation, spawns a tokio task per watch:
  - Executes the check (shell/gh via `tokio::process::Command`, HTTP via `reqwest`)
  - Compares new status to previous — if changed, sets `changed: true` on the event
  - Emits `watch-update` Tauri event with full watch state
  - Sleeps for interval (recurring) or exits (one-shot)
- Persistence: `~/.config/roux/watches.json`, debounced write (500ms) matching session persistence pattern

### Tauri Commands

- `create_watch(watch_config) -> Watch`
- `remove_watch(watch_id)`
- `list_watches() -> Vec<Watch>`
- `pause_watch(watch_id)`
- `resume_watch(watch_id)`

### Notification Flow

1. Rust WatchManager detects state change (e.g. Success → Failure)
2. Emits `watch-update` event with `changed: true`
3. Frontend updates store, flashes session card if session-scoped
4. If `notify.macos_notification` is true and the change matches notify config (on_failure, on_success), Rust sends macOS notification via `tauri-plugin-notification`
5. macOS notification includes watch name, status summary, and action URL (e.g. GH run URL — click opens browser)

## UI

### Watches Pane

- Collapsible side pane, toggled like the existing notes pane
- Grouped by session/project, with a "Global" section at top for unscoped watches
- Each watch row:
  - Status dot (green/red/amber) with pulse animation for in-progress
  - Watch name
  - Last checked timestamp
  - Expand arrow
- Expanded view:
  - Full output/detail (job breakdown for GH Actions, response code for HTTP, stdout for commands)
  - Clickable link to GH run URL (for GitHub Action watches)
  - Controls: pause/resume, remove, interval adjustment
- Badge count on the pane toggle button — count of watches in Failure/Error state

### Session Card Integration

- When a watch changes state, the session card border/background briefly flashes with status color (green = success, red = failure, amber = in-progress)
- Small aggregate watch status dots on the card showing watch health for that session

### Watch Creation

- **Command palette**: "Add Watch" → pick type → configure (URL, command, repo, interval)
- **Context menu on task sidebar**: "Watch this task" on any task item
- **cmd+k in Claude Code**: natural language like "watch the CI on main" → creates watch pre-filled
- **Terminal regex capture**: xterm.js `registerLinkProvider` with custom patterns (GitHub Actions URLs, etc.) — when matched, a clickable decoration appears that triggers "create watch" flow pre-filled with extracted details

### Terminal Link Provider Patterns

Register link providers in xterm.js that detect watchable patterns in terminal output:

- GitHub Actions run URLs: `https://github.com/{owner}/{repo}/actions/runs/{id}`
- GitHub PR check URLs
- Other patterns can be added over time

When a match is detected, xterm renders it as a clickable link. Clicking opens the watch creation flow with type and details pre-filled from the URL.

## Dependencies

### New Crate Dependencies (Rust)
- `reqwest` — HTTP client for health checks
- `tauri-plugin-notification` — macOS desktop notifications
- `tokio` (already present) — async task spawning

### Frontend
- No new dependencies — uses existing xterm.js link provider API and Svelte store patterns

## Scoping

Watches can be:
- **Session-scoped**: tied to a session, grouped under that session in the pane, flash that session's card on change
- **Project-scoped**: tied to a project, shown under the project group
- **Global**: no session/project association, shown in a "Global" group at top of pane
