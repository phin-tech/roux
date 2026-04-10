# Services Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract business logic from command handlers into reusable `services/*.rs` modules, making command handlers thin IPC adapters and eliminating socket.rs code duplication.

**Architecture:** Each service module contains pure functions that take concrete args (not Tauri types) and return `Result<T, anyhow::Error>`. Command handlers extract state and delegate. Socket handlers call the same service functions.

**Tech Stack:** Rust, Tauri 2, anyhow (already a dependency)

---

### Task 1: Create services/sessions.rs — session lifecycle

**Files:**
- Create: `src-tauri/src/services/mod.rs`
- Create: `src-tauri/src/services/sessions.rs`
- Modify: `src-tauri/src/commands/sessions.rs`
- Modify: `src-tauri/src/socket.rs`
- Modify: `src-tauri/src/main.rs` (add `mod services;`)

- [ ] **Step 1: Create `src-tauri/src/services/mod.rs`**

```rust
pub(crate) mod sessions;
pub(crate) mod worktrees;
pub(crate) mod projects;
pub(crate) mod setup;
pub(crate) mod docs;
pub(crate) mod settings;
```

- [ ] **Step 2: Create `src-tauri/src/services/sessions.rs` with `create_session` service function**

```rust
use anyhow::{anyhow, Context};

use crate::pty::PtyManager;
use crate::session::Session;
use crate::session_service::SessionHandle;
use crate::settings::RouxSettings;

pub(crate) fn is_git_repo(path: &str) -> bool {
    std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(crate) fn get_current_branch(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

pub(crate) async fn create_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    repo_path: &str,
    name: &str,
    worktree_path: Option<&str>,
    branch: Option<&str>,
    extra_flags: &[String],
    nono_profile: Option<&str>,
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory
    let (work_dir, actual_branch, is_wt) = if let Some(wt_path) = worktree_path {
        let br = branch
            .map(|b| b.to_string())
            .or_else(|| get_current_branch(wt_path))
            .unwrap_or_else(|| "main".to_string());
        (wt_path.to_string(), br, false)
    } else if let Some(br) = branch {
        let base = settings.worktree_base_path.as_deref();
        let wt_path = crate::worktree::create_worktree(repo_path, br, base)?;
        (wt_path, br.to_string(), true)
    } else {
        let br = get_current_branch(repo_path).unwrap_or_else(|| "main".to_string());
        (repo_path.to_string(), br, false)
    };

    // Merge settings flags with per-session extra flags
    let mut all_flags = settings.additional_flags.clone();
    all_flags.extend_from_slice(extra_flags);

    rlog!("Creating session '{}' (id={}) in '{}'", name, session_id, work_dir);
    rlog!(
        "  branch={}, flags={:?}, claude_binary={:?}",
        actual_branch,
        all_flags,
        settings.claude_binary_path
    );

    // Spawn PTY
    let spawn_result = pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &all_flags,
        nono_profile,
        settings.claude_binary_path.as_deref(),
        app.clone(),
    );

    if let Err(e) = spawn_result {
        rlog!("Session spawn failed: {}", e);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&work_dir);
        }
        return Err(anyhow!("{}", e));
    }
    rlog!("Session '{}' spawned successfully", session_id);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let session = Session {
        id: session_id,
        name: name.to_string(),
        repo_root: repo_path.to_string(),
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: "idle".to_string(),
        model: None,
        cost: None,
        created_at: now,
        project_id: None,
        is_git_repo: is_git_repo(repo_path),
    };

    if let Err(e) = session_handle.add(session.clone()).await {
        pty_manager.kill(&session.id);
        if is_wt {
            let _ = crate::worktree::remove_worktree(&session.worktree_path);
        }
        return Err(e.into());
    }
    Ok(session)
}

pub(crate) async fn reconnect_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    settings: &RouxSettings,
    id: &str,
    extra_flags: &[String],
    app: &tauri::AppHandle,
) -> anyhow::Result<Session> {
    let session = session_handle
        .get(id)
        .await?
        .ok_or_else(|| anyhow!("Session {} not found", id))?;

    pty_manager.kill(id);

    let mut all_flags = settings.additional_flags.clone();
    all_flags.extend_from_slice(extra_flags);

    rlog!("Reconnecting session '{}' (id={}) in '{}'", session.name, id, session.worktree_path);

    pty_manager
        .spawn(
            id,
            &session.worktree_path,
            settings.default_model.as_deref(),
            &all_flags,
            None,
            settings.claude_binary_path.as_deref(),
            app.clone(),
        )
        .map_err(|e| anyhow!("{}", e))?;

    session_handle.update_status(id, "idle").await?;

    rlog!("Session '{}' reconnected successfully", id);

    let mut updated = session;
    updated.status = "idle".to_string();
    Ok(updated)
}

pub(crate) async fn kill_session(
    pty_manager: &PtyManager,
    session_handle: &SessionHandle,
    id: &str,
) -> anyhow::Result<()> {
    pty_manager.kill(id);
    session_handle.remove(id).await?;
    Ok(())
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeSession {
    pub(crate) session_id: String,
    pub(crate) summary: String,
    pub(crate) modified_at: u64,
}

pub(crate) fn list_claude_sessions(cwd: &str) -> anyhow::Result<Vec<ClaudeSession>> {
    use std::io::BufRead;

    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot find home directory"))?;
    let projects_dir = home.join(".claude").join("projects");

    let encoded = cwd.replace('/', "-").replace('.', "-");
    let project_dir = projects_dir.join(&encoded);

    if !project_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in std::fs::read_dir(&project_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("jsonl") {
            continue;
        }
        let session_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let modified_at = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let summary = (|| -> Option<String> {
            let file = std::fs::File::open(&path).ok()?;
            let reader = std::io::BufReader::new(file);
            for line in reader.lines() {
                let line = line.ok()?;
                if !line.contains("\"type\":\"user\"") {
                    continue;
                }
                let val: serde_json::Value = serde_json::from_str(&line).ok()?;
                let content = val.get("message")?.get("content")?;
                if let Some(s) = content.as_str() {
                    return Some(s.chars().take(120).collect());
                }
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                return Some(text.chars().take(120).collect());
                            }
                        }
                    }
                }
                return None;
            }
            None
        })()
        .unwrap_or_default();

        sessions.push(ClaudeSession { session_id, summary, modified_at });
    }

    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(sessions)
}
```

