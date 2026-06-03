# Notification Service And Pane

## Summary

Roux needs a single notification subsystem that all sources (Claude/Codex hooks, watches, task lifecycle, external `roux notify` calls, and later OSC terminal sequences) feed into. A Rust-side `NotificationService` owns the store and policy; the frontend subscribes via an event stream and renders badges plus a dedicated notifications pane. This replaces the current inline Allow/Always/Deny buttons on the session card (which only work for Claude's binary permission prompt and can't handle N-choice `AskUserQuestion` pickers) with a notify-only flow: Roux tells the user something needs attention, the user focuses the session and drives the provider's own TUI.

The v1 scope is the service, the ingress paths, the event stream, the frontend store, and a minimal pane. The inline permission buttons are removed. A richer per-pane UI (rings, inline pickers) is explicitly deferred until the service has been in use long enough to inform what the pane should actually look like.

## Context And Motivation

- The existing session card has Allow / Always / Deny buttons that send raw keystrokes (`\r`, `\x1b[Z`, `\x1b[B\x1b[B\r`) into the PTY to answer Claude's permission prompt. This is a binary-choice hack: it assumes Claude has the "Yes" option pre-highlighted and does not generalize to `AskUserQuestion`-style N-choice prompts, which Roux currently has no way to answer from outside the terminal.
- Watches already fire OS notifications via `tauri-plugin-notification` from `services/watches/manager.rs`. Attention events from the Claude hook bridge do not — they only update `sessionState.permissionInfo` and rely on the inline buttons.
- The in-flight Codex support plan (`docs/plans/2026-04-10-codex-cli-support-via-services-commands.md`) already sketches `services/notifications.rs` as a small policy layer that suppresses notifications while the Roux window is focused. This document extends that stub into a full service with a store, event stream, and multiple ingress paths, in a way that plan can adopt without rework.
- cmux's model (researched 2026-04-10) is the reference UX: one store, many display surfaces (sidebar badge, per-session inline message, ⌘I panel, macOS desktop notification, ⌘⇧U "jump to most recent unread"), multiple ingest paths (OSC 9/99/777 from PTY output, `cmux notify` CLI, hooks), grouped by workspace, with read/unread state and no severity taxonomy. Roux adopts the shape but adds a lightweight severity enum because our sources are more heterogeneous.

## Goals

- One authoritative notification store in Rust, one event stream to the frontend.
- All current notification-ish events funnel through it: hook attention, hook session-done, watch success/failure, task completion, external CLI.
- A new `roux notify` subcommand that any shell script or agent hook can use to push a notification into a running Roux.
- Frontend: a notifications pane (cmux-style inbox), unread badge on the sidebar, click-through to focus the relevant session.
- OS-notification policy lives in exactly one place (the service), gated by window focus and per-source settings.
- Remove the inline Allow/Always/Deny buttons from the session card; keep the amber "attention" dot as a presence indicator only.
- Parse OSC 9 / 99 / 777 notification escape sequences out of PTY output and route them through the same service, so any TUI that already emits these (and `roux notify` as a thin wrapper) gets a consistent experience.
- Ship notification actions in v1, not just focus-session click-through — see the Actions section for the v1 surface and open questions.

## Non-Goals

- No inline `AskUserQuestion` picker in v1. The service makes the user aware; the user drives the PTY.
- No cross-restart persistence. Notifications are ephemeral; on app restart the inbox is empty. Watches and tasks re-emit their own state on startup, which covers the "still broken" case.
- No cross-device sync.

## Data Model

```rust
// roux-core or src-tauri/src/services/notifications/model.rs

pub struct Notification {
    pub id: NotificationId,              // ulid, monotonically ordered
    pub created_at: SystemTime,
    pub level: NotificationLevel,
    pub source: NotificationSource,
    pub title: String,
    pub subtitle: Option<String>,         // OSC 99 `p=subtitle`, otherwise optional free-form
    pub body: Option<String>,
    pub session_id: Option<String>,      // None = global (not tied to a session)
    pub read: bool,
    pub actions: Vec<NotificationAction>, // see Actions section; first with primary=true is row-click target
}

pub enum NotificationLevel {
    Info,
    Success,
    Attention,   // user action expected (permission prompt, AskUserQuestion, etc.)
    Warning,
    Error,
}

pub enum NotificationSource {
    Hook { provider: String },                 // "claude" | "codex" | ...
    Watch { watch_id: String },
    Task { pane_id: String },
    Cli,                                       // `roux notify`
    Osc { code: u16, sender_id: Option<String> }, // OSC 9 / 99 / 777; sender_id = OSC 99 `i=` value
    Internal,                                  // generated by Roux itself
}
```

