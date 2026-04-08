# Watches Feature Design

## Overview

Watches are background monitors that track external state and notify the user when something changes. They run in the Rust backend, survive frontend reloads, and surface status through the UI via a dedicated collapsible side pane, session card indicators, and macOS/desktop notifications.

## Watch Types

### GitHub Actions
- Polls `gh run view --json` for a specific run or latest run matching a workflow + branch filter
- Captures structured detail:
  - Run status (queued, in_progress, completed)
  - Conclusion (success, failure, cancelled, etc.)
  - Individual job names + statuses + conclusions
  - Failed step name for broken jobs
  - Run URL (clickable link to open in browser)
- **Prerequisite checks**: on watch creation, verify `gh` CLI is installed and authenticated (`gh auth status`). If missing or unauthenticated, report error immediately rather than silently failing on first poll.

### HTTP Health Check
- HTTP GET to a URL on interval
- Checks response status code against expected (default 200)
- Captures response status code and response time (ms)

### Shell Command
- Runs a command via `tokio::process::Command`
- Checks exit code against expected (default 0)
- Captures stdout/stderr output (truncated to 64KB max)

### Task
- Runs a project task discovered by the existing task system (npm, Taskfile, Make, Just)
- Reuses the existing `TaskDefinition.command` field — the watch simply executes that resolved shell command (e.g. `"task build"`, `"npm run test"`, `"just lint"`) via `tokio::process::Command`
- No new task runner abstraction — watches treat tasks as shell commands with metadata
- Checks exit code, captures stdout/stderr output (truncated to 64KB max)

## Watch Modes

- **Recurring** — runs on a configurable interval (default varies by type: GH Actions 30s, HTTP 60s, shell 30s, task 30s)
- **One-shot** — runs once, notifies on completion, then transitions to `Stopped` state

## Data Model

### Rust

```rust
struct Watch {
    id: String,
    name: String,
    kind: WatchKind,
    mode: WatchMode,
    scope: WatchScope,
    runtime_state: RuntimeState,
    last_result: Option<WatchResult>,
    last_checked: Option<DateTime<Utc>>,
    notify: NotifyConfig,
    created_at: DateTime<Utc>,
}

enum WatchScope {
    Global,
    Session(String),       // session_id
    Project(String),       // project_id
}

enum RuntimeState {
    /// Watch created but not yet started (initial state on rehydration)
    Pending,
    /// Actively polling / running
    Active,
    /// User paused this watch
    Paused,
    /// One-shot completed or user stopped
    Stopped,
    /// Watch itself errored (e.g. gh CLI missing, network unreachable)
    Error(String),
}

enum WatchKind {
    GithubAction {
        repo: String,
        run_id: Option<u64>,          // specific run, or None to use filter
        workflow: Option<String>,      // workflow name/filename for filtering
        branch: Option<String>,
    },
    HttpHealth {
        url: String,
        expected_status: u16,          // default 200
    },
    ShellCommand {
        command: String,
        working_dir: Option<String>,
        success_exit_code: i32,        // default 0
    },
    Task {
        task_id: String,               // e.g. "npm:test", "taskfile:build", "just:lint"
        command: String,               // resolved command from TaskDefinition.command
        working_dir: String,
    },
}

enum WatchMode {
    Recurring { interval_secs: u64 },
    OneShot,
}

/// Result of the last check — typed per watch kind
enum WatchResult {
    GithubRun(GithubRunResult),
    HttpCheck(HttpCheckResult),
    CommandRun(CommandRunResult),
}

struct GithubRunResult {
    run_id: u64,
    status: String,                    // "queued", "in_progress", "completed"
    conclusion: Option<String>,        // "success", "failure", "cancelled", etc.
    url: String,
    jobs: Vec<GithubJob>,
    outcome: WatchOutcome,
}

struct GithubJob {
    name: String,
    status: String,
    conclusion: Option<String>,
    failed_step: Option<String>,
}

struct HttpCheckResult {
    status_code: u16,
    response_time_ms: u64,
    outcome: WatchOutcome,
}

struct CommandRunResult {
    exit_code: i32,
    stdout: String,                    // truncated to 64KB
    stderr: String,                    // truncated to 64KB
    outcome: WatchOutcome,
}

/// The semantic result of a check
enum WatchOutcome {
    Success,
    Failure,
    InProgress,                        // e.g. GH Action still running
}

struct NotifyConfig {
    desktop_notification: bool,        // default: true for failures
    on_failure: bool,                  // default: true
    on_success: bool,                  // default: false
}
```

### Frontend

Mirror of the Rust model in TypeScript, stored in a `watchState` writable store, synced via Tauri `watch-update` events.

```typescript
interface WatchUpdateEvent {
    watch: Watch;
    changed: boolean;                  // true if outcome changed since last check
    previous_outcome: WatchOutcome | null;
}
```

## Architecture

### Rust: WatchManager

- Lives in Tauri app state alongside `PtyManager` and `SessionStore`
- Owns `HashMap<String, WatchHandle>` where each handle holds a `tokio::JoinHandle` and a `tokio_util::sync::CancellationToken`
- Persistence: `dirs::config_dir()/roux/watches.json`, debounced write (500ms) matching session persistence pattern. Only persists watch config and last result — not runtime state (watches restart as `Pending` on app launch).

#### Scheduler Policy

