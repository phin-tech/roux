# Services Extraction Design

## Problem

Command handlers in `commands/*.rs` contain business logic directly. Socket handlers in `socket.rs` duplicate session creation logic. Neither is testable without the Tauri runtime.

## Solution

Extract business logic into `services/*.rs` modules. Command handlers become thin IPC adapters. Socket handlers call the same service functions.

## Module Layout

```
services/
  mod.rs
  sessions.rs   — create/reconnect/kill session, Claude session discovery
  worktrees.rs   — worktree CRUD, branch listing, git init, git helpers
  projects.rs    — project CRUD, notes, session-project linking
  setup.rs       — CLI detection, nono profiles, gh availability
  docs.rs        — file read/write, doc listing
  settings.rs    — settings load/save/update
```

## Pattern

Service functions take concrete args (not `tauri::State`), return `Result<T, anyhow::Error>`.

```rust
// services/sessions.rs
pub(crate) async fn create_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str, name: &str,
    worktree_path: Option<&str>, branch: Option<&str>,
    extra_flags: &[String], nono_profile: Option<&str>,
    app: &tauri::AppHandle,
) -> Result<Session, anyhow::Error> { ... }
```

Command handlers extract state, call service, convert error:

```rust
#[tauri::command]
pub(crate) async fn create_session(..., state: State<'_, AppState>, app: AppHandle) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::services::sessions::create_session(
        &state.pty_manager, &state.session_handle, &settings, ...
    ).await.map_err(|e| e.to_string())
}
```

## Error Handling

- Service functions: `anyhow::Error` for composition
- Command handlers: `.map_err(|e| e.to_string())` for Tauri IPC
- Socket handlers: convert to `Response::err(format!("{}", e))`

## Socket Deduplication

`handle_session_create` in socket.rs calls `services::sessions::create_session` instead of reimplementing PTY spawn + session store logic.

## What Stays in Commands

- Extracting args from `tauri::State`
- Merging per-call overrides with settings
- Error conversion to String
- Tauri-specific annotations (`#[tauri::command]`)

## Testing

Service functions are testable without Tauri runtime. `PtyManager` and `SessionHandle` can be constructed directly in tests.