- [ ] **Step 3: Add `mod services;` to main.rs**

Add after `mod state;`:
```rust
mod services;
```

- [ ] **Step 4: Rewrite `commands/sessions.rs` as thin adapters calling service functions**

```rust
use crate::state::AppState;
use crate::session::Session;
use crate::services::sessions as svc;

#[tauri::command]
pub(crate) fn write_to_session(id: String, data: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn resize_session(id: String, cols: u16, rows: u16, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn attach_pty_output(id: String, on_event: tauri::ipc::Channel<tauri::ipc::Response>, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.attach_output_channel(&id, on_event);
    Ok(())
}

#[tauri::command]
pub(crate) fn spawn_shell(id: String, working_dir: String, state: tauri::State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    state.pty_manager.spawn_shell(&id, &working_dir, None, app.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn spawn_task(id: String, command: String, working_dir: String, state: tauri::State<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    state.pty_manager.spawn_task(&id, &command, &working_dir, None, app.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn get_pty_generation(id: String, state: tauri::State<AppState>) -> Option<u64> {
    state.pty_manager.get_generation(&id)
}

#[tauri::command]
pub(crate) async fn kill_session(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    svc::kill_session(&state.pty_manager, &state.session_handle, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn create_session(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    extra_flags: Option<Vec<String>>,
    nono_profile: Option<String>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let flags = extra_flags.unwrap_or_default();
    svc::create_session(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &repo_path,
        &name,
        worktree_path.as_deref(),
        branch.as_deref(),
        &flags,
        nono_profile.as_deref(),
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn reconnect_session(
    id: String,
    extra_flags: Option<Vec<String>>,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let flags = extra_flags.unwrap_or_default();
    svc::reconnect_session(
        &state.pty_manager,
        &state.session_handle,
        &settings,
        &id,
        &flags,
        &app,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn list_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<Session>, String> {
    state.session_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn refresh_session_git_status(id: String, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let handle = state.session_handle.clone();
    let session = handle.get(&id).await.map_err(|e| e.to_string())?;
    if let Some(s) = session {
        let is_git = svc::is_git_repo(&s.worktree_path);
        if is_git != s.is_git_repo {
            handle.set_git_repo(&id, is_git).await.map_err(|e| e.to_string())?;
        }
        Ok(is_git)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub(crate) fn check_is_git_repo(path: String) -> bool {
    svc::is_git_repo(&path)
}

#[tauri::command]
pub(crate) fn list_claude_sessions(cwd: String) -> Result<Vec<svc::ClaudeSession>, String> {
    svc::list_claude_sessions(&cwd).map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Update socket.rs to use service functions**

Replace `handle_session_create` body to call `crate::services::sessions::create_session`.  
Replace `crate::commands::sessions::is_git_repo` → `crate::services::sessions::is_git_repo`.  
Replace `crate::commands::sessions::get_current_branch` → `crate::services::sessions::get_current_branch`.

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test`  
Expected: compiles clean, 67 tests pass

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/services/ src-tauri/src/commands/sessions.rs src-tauri/src/socket.rs src-tauri/src/main.rs
git commit -m "refactor: extract session lifecycle into services/sessions.rs (#17)"
```

---

### Task 2: Create services/worktrees.rs

**Files:**
- Create: `src-tauri/src/services/worktrees.rs`
- Modify: `src-tauri/src/commands/worktrees.rs`

- [ ] **Step 1: Create `src-tauri/src/services/worktrees.rs`**

```rust
use anyhow::anyhow;