Severity drives both styling in the pane and the default OS-notification policy (attention/warning/error fire by default; info/success are quiet unless the source opts in).

## Service Architecture

`src-tauri/src/services/notifications/`:

- `mod.rs` — `NotificationService` with `Arc<RwLock<Store>>` and a `tokio::sync::broadcast` channel.
- `store.rs` — in-memory ring buffer (cap ~500) of `Notification`, plus per-session unread counts.
- `events.rs` — event enum emitted on the broadcast channel: `Added`, `Read`, `ReadAll`, `Removed`.
- `policy.rs` — decides whether an incoming notification also fires an OS notification (focus-gated + per-source settings).

Public API:

```rust
impl NotificationService {
    pub fn new(app: AppHandle) -> Self;

    // Ingress
    pub async fn push(&self, req: NotificationRequest) -> NotificationId;

    // Queries
    pub fn list(&self, filter: ListFilter) -> Vec<Notification>;
    pub fn unread_count(&self, session_id: Option<&str>) -> usize;

    // Mutations from UI
    pub fn mark_read(&self, id: NotificationId);
    pub fn mark_all_read(&self, session_id: Option<&str>);
    pub fn remove(&self, id: NotificationId);
    pub fn clear(&self, session_id: Option<&str>);

    // Subscribe (returns broadcast Receiver)
    pub fn subscribe(&self) -> broadcast::Receiver<NotificationEvent>;
}
```

`NotificationRequest` is the write-side struct without `id` / `created_at` / `read` (filled in by the store).

## Ingress Paths

All of these land in `NotificationService::push`:

1. **Hook bridge** — `cli.rs::handle_hook` currently writes a JSON status file; extend it so that `attention`, and terminal states (`idle`/`error` after a working turn), also land on the notification service via the socket bridge. Provider is supplied by the hook payload (`claude` | `codex`).
2. **Watches** — `services/watches/manager.rs` today calls `app.notification().builder()` directly. Replace with `notifications.push(..)`; the policy layer inside the service decides whether to fan out to the OS.
3. **Task lifecycle** — command-pane PTY exits currently only flip UI state. Add a push with `level = Success|Error` on exit.
4. **`roux notify` CLI** — see CLI section below.
5. **Internal** — direct Rust calls (e.g. "connection to socket lost", "codex hooks install failed").
6. **OSC escape sequences in PTY output** — parsed inside the PTY output pipeline before bytes are forwarded to xterm. See the OSC Parsing section below.

## OSC Parsing (v1)

cmux exposes three OSC notification protocols that already have broad TUI support; we mirror exactly the same set so any script that works with cmux works with Roux unchanged.

### Wire formats

- **OSC 9** (iTerm2 growl — body only):

  ```
  ESC ] 9 ; <body> BEL
  ```

  Single string, no title. Maps to `Notification { title: <body>, body: None, level: Info }`.

- **OSC 777** (rxvt `notify;title;body`):

  ```
  ESC ] 777 ; notify ; <title> ; <body> BEL
  ```

  Semicolon-delimited, fixed positional fields. Maps to `Notification { title, body, level: Info }`.

- **OSC 99** (kitty desktop notification protocol — rich):
  ```
  ESC ] 99 ; <params> ; <payload> ST
  ```
  `<params>` is a `k=v:k=v:…` list with the relevant keys being `i` (notification id — used for dedupe/update, namespaced per-PTY), `e` (payload encoding, `0`=utf8, `1`=base64), `d` (done flag for chunked messages, `0`=more, `1`=last), `p` (part — `title | subtitle | body | icon` etc.). Multiple OSC 99 packets with the same `i` compose one notification, with `p=title`, `p=subtitle`, and `p=body` landing in the corresponding `Notification` fields directly. Maps to `Notification { title, subtitle, body, level, source: Osc { code: 99, sender_id: Some(<i>) } }`; subsequent packets with the same `i` update the existing notification in place rather than creating a new one.