- **Per-check timeout**: each check has a maximum execution time (default 30s for shell/task/GH, 10s for HTTP). If exceeded, the check is killed and the result is `Error`.
- **No overlap**: a new check does not start until the previous one completes (or times out). The interval timer starts *after* completion, not from the start of the previous check.
- **Cancellation**: pausing or removing a watch cancels any in-flight check via the `CancellationToken`. Child processes are killed (SIGTERM, then SIGKILL after 2s — matching existing PTY kill behavior).
- **Jittered intervals**: on startup when rehydrating multiple watches, each watch adds 0–5s random jitter to avoid thundering herd.
- **Output capping**: stdout/stderr stored in `WatchResult` is truncated to 64KB. Persisted output in `watches.json` is also capped at 64KB per watch.
- **Flap debouncing**: notifications are only sent on *state transitions* (e.g. Success → Failure), not on repeated same-state checks. If a watch flaps (alternates states within 60s), notifications are suppressed until the state is stable for at least 2 consecutive checks.

#### Startup Rehydration

On app launch:
1. Load `watches.json` from disk
2. All watches start in `Pending` state regardless of their previous runtime state
3. Recurring watches are re-spawned with jittered delays
4. One-shot watches that previously completed (`Stopped`) are *not* re-spawned — they remain stopped with their last result visible
5. Watches scoped to a session that no longer exists are cleaned up (removed from persistence)
6. Watches scoped to a project that no longer exists are cleaned up

### Tauri Commands

- `create_watch(watch_config) -> Watch`
- `remove_watch(watch_id)`
- `list_watches() -> Vec<Watch>`
- `pause_watch(watch_id)`
- `resume_watch(watch_id)`

### Notification Flow

1. Rust WatchManager detects outcome change (e.g. Success → Failure)
2. Checks flap debounce — skips notification if state is unstable
3. Emits `watch-update` Tauri event with `WatchUpdateEvent { watch, changed: true, previous_outcome }`
4. Frontend updates store, flashes session card if session-scoped
5. If `notify.desktop_notification` is true and the change matches notify config (on_failure, on_success), Rust sends desktop notification via `tauri-plugin-notification`
6. Desktop notification includes watch name, status summary, and action URL (e.g. GH run URL — click opens browser)

**Platform note**: `tauri-plugin-notification` works on macOS, Windows, and Linux. The spec uses "desktop notification" rather than "macOS notification" since Roux targets all platforms.

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
  - Full output/detail (job breakdown with per-job status for GH Actions, response code + time for HTTP, stdout/stderr for commands)
  - Clickable link to GH run URL (for GitHub Action watches)
  - Controls: pause/resume, remove, interval adjustment
- Badge count on the pane toggle button — count of watches with `Failure` outcome

### Session Card Integration

- When a watch outcome changes, the session card border/background briefly flashes with status color (green = success, red = failure, amber = in-progress)
- Small aggregate watch status dots on the card showing watch health for that session

### Watch Creation

- **Command palette**: "Add Watch" → pick type → configure (URL, command, repo, interval)
- **Context menu on task sidebar**: "Watch this task" on any task item — pre-fills with the task's `command` and `working_dir`
- **cmd+k in Claude Code**: natural language like "watch the CI on main" → creates watch pre-filled (deferred to a later phase — requires defining the Claude Code ↔ Roux integration protocol)
- **Terminal link detection**: xterm.js `registerLinkProvider` with custom patterns — when matched, a clickable decoration appears that triggers "create watch" flow pre-filled with extracted details

### Terminal Link Provider Patterns

Register link providers in xterm.js that detect watchable patterns in terminal output:

- GitHub Actions run URLs: `https://github.com/{owner}/{repo}/actions/runs/{id}`
- GitHub PR check URLs

**Precedence with WebLinksAddon**: The existing `WebLinksAddon` handles general URL clicking (open in browser). Watch link providers should be registered *after* `WebLinksAddon` so they take priority for matched patterns. For GitHub Actions URLs, the watch link provider intercepts the click to offer "Watch this run" instead of opening the browser. A secondary action (e.g. cmd+click or right-click) should still open the URL in the browser.

Other patterns can be added over time.

## Dependencies

### New Crate Dependencies (Rust)
- `reqwest` — HTTP client for health checks
- `tauri-plugin-notification` — desktop notifications (register in app builder)
- `tokio-util` — for `CancellationToken`
- Update `tokio` features to include `process` and `time`

### Tauri Plugin Registration
- Add `tauri_plugin_notification` to the app builder in `main.rs`
- Add notification capability to `tauri.conf.json`

### Frontend
- No new dependencies — uses existing xterm.js link provider API and Svelte store patterns

## Scoping

Watches use a `WatchScope` enum (not dual optional IDs):
- **Global**: not tied to any session or project, always visible
- **Session(id)**: tied to a session, grouped under that session in the pane, flash that session's card on change
- **Project(id)**: tied to a project, shown under the project group

### Lifecycle

- When a **session is deleted**, all watches scoped to that session are removed and cleaned from persistence
- When a **project is removed**, all watches scoped to that project are removed and cleaned from persistence
- On **app restart**, watches are rehydrated from persistence, scoped watches are validated against existing sessions/projects, and orphaned watches are cleaned up

## Error Handling

### GitHub Actions
- **Missing `gh` CLI**: detect on watch creation, set `RuntimeState::Error("gh CLI not found")`, surface in UI
- **Unauthenticated**: detect via `gh auth status` on creation, report error
- **Network offline**: check fails with timeout, watch stays `Active` with last known result, outcome set to `Failure`
- **Repo/workflow not found**: parse `gh` stderr, set `RuntimeState::Error` with descriptive message

### HTTP Health
- **Network unreachable**: timeout after 10s, outcome `Failure`
- **DNS resolution failure**: outcome `Failure` with error in result
- **TLS errors**: outcome `Failure` with error in result

### Shell/Task
- **Command not found**: exit code from shell, outcome `Failure`
- **Timeout exceeded**: process killed, outcome `Failure` with "(timed out)" in stderr