pub(crate) fn list_branches(repo_path: &str) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| anyhow!("Failed to list branches: {}", e))?;
    if !output.status.success() {
        return Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

pub(crate) fn git_init(path: &str) -> anyhow::Result<()> {
    let output = std::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .map_err(|e| anyhow!("Failed to run git init: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}
```

- [ ] **Step 2: Rewrite `commands/worktrees.rs` to delegate**

```rust
use crate::state::AppState;
use crate::services::worktrees as svc;

#[tauri::command]
pub(crate) fn cmd_create_worktree(repo_path: String, branch: String, state: tauri::State<AppState>) -> Result<String, String> {
    let settings = state.settings.lock().unwrap();
    let base_path = settings.worktree_base_path.as_deref();
    crate::worktree::create_worktree(&repo_path, &branch, base_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_remove_worktree(worktree_path: String) -> Result<(), String> {
    crate::worktree::remove_worktree(&worktree_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_list_worktrees(repo_path: String) -> Result<Vec<crate::worktree::Worktree>, String> {
    crate::worktree::list_worktrees(&repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn cmd_list_branches(repo_path: String) -> Result<Vec<String>, String> {
    svc::list_branches(&repo_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn git_init(path: String) -> Result<(), String> {
    svc::git_init(&path).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/worktrees.rs src-tauri/src/commands/worktrees.rs
git commit -m "refactor: extract worktree helpers into services/worktrees.rs (#17)"
```

---

### Task 3: Create services/projects.rs

**Files:**
- Create: `src-tauri/src/services/projects.rs`
- Modify: `src-tauri/src/commands/projects.rs`

- [ ] **Step 1: Create `src-tauri/src/services/projects.rs`**

```rust
use std::path::PathBuf;

pub(crate) fn notes_path(project_id: &str) -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux").join("notes").join(format!("{}.txt", project_id))
}

pub(crate) fn get_notes(project_id: &str) -> anyhow::Result<String> {
    let path = notes_path(project_id);
    if path.exists() {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Ok(String::new())
    }
}

pub(crate) fn set_notes(project_id: &str, content: &str) -> anyhow::Result<()> {
    let path = notes_path(project_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}
```

- [ ] **Step 2: Rewrite `commands/projects.rs` to delegate notes operations**

```rust
use crate::projects::Project;
use crate::state::AppState;
use crate::services::projects as svc;

#[tauri::command]
pub(crate) fn list_projects(state: tauri::State<AppState>) -> Vec<Project> {
    state.project_store.list()
}

#[tauri::command]
pub(crate) fn create_project(name: String, state: tauri::State<AppState>) -> Project {
    let project = Project { id: uuid::Uuid::new_v4().to_string(), name };
    state.project_store.add(project.clone());
    project
}

#[tauri::command]
pub(crate) fn remove_project(id: String, state: tauri::State<AppState>) {
    state.project_store.remove(&id);
}

#[tauri::command]
pub(crate) fn rename_project(id: String, name: String, state: tauri::State<AppState>) {
    state.project_store.rename(&id, &name);
}

#[tauri::command]
pub(crate) async fn set_session_project(session_id: String, project_id: Option<String>, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.session_handle.set_project(&session_id, project_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn get_project_notes(project_id: String) -> Result<String, String> {
    svc::get_notes(&project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn set_project_notes(project_id: String, content: String) -> Result<(), String> {
    svc::set_notes(&project_id, &content).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/projects.rs src-tauri/src/commands/projects.rs
git commit -m "refactor: extract project notes into services/projects.rs (#17)"
```

---

### Task 4: Create services/setup.rs

**Files:**
- Create: `src-tauri/src/services/setup.rs`
- Modify: `src-tauri/src/commands/setup.rs`

- [ ] **Step 1: Create `src-tauri/src/services/setup.rs`**

```rust
pub(crate) fn is_command_available(command: &str) -> bool {
    let user_path = crate::pty::get_user_path();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    std::process::Command::new(&shell)
        .args(["-c", &format!("command -v {}", command)])
        .env("PATH", &user_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(crate) fn list_nono_profiles() -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let profiles_dir = home.join(".config").join("nono").join("profiles");
    if !profiles_dir.is_dir() {
        return Vec::new();
    }
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                profiles.push(name.to_string());
            }
        }
    }
    profiles.sort();
    profiles
}
```

- [ ] **Step 2: Rewrite `commands/setup.rs` to delegate**

```rust
use crate::services::setup as svc;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    cli_installed: bool,
    gh_available: bool,
}

#[tauri::command]
pub(crate) fn check_setup_status() -> SetupStatus {
    SetupStatus {
        cli_installed: crate::hooks::cli_is_installed(),
        gh_available: svc::is_command_available("gh"),
    }
}

#[tauri::command]
pub(crate) fn check_setup_needed() -> bool {
    !crate::hooks::cli_is_installed()
}

#[tauri::command]
pub(crate) fn run_setup() -> Result<(), String> {
    crate::hooks::install_hooks().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn check_nono_installed() -> bool {
    svc::is_command_available("nono")
}

#[tauri::command]
pub(crate) fn list_nono_profiles() -> Vec<String> {
    svc::list_nono_profiles()
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/setup.rs src-tauri/src/commands/setup.rs
git commit -m "refactor: extract setup checks into services/setup.rs (#17)"
```

---

### Task 5: Create services/docs.rs

**Files:**
- Create: `src-tauri/src/services/docs.rs`
- Modify: `src-tauri/src/commands/docs.rs`

- [ ] **Step 1: Create `src-tauri/src/services/docs.rs`**

```rust
use std::path::Path;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocFile {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) relative_path: String,
    pub(crate) modified: u64,
}

pub(crate) fn list_docs(dir: &str) -> anyhow::Result<Vec<DocFile>> {
    let base = Path::new(dir);
    if !base.is_dir() {
        return Err(anyhow::anyhow!("Not a directory: {}", dir));
    }

    let skip_dirs: std::collections::HashSet<&str> =
        ["node_modules", ".git", "target", "dist", ".svelte-kit", ".superpowers"]
            .iter()
            .copied()
            .collect();

    let mut docs = Vec::new();
    let mut stack = vec![base.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !skip_dirs.contains(name) {
                        stack.push(path);
                    }
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let modified = path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let relative =
                    path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();

                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

                docs.push(DocFile {
                    path: path.to_string_lossy().to_string(),
                    name,
                    relative_path: relative,
                    modified,
                });
            }
        }
    }

    docs.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(docs)
}
```

- [ ] **Step 2: Rewrite `commands/docs.rs` as thin adapters**

```rust
use crate::services::docs as svc;

#[tauri::command]
pub(crate) fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
pub(crate) fn write_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, &contents).map_err(|e| format!("Failed to write file: {}", e))
}

#[tauri::command]
pub(crate) fn list_docs(dir: String) -> Result<Vec<svc::DocFile>, String> {
    svc::list_docs(&dir).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/docs.rs src-tauri/src/commands/docs.rs
git commit -m "refactor: extract doc listing into services/docs.rs (#17)"
```

---

### Task 6: Create services/settings.rs

**Files:**
- Create: `src-tauri/src/services/settings.rs`
- Modify: `src-tauri/src/commands/settings.rs`

- [ ] **Step 1: Create `src-tauri/src/services/settings.rs`**

```rust
use crate::settings::RouxSettings;

pub(crate) fn update_settings(new_settings: RouxSettings) -> anyhow::Result<RouxSettings> {
    let settings = new_settings.normalized();
    crate::logging::set_enabled(settings.enable_logging);
    crate::settings::save_settings(&settings)?;
    Ok(settings)
}
```

- [ ] **Step 2: Rewrite `commands/settings.rs` to delegate**

```rust
use crate::state::AppState;
use crate::services::settings as svc;
use tauri::Emitter;

#[tauri::command]
pub(crate) fn get_settings(state: tauri::State<AppState>) -> crate::settings::RouxSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub(crate) fn update_settings(
    settings: crate::settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let settings = svc::update_settings(settings).map_err(|e| e.to_string())?;
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/settings.rs src-tauri/src/commands/settings.rs
git commit -m "refactor: extract settings update into services/settings.rs (#17)"
```

---

### Task 7: Update socket.rs to use session service and final cleanup

**Files:**
- Modify: `src-tauri/src/socket.rs`

- [ ] **Step 1: Rewrite `handle_session_create` in socket.rs to use service**

Replace the duplicated PTY spawn + session store logic with a call to `crate::services::sessions::create_session`. The socket handler still needs to resolve `repo_path` from `working_dir` or the requesting session, then delegate.

Key changes:
- Replace lines 179-221 with a call to `crate::services::sessions::create_session`
- Update `is_git_repo`/`get_current_branch` imports to `crate::services::sessions::`

- [ ] **Step 2: Build and test**

Run: `cargo build && cargo test`

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/socket.rs
git commit -m "refactor: deduplicate socket session create via services (#17)"
```

---

### Task 8: Final build, test, and PR

- [ ] **Step 1: Full build and test**

Run: `cargo build && cargo test`  
Expected: compiles clean, 67 tests pass

- [ ] **Step 2: Create PR**

```bash
git push -u origin refactor/split-command-handlers
gh pr create --title "refactor: extract business logic into services modules" --body "..."
```