Both `BEL` (`\x07`) and `ST` (`ESC \`) terminators are accepted for all three.

### Parser location and plumbing

- Parser lives in a new module `src-tauri/src/services/notifications/osc_parser.rs`, wrapped around the existing PTY output thread in `pty.rs`. The PTY thread feeds bytes into a `vte::Parser` (the alacritty VT crate — battle-tested, streaming, handles BEL and ST terminators and partial reads). We implement `vte::Perform::osc_dispatch` to match on the three OSC codes above and emit parsed events.
- Bytes are forwarded to xterm unchanged, even if an OSC notification was extracted — we are non-consuming. OSC 9/777 are invisible to xterm anyway (it ignores OSC codes it doesn't know), and OSC 99 is likewise a no-op for xterm in our config.
- Each PTY in Roux already has a known `ptyId` → `sessionId` mapping; the parser passes that as the `session_id` on the resulting `NotificationRequest`. OSC notifications are therefore always session-scoped.
- OSC 99 `i=` IDs are namespaced per PTY to avoid collisions when two sessions both use `i=1`.
- `NotificationSource` gains an `Osc { code: u16, sender_id: Option<String> }` variant.

### `roux notify` relation to OSC

`roux notify` is the friendly, structured path (arguments, severity, actions, cwd resolution). OSC is the zero-dependency fallback for scripts that can't assume `roux` is on `$PATH`. Internally `roux notify` may choose to emit an OSC 99 sequence _as well_ when run inside a Roux PTY, so that `tee`'d logs and session recordings retain the notification markers — but the primary path is still the socket.

## Actions

### v1 surface

Every notification may carry zero or more actions. The service stores them; the pane renders them as buttons (or a menu if overflow); clicking invokes the action via a single Tauri command.

```rust
pub struct NotificationAction {
    pub id: String,              // stable within the notification, used as the click target
    pub label: String,
    pub kind: ActionKind,
    pub primary: bool,           // if true, also fired by row click / Enter key in the pane
}

pub enum ActionKind {
    FocusSession { session_id: String },
    FocusPane { pane_id: String },
    OpenUrl { url: String },
    OpenPath { path: String },               // file:// → opener plugin
    RunCommand { command_id: String },       // frontend command registry entry (args: see Open Questions)
    RetryWatch { watch_id: String },         // re-fire a watch; specialized so it's available in v1
    Dismiss,                                 // removes this notification
    DismissSource { source: NotificationSource }, // removes all from the same source
    MarkRead,                                // explicit in case primary isn't mark-read
}
```

Default actions when the source doesn't specify any:

- Hook attention → `[FocusSession(primary), Dismiss]`
- Watch failure → `[FocusSession(primary), RetryWatch, DismissSource]`
- Watch success → `[FocusSession(primary), Dismiss]` (retry is only useful on failure)
- Task success → `[FocusPane(primary), Dismiss]`
- Task error → `[FocusPane(primary), Dismiss]` (rerun is already available from the pane UI; not duplicated here)
- OSC 9/777 (no id) → `[FocusSession(primary), Dismiss]`
- OSC 99 with `i=` → `[FocusSession(primary), Dismiss]`, plus the notification is _updatable/removable_ by subsequent OSC 99 packets with the same id.

### Deliberately excluded from v1

- `RunShellCommand { argv }` — arbitrary shell exec from a notification is a foot-gun; only pre-registered frontend commands via `RunCommand { command_id }` are allowed.
- Inline reply / inline `AskUserQuestion` answering — still deferred.
- Per-action confirmation dialogs — if the action needs a confirm, it's the frontend command's job to handle it.

## Event Stream To Frontend

New Tauri event `notification://event` (or similar) carrying `NotificationEvent`. A Specta-generated type shared with the frontend via the existing bindings pipeline, so the frontend store and the Rust broadcast channel stay in lockstep.

Frontend:

- `src/lib/stores/notifications.ts` — writable store mirroring the Rust side.
- On app start: call `notifications.list()` once to hydrate, then subscribe.
- Derived stores: `unreadBySession: Map<string, number>`, `totalUnread: number`.

## OS-Notification Policy

Single place: `services/notifications/policy.rs`.

Rules (v1):

- If the Roux window is focused, suppress OS notifications entirely (in-pane badge is enough).
- If unfocused:
  - `Attention | Warning | Error` → always fire.
  - `Info | Success` → fire only if the source opted in (watch config already has `desktop_notification`; CLI gets a `--os` flag; default off for low-severity).
- Respect a new top-level setting `notificationsEnabled` (kill switch) and existing watch-level `notify.desktop_notification`.

Focus detection: Tauri's `Window::is_focused()` checked at push time. If we want edge-triggered behavior (e.g., "I stepped away and came back, show what I missed") that's v2.

## Frontend: Pane And Surfaces

Reference UX is cmux:

- **Notifications pane** — dedicated sidebar pane (like Watches/Notes), opened via command palette + keybinding. Lists notifications newest-first, grouped by session with "Global" as a top group for `session_id = None`. Each row: level dot, title, body snippet, source badge, timestamp, dismiss button. Click row = click_action (focus session/pane) + mark read.
- **Session sidebar badge** — small unread count on each session tab in `SessionTabs`.
- **Global unread pill** on the notifications pane toggle in the status bar.
- **Remove inline Allow/Deny** from `SessionCard` and the `session.approve` command. Keep the amber status dot as a presence indicator; the notification pane is where the user goes to see details.
- **`AskUserQuestion` handling** — for now, it's just a notification like any other. User clicks the notification → focuses the session → answers in Claude's TUI picker. A future iteration may parse the options and render a picker in the pane, but not in v1.

Keybindings (match vim-nav conventions in the keynav design if it lands first):

- `g n` — toggle notifications pane
- `g u` — jump to most recent unread (cmux's ⌘⇧U)
- In pane: `j/k` row nav, `Enter` = click, `x` = dismiss, `d` = mark all read

## CLI: `roux notify`

New subcommand in `roux-cli`. Usage:

```
roux notify \
  --level info|success|attention|warning|error \
  --title "Build done" \
  [--body "cargo build succeeded in 14s"] \
  [--session <id>]          # explicit session
  [--cwd <path>]            # fall back to session-by-cwd
  [--source <string>]       # free-form source tag for filtering
  [--os]                    # also fire OS notification even if level is info/success
  [--click focus|pane:<id>|url:<url>]
```

Also accepts `--json -` / stdin for hook-like wiring:

```json
{
  "level": "attention",
  "title": "...",
  "body": "...",
  "cwd": "/path",
  "source": "codex"
}
```

Session resolution order: `--session` id → `--cwd` lookup → env `ROUX_SESSION_ID` → global (unattached). Global notifications render in the pane under a "Global" group.

Transport: the existing Unix socket to the running Roux app. Unlike `roux hook` (which writes to a status file that the app's status watcher picks up — this exists so the CLI works even if the app is running but socket is momentarily unavailable), `roux notify` is interactive and should ack synchronously. If the socket is not listening, `roux notify` prints a warning to stderr and exits non-zero; it does not fall back to a file drop. Scripts that want fire-and-forget can `|| true`.

Keep `roux hook` as-is for the existing Claude hook install contract; internally `handle_hook` gains a branch that also routes into the socket for richer notification payloads.

## Public Interfaces (Rust → TS)

- New Tauri commands: `notifications_list`, `notifications_mark_read`, `notifications_mark_all_read`, `notifications_remove`, `notifications_clear`, `notifications_push` (used by frontend for test/demo; production pushes come from Rust).
- New event: `notifications://event` with Specta-typed payload.
- Removed: `session.approve` command and its substeps; `SessionCard` `onapprove`/`onalways`/`ondeny` props.

## Rollout Phases

1. **Phase 1 — skeleton service with actions**
   - Create `services/notifications/` with store, event stream, actions model, Tauri commands, Specta types.
   - Wire watches to push through the service instead of calling `tauri-plugin-notification` directly. Verify watch notifications still work end-to-end with the new default actions (`FocusSession`, `DismissSource`).
2. **Phase 2 — frontend store and minimal pane**
   - `stores/notifications.ts`, list + subscribe.
   - A simple notifications pane component behind a command-palette entry, rendering actions as buttons with primary-action row click.
   - Sidebar unread badge.
3. **Phase 3 — retire the inline approve flow**
   - Remove `session.approve`, `SessionCard` Allow/Deny buttons, and `handleApprove/handleAlways/handleDeny` in `SessionTabs`.
   - Hook bridge pushes an `Attention` notification on permission prompts instead, with `FocusSession` as the primary action.
4. **Phase 4 — `roux notify` CLI**
   - Subcommand, socket transport, cwd resolution, `--action` flags.
5. **Phase 5 — OSC parsing in PTY pipeline**
   - `vte::Parser` wrapper around the PTY output thread.
   - OSC 9 / 777 / 99 dispatch into `NotificationService::push`.
   - OSC 99 id-based update/dismiss.
   - Tests driven by `scripts/notify_probe.sh`-style fixtures.
6. **Phase 6 — policy + focus gating**
   - Focus-aware OS fan-out, per-source settings.
7. **Deferred — rings-per-pane, inline pickers, cross-restart persistence, shell-exec actions.**

## Test Plan

- Rust unit tests
  - `Store::push` enforces ring-buffer cap and ULID ordering.
  - `unread_count` by session and global.
  - `mark_all_read` scoping.
  - `policy` decision matrix: (focused, level, source opts, global enable) → fire/suppress.
  - OSC parser: OSC 9 body-only, OSC 777 `notify;title;body`, OSC 99 single packet, OSC 99 with `p=title` + `p=subtitle` + `p=body` all resolving into the right fields, OSC 99 chunked (`d=0` then `d=1`), OSC 99 update by `i=`, BEL vs ST terminator, partial-read byte splitting mid-sequence.
  - Actions: default action set per source, `Dismiss` removes exactly one, `DismissSource` removes all from the matching source variant.
- Rust integration tests
  - Watch manager push routes through `NotificationService` with the expected default actions.
  - Hook bridge `attention` status produces a notification with the right session match (session-id and cwd fallback) and `FocusSession` primary action.
  - `roux notify` CLI end-to-end against a test socket, including `--action` flags.
  - OSC sequences written into a fake PTY reach the service and produce notifications scoped to the right session.
- Frontend store tests
  - Hydration from `notifications_list`.
  - Event stream mutations apply correctly (including OSC 99 updates in place).
  - Derived unread counts.
  - Action button rendering and primary-action row-click wiring.
- Manual verification
  - Switching focus away and back shows OS notification for attention, not for info.
  - Clicking a notification in the pane focuses the session.
  - Removing the inline Allow/Deny buttons doesn't break existing permission flow — user answers in the TUI.
  - `roux notify` from inside and outside a worktree lands in the right place.
  - `printf '\e]777;notify;Hi;Body\a'` from a shell inside any Roux PTY produces a notification.

## Open Questions

1. **Notification grouping in the pane** — by session only, or also by source (e.g., "Watches: 3 failures")? cmux keeps it flat; we might want collapsible source groups once we have watches + tasks + attention mixed.
2. **Rate limiting** — a runaway script calling `roux notify` or looping OSC 9 could flood the pane. Soft cap per source per second?
3. **Cross-session dedup** — if the same watch fires twice in 10s, is it one notification with a count, or two? cmux is flat. I'd say two for v1, simpler. Note that OSC 99 already handles its own dedup via `i=` — this question is about non-OSC sources.
4. **Interaction with the keynav design** — if keynav ships first, we reuse its region model for the pane; if this ships first, we pick keybindings provisionally and reconcile later.
5. **`RunCommand` arguments** — should notification actions be able to fire `command.id` + `args`, or only bare command ids? Arguments would let a single generic action target many things; bare ids mean one registered command per target. Watch retry is already handled by a dedicated `RetryWatch` variant so this is no longer blocking v1, but it affects how future actions are modeled.
6. **Keyboard navigation on the primary action** — does `Enter` on a selected notification row fire its primary action and `Space` mark it read, or do we reserve a different chord? Waiting on the keynav design to land before committing.

## Assumptions

- Specta bindings are the source of truth for shared types (consistent with `fix(bindings)` from 2026-04-10).
- The socket bridge (`src-tauri/src/socket.rs`) is reliable enough to be the primary transport for `roux notify`.
- `tauri-plugin-notification` remains the OS-notification primitive; this service wraps it, not replaces it.
- `vte` (the alacritty VT parser crate) is the chosen OSC parser. It handles streaming, partial reads, BEL/ST terminators, and is already `no_std`-friendly. Adds one dependency to `src-tauri/Cargo.toml`.
- Parsing OSC notifications non-consumingly (letting the bytes continue through to xterm) is safe because xterm ignores OSC codes it does not recognize, and 9/777/99 are all in that set for our config.
- The existing session-card "attention" visual treatment (amber dot) is good enough as a presence indicator after the inline buttons are removed.
