# Roux Terminal Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a desktop app that manages multiple concurrent Claude Code terminal sessions with vertical tabs, git worktree integration, and a settings panel.

**Architecture:** Tauri v2 Rust backend manages PTY processes and git worktrees. Svelte 5 frontend renders xterm.js terminals in a tabbed layout. Communication flows through Tauri IPC commands (frontend→backend) and events (backend→frontend, for PTY output streaming and status updates).

**Tech Stack:** Tauri v2, Svelte 5, Bits UI, Tailwind CSS v4, xterm.js 6 (@xterm/xterm), portable-pty 0.9, Rust

**Note on Bits UI:** Bits UI is installed as a dependency (Task 1) and available for use. The initial implementation uses custom Tailwind-styled components that match the prototype aesthetic. These can be incrementally migrated to Bits UI primitives (Dialog, Switch, Select, etc.) for better accessibility and keyboard handling after the core functionality works. The custom components follow the same visual design, so migration is cosmetic, not structural.

**Spec:** `docs/superpowers/specs/2026-04-04-roux-terminal-manager-design.md`
**Prototype:** `prototype/index.html`

---

## File Map

```
roux/
├── src-tauri/
│   ├── Cargo.toml                    # Rust deps: tauri, portable-pty, serde, serde_json, uuid, base64
│   ├── tauri.conf.json               # Tauri window config, permissions
│   ├── capabilities/
│   │   └── default.json              # Tauri v2 capability permissions
│   └── src/
│       ├── main.rs                   # Tauri app entry, state setup, command/event registration
│       ├── pty.rs                    # PtyManager: spawn, write, resize, kill PTY sessions
│       ├── osc.rs                    # OSC escape sequence parser for status/model/cost
│       ├── session.rs                # Session struct, SessionStore, persistence to disk
│       ├── worktree.rs               # Git worktree create/remove/list (shells out to git CLI)
│       ├── settings.rs               # RouxSettings struct, read/write ~/.config/roux/settings.json
│       └── ipc.rs                    # All #[tauri::command] handlers
├── src/
│   ├── app.css                       # Tailwind import + CSS variables from prototype
│   ├── index.html                    # Vite HTML entry (plain Svelte, not SvelteKit)
│   ├── main.ts                       # Svelte app mount
│   ├── App.svelte                    # Root component
│   ├── lib/
│   │   ├── types.ts                  # TypeScript interfaces: Session, RouxSettings, Worktree
│   │   ├── stores/
│   │   │   ├── sessions.ts           # Svelte writable store for sessions + activeSessionId
│   │   │   └── settings.ts           # Svelte writable store for RouxSettings
│   │   ├── tauri.ts                  # Typed wrappers around invoke() and listen()
│   │   └── components/
│   │       ├── Layout.svelte         # Top-level flexbox shell (sidebar + terminal + statusbar)
│   │       ├── SessionTabs.svelte    # Sidebar: session list + footer buttons
│   │       ├── SessionCard.svelte    # Individual session tab card
│   │       ├── Terminal.svelte       # xterm.js wrapper with attach/detach
│   │       ├── StatusBar.svelte      # Bottom status bar
│   │       ├── NewSessionDialog.svelte  # New session modal (repo picker, worktree mode)
│   │       └── SettingsPanel.svelte  # Slide-in settings overlay
├── package.json
├── svelte.config.js
├── vite.config.ts
└── tsconfig.json
```

---

### Task 1: Project Scaffolding

**Files:**
- Create: `package.json`, `svelte.config.js`, `vite.config.ts`, `tsconfig.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src/main.ts`, `src/App.svelte`, `src/app.html`, `src/app.css`

- [ ] **Step 1: Scaffold Tauri + Svelte project**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm create tauri-app@latest . -- --template svelte --manager npm
```

Select Svelte, TypeScript. If the tool asks about directory not being empty, choose to proceed (the existing `docs/` and `prototype/` dirs are fine).

- [ ] **Step 2: Install frontend dependencies**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm install @xterm/xterm @xterm/addon-webgl @xterm/addon-fit @xterm/addon-web-links bits-ui
npm install -D tailwindcss @tailwindcss/vite
```

- [ ] **Step 3: Configure Tailwind v4 via Vite plugin**

In `vite.config.ts`, add the Tailwind plugin:

```typescript
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
```

- [ ] **Step 4: Set up app.css with design tokens from prototype**

Replace `src/app.css` contents with:

```css
@import "tailwindcss";

@theme {
  --color-bg-deep: #0a0a0c;
  --color-bg-base: #0f1014;
  --color-bg-surface: #161821;
  --color-bg-elevated: #1c1e2a;
  --color-bg-hover: #222436;
  --color-bg-active: #282b40;
  --color-border: #2a2d3e;
  --color-border-subtle: #1e2030;
  --color-text-primary: #c8cad8;
  --color-text-secondary: #6e7191;
  --color-text-muted: #464960;
  --color-accent: #7aa2f7;
  --color-accent-dim: #3d5a9e;
  --color-green: #9ece6a;
  --color-green-dim: #3a5a1e;
  --color-amber: #e0af68;
  --color-amber-dim: #6e5530;
  --color-red: #f7768e;
  --color-red-dim: #6e2a36;
  --color-blue: #7dcfff;
  --color-blue-dim: #2a5a6e;
  --color-gray: #545878;
  --font-mono: 'IBM Plex Mono', monospace;
  --font-sans: 'Outfit', sans-serif;
}

@layer base {
  body {
    font-family: var(--font-sans);
    background: var(--color-bg-deep);
    color: var(--color-text-primary);
    height: 100vh;
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
  }
}
```

- [ ] **Step 5: Add Google Fonts to index.html**

Edit `index.html` (Vite entry point at project root — plain Svelte template, NOT SvelteKit) to include the font link in `<head>`:

```html
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=Outfit:wght@300;400;500;600&display=swap" rel="stylesheet">
```

- [ ] **Step 6: Add Rust dependencies to Cargo.toml**

Add to `[dependencies]` in `src-tauri/Cargo.toml`:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
portable-pty = "0.9"
base64 = "0.22"
dirs = "6"
```

- [ ] **Step 7: Set up minimal main.rs**

Replace `src-tauri/src/main.rs` with:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 8: Configure Tauri window**

In `src-tauri/tauri.conf.json`, **merge** the following into the existing scaffold config (do NOT replace the whole file — keep existing top-level fields like `build`, `bundle`, `identifier`):

```json
{
  "app": {
    "windows": [
      {
        "title": "Roux",
        "width": 1200,
        "height": 800,
        "minWidth": 800,
        "minHeight": 500,
        "decorations": true,
        "transparent": false
      }
    ]
  }
}
```

- [ ] **Step 9: Replace App.svelte with placeholder**

Replace `src/App.svelte`:

```svelte
<script lang="ts">
</script>

<main class="h-screen flex flex-col bg-bg-deep text-text-primary">
  <p class="m-auto font-mono text-text-muted">roux — loading...</p>
</main>
```

- [ ] **Step 10: Verify build**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm run tauri dev
```

Expected: Tauri window opens with "roux — loading..." centered text. Close the window.

- [ ] **Step 11: Initialize git and commit**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
git init
echo ".superpowers/" >> .gitignore
git add -A
git commit -m "feat: scaffold Tauri v2 + Svelte 5 project with Tailwind and deps"
```

---

### Task 2: TypeScript Types and Tauri IPC Wrappers

**Files:**
- Create: `src/lib/types.ts`, `src/lib/tauri.ts`

- [ ] **Step 1: Create shared TypeScript types**

Create `src/lib/types.ts`:

```typescript
export interface Session {
  id: string;
  name: string;
  repoRoot: string;
  worktreePath: string;
  branch: string;
  isWorktree: boolean;
  status: "idle" | "thinking" | "generating" | "error" | "disconnected";
  model: string | null;
  cost: number | null;
  createdAt: number;
}

export interface RouxSettings {
  tabPosition: "left" | "right";
  tabWidth: number;
  fontSize: number;
  fontFamily: string;
  lineHeight: number;
  scrollback: number;
  cursorStyle: "block" | "underline" | "bar";
  cursorBlink: boolean;
  defaultProjectPath: string | null;
  confirmOnClose: boolean;
  restoreSessionsOnLaunch: boolean;
  worktreeBasePath: string | null;
  cleanupWorktreesOnClose: boolean;
  theme: "dark";
  defaultModel: string | null;
  additionalFlags: string[];
}

export interface Worktree {
  path: string;
  branch: string;
  isMain: boolean;
}

export interface SessionStatusPayload {
  status: string;
  model: string | null;
  cost: number | null;
}

export const DEFAULT_SETTINGS: RouxSettings = {
  tabPosition: "left",
  tabWidth: 260,
  fontSize: 14,
  fontFamily: "IBM Plex Mono, monospace",
  lineHeight: 1.2,
  scrollback: 5000,
  cursorStyle: "block",
  cursorBlink: true,
  defaultProjectPath: null,
  confirmOnClose: true,
  restoreSessionsOnLaunch: true,
  worktreeBasePath: null,
  cleanupWorktreesOnClose: false,
  theme: "dark",
  defaultModel: null,
  additionalFlags: [],
};
```

- [ ] **Step 2: Create Tauri IPC wrappers**

Create `src/lib/tauri.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Session,
  RouxSettings,
  Worktree,
  SessionStatusPayload,
} from "./types";

// Commands (frontend → backend)
export async function createSession(
  repoPath: string,
  name: string,
  worktreePath: string | null,
  branch: string | null
): Promise<Session> {
  return invoke("create_session", {
    repoPath,
    name,
    worktreePath,
    branch,
  });
}

export async function killSession(id: string): Promise<void> {
  return invoke("kill_session", { id });
}

export async function writeToSession(
  id: string,
  data: string
): Promise<void> {
  return invoke("write_to_session", { id, data });
}

export async function resizeSession(
  id: string,
  cols: number,
  rows: number
): Promise<void> {
  return invoke("resize_session", { id, cols, rows });
}

export async function listSessions(): Promise<Session[]> {
  return invoke("list_sessions");
}

export async function getSettings(): Promise<RouxSettings> {
  return invoke("get_settings");
}

export async function updateSettings(
  settings: RouxSettings
): Promise<void> {
  return invoke("update_settings", { settings });
}

export async function createWorktree(
  repoPath: string,
  branch: string
): Promise<string> {
  return invoke("cmd_create_worktree", { repoPath, branch });
}

export async function removeWorktree(
  worktreePath: string
): Promise<void> {
  return invoke("remove_worktree", { worktreePath });
}

export async function listWorktrees(
  repoPath: string
): Promise<Worktree[]> {
  return invoke("list_worktrees", { repoPath });
}

// Events (backend → frontend)
export function onPtyOutput(
  sessionId: string,
  callback: (data: string) => void
): Promise<UnlistenFn> {
  return listen<string>(`pty-output:${sessionId}`, (event) => {
    callback(event.payload);
  });
}

export function onSessionStatus(
  sessionId: string,
  callback: (payload: SessionStatusPayload) => void
): Promise<UnlistenFn> {
  return listen<SessionStatusPayload>(
    `session-status:${sessionId}`,
    (event) => {
      callback(event.payload);
    }
  );
}

export function onSessionExit(
  sessionId: string,
  callback: (code: number | null) => void
): Promise<UnlistenFn> {
  return listen<{ code: number | null }>(
    `session-exit:${sessionId}`,
    (event) => {
      callback(event.payload.code);
    }
  );
}

export function onSettingsChanged(
  callback: (settings: RouxSettings) => void
): Promise<UnlistenFn> {
  return listen<RouxSettings>("settings-changed", (event) => {
    callback(event.payload);
  });
}
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/types.ts src/lib/tauri.ts
git commit -m "feat: add TypeScript types and Tauri IPC wrappers"
```

---

### Task 3: Rust Settings Module

**Files:**
- Create: `src-tauri/src/settings.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Create settings.rs**

Create `src-tauri/src/settings.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouxSettings {
    pub tab_position: String,
    pub tab_width: u32,
    pub font_size: u32,
    pub font_family: String,
    pub line_height: f64,
    pub scrollback: u32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub default_project_path: Option<String>,
    pub confirm_on_close: bool,
    pub restore_sessions_on_launch: bool,
    pub worktree_base_path: Option<String>,
    pub cleanup_worktrees_on_close: bool,
    pub theme: String,
    pub default_model: Option<String>,
    pub additional_flags: Vec<String>,
}

impl Default for RouxSettings {
    fn default() -> Self {
        Self {
            tab_position: "left".to_string(),
            tab_width: 260,
            font_size: 14,
            font_family: "IBM Plex Mono, monospace".to_string(),
            line_height: 1.2,
            scrollback: 5000,
            cursor_style: "block".to_string(),
            cursor_blink: true,
            default_project_path: None,
            confirm_on_close: true,
            restore_sessions_on_launch: true,
            worktree_base_path: None,
            cleanup_worktrees_on_close: false,
            theme: "dark".to_string(),
            default_model: None,
            additional_flags: vec![],
        }
    }
}

fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load_settings() -> RouxSettings {
    let path = settings_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        RouxSettings::default()
    }
}

pub fn save_settings(settings: &RouxSettings) -> Result<(), String> {
    let path = settings_path();
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Wire settings into main.rs**

Replace `src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;

use std::sync::Mutex;
use tauri::{Emitter, Manager};

struct AppState {
    settings: Mutex<settings::RouxSettings>,
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> settings::RouxSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn update_settings(
    settings: settings::RouxSettings,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    settings::save_settings(&settings)?;
    *state.settings.lock().unwrap() = settings.clone();
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}

fn main() {
    let initial_settings = settings::load_settings();

    tauri::Builder::default()
        .manage(AppState {
            settings: Mutex::new(initial_settings),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Verify it compiles**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
cd src-tauri && cargo check
```

Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/settings.rs src-tauri/src/main.rs
git commit -m "feat: add settings module with load/save to ~/.config/roux/"
```

---

### Task 4: Rust Worktree Module

**Files:**
- Create: `src-tauri/src/worktree.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Create worktree.rs**

Create `src-tauri/src/worktree.rs`:

```rust
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_main: bool,
}

fn sanitize_branch_for_path(branch: &str) -> String {
    branch
        .replace('/', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

fn repo_name(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string())
}

fn resolve_worktree_path(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
) -> PathBuf {
    let sanitized = sanitize_branch_for_path(branch);
    let name = repo_name(repo_path);
    let dir_name = format!("{}-{}", name, sanitized);

    let base = match base_path {
        Some(p) => {
            let expanded = if p == "~" {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
            } else if let Some(rest) = p.strip_prefix("~/") {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                home.join(rest)
            } else {
                PathBuf::from(p)
            };
            expanded
        }
        None => Path::new(repo_path).parent().unwrap_or(Path::new(".")).to_path_buf(),
    };

    let mut target = base.join(&dir_name);
    let mut suffix = 2;
    while target.exists() {
        target = base.join(format!("{}-{}", dir_name, suffix));
        suffix += 1;
    }
    target
}

fn branch_exists(repo_path: &str, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn create_worktree(
    repo_path: &str,
    branch: &str,
    base_path: Option<&str>,
) -> Result<String, String> {
    let target = resolve_worktree_path(repo_path, branch, base_path);
    let target_str = target.to_string_lossy().to_string();

    let output = if branch_exists(repo_path, branch) {
        Command::new("git")
            .args(["worktree", "add", &target_str, branch])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    } else {
        Command::new("git")
            .args(["worktree", "add", "-b", branch, &target_str])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", stderr));
    }

    Ok(target_str)
}

pub fn remove_worktree(worktree_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["worktree", "remove", worktree_path, "--force"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree remove failed: {}", stderr));
    }

    Ok(())
}

pub fn list_worktrees(repo_path: &str) -> Result<Vec<Worktree>, String> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree list failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.to_string());
            current_branch = None;
            is_bare = false;
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // branch refs/heads/main -> main
            current_branch = Some(
                branch_ref
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch_ref)
                    .to_string(),
            );
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                if !is_bare {
                    let branch = current_branch
                        .take()
                        .unwrap_or_else(|| "HEAD".to_string());
                    let is_main = worktrees.is_empty(); // first entry is main worktree
                    worktrees.push(Worktree {
                        path,
                        branch,
                        is_main,
                    });
                }
            }
            current_branch = None;
            is_bare = false;
        }
    }

    // Handle last entry (no trailing blank line)
    if let Some(path) = current_path {
        if !is_bare {
            let branch = current_branch.unwrap_or_else(|| "HEAD".to_string());
            let is_main = worktrees.is_empty();
            worktrees.push(Worktree {
                path,
                branch,
                is_main,
            });
        }
    }

    Ok(worktrees)
}
```

- [ ] **Step 2: Add worktree commands to main.rs**

Add to `src-tauri/src/main.rs`:

```rust
mod worktree;
```

And add these command functions:

```rust
#[tauri::command]
fn cmd_create_worktree(
    repo_path: String,
    branch: String,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap();
    let base_path = settings.worktree_base_path.as_deref();
    worktree::create_worktree(&repo_path, &branch, base_path)
}

#[tauri::command]
fn cmd_remove_worktree(worktree_path: String) -> Result<(), String> {
    worktree::remove_worktree(&worktree_path)
}

#[tauri::command]
fn cmd_list_worktrees(repo_path: String) -> Result<Vec<worktree::Worktree>, String> {
    worktree::list_worktrees(&repo_path)
}
```

Add to `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    get_settings,
    update_settings,
    cmd_create_worktree,
    cmd_remove_worktree,
    cmd_list_worktrees,
])
```

- [ ] **Step 3: Update tauri.ts to match command names**

In `src/lib/tauri.ts`, update the worktree function invoke names to match the Rust `cmd_` prefixed command names:

```typescript
export async function createWorktree(
  repoPath: string,
  branch: string
): Promise<string> {
  return invoke("cmd_create_worktree", { repoPath, branch });
}

export async function removeWorktree(
  worktreePath: string
): Promise<void> {
  return invoke("cmd_remove_worktree", { worktreePath });
}

export async function listWorktrees(
  repoPath: string
): Promise<Worktree[]> {
  return invoke("cmd_list_worktrees", { repoPath });
}
```

- [ ] **Step 4: Verify it compiles**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux/src-tauri && cargo check
```

Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/worktree.rs src-tauri/src/main.rs src/lib/tauri.ts
git commit -m "feat: add git worktree module with create/remove/list"
```

---

### Task 5: Rust OSC Parser

**Files:**
- Create: `src-tauri/src/osc.rs`

- [ ] **Step 1: Create osc.rs**

Create `src-tauri/src/osc.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub status: String,
    pub model: Option<String>,
    pub cost: Option<f64>,
}

/// Scans a byte buffer for OSC title-set sequences (\x1b]0;...\x07 or \x1b]0;...\x1b\\)
/// and extracts Claude Code status information from the title string.
pub fn parse_osc_status(buf: &[u8]) -> Option<SessionStatus> {
    let text = String::from_utf8_lossy(buf);
    let mut last_title: Option<&str> = None;

    // Find all OSC sequences and keep the last one
    let mut search = text.as_ref();
    while let Some(start) = search.find("\x1b]") {
        let after_osc = &search[start + 2..];
        // Find BEL terminator (\x07) or ST terminator (\x1b\\, 2 bytes)
        let (end_pos, terminator_len) = if let Some(pos) = after_osc.find('\x07') {
            (pos, 1)
        } else if let Some(pos) = after_osc.find("\x1b\\") {
            (pos, 2)
        } else {
            break;
        };
        let payload = &after_osc[..end_pos];
        // OSC 0 or OSC 2 set window title
        if let Some(title) = payload.strip_prefix("0;").or_else(|| payload.strip_prefix("2;")) {
            last_title = Some(title);
        }
        search = &after_osc[end_pos + terminator_len..];
    }

    let title = last_title?;

    // Parse Claude Code title format
    // Typical: "Thinking | ~/project | personal | Opus 4.6 (1M) | 2m | $0.16 | 5%"
    // Or: "Generating | ~/project | ..."
    // Or: "Idle | ~/project | ..."
    let parts: Vec<&str> = title.split(" | ").collect();
    if parts.is_empty() {
        return None;
    }

    let status_str = parts[0].trim().to_lowercase();
    let status = match status_str.as_str() {
        s if s.contains("think") => "thinking",
        s if s.contains("generat") => "generating",
        s if s.contains("idle") => "idle",
        _ => return None,
    };

    let mut model: Option<String> = None;
    let mut cost: Option<f64> = None;

    for part in &parts[1..] {
        let trimmed = part.trim();
        if trimmed.starts_with('$') {
            if let Ok(c) = trimmed[1..].parse::<f64>() {
                cost = Some(c);
            }
        } else if trimmed.contains("Opus")
            || trimmed.contains("Sonnet")
            || trimmed.contains("Haiku")
        {
            model = Some(trimmed.to_string());
        }
    }

    Some(SessionStatus {
        status: status.to_string(),
        model,
        cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thinking_status() {
        let buf = b"\x1b]0;Thinking | ~/project | personal | Opus 4.6 (1M) | 2m | $0.16 | 5%\x07";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "thinking");
        assert_eq!(result.model, Some("Opus 4.6 (1M)".to_string()));
        assert_eq!(result.cost, Some(0.16));
    }

    #[test]
    fn test_parse_idle_status() {
        let buf = b"\x1b]0;Idle | ~/project | personal | Sonnet 4.6 | 0m | $0.00 | 0%\x07";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "idle");
        assert_eq!(result.model, Some("Sonnet 4.6".to_string()));
        assert_eq!(result.cost, Some(0.0));
    }

    #[test]
    fn test_no_osc_returns_none() {
        let buf = b"Hello world, no OSC here";
        assert!(parse_osc_status(buf).is_none());
    }

    #[test]
    fn test_parse_generating_with_st_terminator() {
        let buf = b"\x1b]0;Generating | ~/project | Opus 4.6 (1M) | $1.23\x1b\\";
        let result = parse_osc_status(buf).unwrap();
        assert_eq!(result.status, "generating");
        assert_eq!(result.cost, Some(1.23));
    }
}
```

- [ ] **Step 2: Add mod to main.rs**

Add to `src-tauri/src/main.rs`:

```rust
mod osc;
```

- [ ] **Step 3: Run tests**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux/src-tauri && cargo test
```

Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/osc.rs src-tauri/src/main.rs
git commit -m "feat: add OSC escape sequence parser for Claude Code status detection"
```

---

### Task 6: Rust PTY Manager

**Files:**
- Create: `src-tauri/src/pty.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Create pty.rs**

Create `src-tauri/src/pty.rs`:

```rust
use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::Emitter;

use crate::osc;

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn std::io::Write + Send>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn spawn(
        &self,
        session_id: &str,
        working_dir: &str,
        model: Option<&str>,
        additional_flags: &[String],
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let mut cmd = CommandBuilder::new("claude");
        if let Some(m) = model {
            cmd.arg("--model");
            cmd.arg(m);
        }
        for flag in additional_flags {
            cmd.arg(flag);
        }
        cmd.cwd(working_dir);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let id_for_thread = session_id.to_string();
        let app_for_thread = app.clone();

        // Reader thread: reads PTY output, emits to frontend, parses OSC
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // PTY closed
                        let _ = app_for_thread.emit(
                            &format!("session-exit:{}", id_for_thread),
                            serde_json::json!({"code": null}),
                        );
                        break;
                    }
                    Ok(n) => {
                        let data = &buf[..n];

                        // Emit raw output as base64
                        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
                        let _ = app_for_thread.emit(
                            &format!("pty-output:{}", id_for_thread),
                            b64,
                        );

                        // Parse OSC for status updates
                        if let Some(status) = osc::parse_osc_status(data) {
                            let _ = app_for_thread.emit(
                                &format!("session-status:{}", id_for_thread),
                                &status,
                            );
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let session = PtySession {
            master: pair.master,
            child,
            writer,
        };

        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string(), session);

        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        use std::io::Write;
        session
            .writer
            .write_all(data)
            .map_err(|e| format!("Write failed: {}", e))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("Flush failed: {}", e))
    }

    pub fn resize(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize failed: {}", e))
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Wire PTY manager into main.rs**

Update `src-tauri/src/main.rs`. Add `mod pty;` at the top with the other module declarations, then update AppState:

```rust
mod pty;

use crate::pty::PtyManager;

struct AppState {
    settings: Mutex<settings::RouxSettings>,
    pty_manager: PtyManager,
}
```

Add PTY command handlers:

```rust
// Note: spec says Vec<u8> but xterm.js onData sends UTF-8 strings.
// We accept String and convert to bytes server-side for simplicity.
#[tauri::command]
fn write_to_session(id: String, data: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.write(&id, data.as_bytes())
}

#[tauri::command]
fn resize_session(
    id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.pty_manager.resize(&id, cols, rows)
}

#[tauri::command]
fn kill_session(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.kill(&id)
}
```

Update builder:

```rust
.manage(AppState {
    settings: Mutex::new(initial_settings),
    pty_manager: PtyManager::new(),
})
.invoke_handler(tauri::generate_handler![
    get_settings,
    update_settings,
    create_worktree,
    cmd_remove_worktree,
    cmd_list_worktrees,
    write_to_session,
    resize_session,
    kill_session,
])
```

- [ ] **Step 3: Verify it compiles**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux/src-tauri && cargo check
```

Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/pty.rs src-tauri/src/main.rs
git commit -m "feat: add PTY manager with spawn, write, resize, kill"
```

---

### Task 7: Rust Session Module and create_session Command

**Files:**
- Create: `src-tauri/src/session.rs`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Create session.rs**

Create `src-tauri/src/session.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub repo_root: String,
    pub worktree_path: String,
    pub branch: String,
    pub is_worktree: bool,
    pub status: String,
    pub model: Option<String>,
    pub cost: Option<f64>,
    pub created_at: u64,
}

pub struct SessionStore {
    sessions: Mutex<Vec<Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }

    pub fn load_persisted() -> Self {
        let path = Self::persistence_path();
        let sessions = if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut sessions: Vec<Session> =
                serde_json::from_str(&content).unwrap_or_default();
            // Mark all restored sessions as disconnected
            for s in &mut sessions {
                s.status = "disconnected".to_string();
            }
            sessions
        } else {
            Vec::new()
        };
        Self {
            sessions: Mutex::new(sessions),
        }
    }

    pub fn add(&self, session: Session) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.push(session);
        Self::persist(&sessions);
    }

    pub fn remove(&self, id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.id != id);
        Self::persist(&sessions);
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.lock().unwrap().clone()
    }

    pub fn update_status(&self, id: &str, status: &str, model: Option<String>, cost: Option<f64>) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.iter_mut().find(|s| s.id == id) {
            session.status = status.to_string();
            if let Some(m) = model {
                session.model = Some(m);
            }
            if let Some(c) = cost {
                session.cost = Some(c);
            }
        }
        Self::persist(&sessions);
    }

    fn persistence_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("roux").join("sessions.json")
    }

    fn persist(sessions: &[Session]) {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(sessions) {
            let _ = fs::write(&path, json);
        }
    }
}
```

- [ ] **Step 2: Add session store to AppState and create_session command**

Update `src-tauri/src/main.rs`:

```rust
mod session;

use crate::session::{Session, SessionStore};

struct AppState {
    settings: Mutex<settings::RouxSettings>,
    pty_manager: PtyManager,
    session_store: SessionStore,
}
```

Add the `create_session` command:

```rust
#[tauri::command]
fn create_session(
    repo_path: String,
    name: String,
    worktree_path: Option<String>,
    branch: Option<String>,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<Session, String> {
    let settings = state.settings.lock().unwrap().clone();
    let session_id = uuid::Uuid::new_v4().to_string();

    // Determine working directory
    let (work_dir, actual_branch, is_wt) = if let Some(wt_path) = worktree_path {
        // Use provided worktree path — detect branch from the directory
        let br = branch
            .or_else(|| get_current_branch(&wt_path))
            .unwrap_or_else(|| "main".to_string());
        (wt_path, br, false)
    } else if let Some(br) = branch {
        // Create new worktree
        let base = settings.worktree_base_path.as_deref();
        let wt_path = worktree::create_worktree(&repo_path, &br, base)?;
        (wt_path, br, true)
    } else {
        // Use repo directly
        let br = get_current_branch(&repo_path).unwrap_or_else(|| "main".to_string());
        (repo_path.clone(), br, false)
    };

    // Spawn PTY
    let spawn_result = state.pty_manager.spawn(
        &session_id,
        &work_dir,
        settings.default_model.as_deref(),
        &settings.additional_flags,
        app.clone(),
    );

    // Rollback worktree on spawn failure
    if let Err(e) = spawn_result {
        if is_wt {
            let _ = worktree::remove_worktree(&work_dir);
        }
        return Err(e);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let session = Session {
        id: session_id,
        name,
        repo_root: repo_path,
        worktree_path: work_dir,
        branch: actual_branch,
        is_worktree: is_wt,
        status: "idle".to_string(),
        model: None,
        cost: None,
        created_at: now,
    };

    state.session_store.add(session.clone());
    Ok(session)
}

fn get_current_branch(repo_path: &str) -> Option<String> {
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

#[tauri::command]
fn list_sessions(state: tauri::State<AppState>) -> Vec<Session> {
    state.session_store.list()
}
```

Update `kill_session` to also remove from store:

```rust
#[tauri::command]
fn kill_session(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state.pty_manager.kill(&id)?;
    state.session_store.remove(&id);
    Ok(())
}
```

Add to `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    get_settings,
    update_settings,
    create_worktree,
    cmd_remove_worktree,
    cmd_list_worktrees,
    write_to_session,
    resize_session,
    kill_session,
    create_session,
    list_sessions,
])
```

Update `.manage()`:

```rust
.manage(AppState {
    settings: Mutex::new(initial_settings),
    pty_manager: PtyManager::new(),
    session_store: SessionStore::load_persisted(),
})
```

- [ ] **Step 3: Verify it compiles**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux/src-tauri && cargo check
```

Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/session.rs src-tauri/src/main.rs
git commit -m "feat: add session store with persistence and atomic create_session"
```

---

### Task 8: Svelte Stores

**Files:**
- Create: `src/lib/stores/sessions.ts`, `src/lib/stores/settings.ts`

- [ ] **Step 1: Create sessions store**

Create `src/lib/stores/sessions.ts`:

```typescript
import { writable, derived } from "svelte/store";
import type { Session } from "../types";

interface SessionState {
  sessions: Session[];
  activeSessionId: string | null;
}

export const sessionState = writable<SessionState>({
  sessions: [],
  activeSessionId: null,
});

export const activeSession = derived(sessionState, ($state) =>
  $state.sessions.find((s) => s.id === $state.activeSessionId) ?? null
);

export function addSession(session: Session) {
  sessionState.update((state) => ({
    ...state,
    sessions: [...state.sessions, session],
    activeSessionId: session.id,
  }));
}

export function removeSession(id: string) {
  sessionState.update((state) => {
    const sessions = state.sessions.filter((s) => s.id !== id);
    const activeSessionId =
      state.activeSessionId === id
        ? sessions[sessions.length - 1]?.id ?? null
        : state.activeSessionId;
    return { sessions, activeSessionId };
  });
}

export function setActiveSession(id: string) {
  sessionState.update((state) => ({ ...state, activeSessionId: id }));
}

export function updateSessionStatus(
  id: string,
  status: Session["status"],
  model?: string | null,
  cost?: number | null
) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id
        ? {
            ...s,
            status,
            model: model ?? s.model,
            cost: cost ?? s.cost,
          }
        : s
    ),
  }));
}

export function setSessionDisconnected(id: string) {
  updateSessionStatus(id, "disconnected");
}

export function renameSession(id: string, newName: string) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, name: newName } : s
    ),
  }));
}
```

- [ ] **Step 2: Create settings store**

Create `src/lib/stores/settings.ts`:

```typescript
import { writable } from "svelte/store";
import type { RouxSettings } from "../types";
import { DEFAULT_SETTINGS } from "../types";
import {
  getSettings,
  updateSettings as updateSettingsApi,
  onSettingsChanged,
} from "../tauri";

export const settings = writable<RouxSettings>(DEFAULT_SETTINGS);

let debounceTimer: ReturnType<typeof setTimeout> | null = null;

export async function initSettings() {
  const loaded = await getSettings();
  settings.set(loaded);

  // Listen for changes from backend
  await onSettingsChanged((updated) => {
    settings.set(updated);
  });
}

export function updateSetting<K extends keyof RouxSettings>(
  key: K,
  value: RouxSettings[K]
) {
  settings.update((s) => {
    const updated = { ...s, [key]: value };

    // Debounced save to backend
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      updateSettingsApi(updated);
    }, 500);

    return updated;
  });
}
```

- [ ] **Step 3: Commit**

```bash
git add src/lib/stores/sessions.ts src/lib/stores/settings.ts
git commit -m "feat: add Svelte stores for sessions and settings"
```

---

### Task 9: Terminal Component (xterm.js)

**Files:**
- Create: `src/lib/components/Terminal.svelte`

- [ ] **Step 1: Create Terminal.svelte**

Create `src/lib/components/Terminal.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { onPtyOutput, onSessionStatus, onSessionExit, writeToSession, resizeSession } from "$lib/tauri";
  import { updateSessionStatus, setSessionDisconnected } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    sessionId: string;
    active: boolean;
  }

  let { sessionId, active }: Props = $props();

  let containerEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let unlisteners: UnlistenFn[] = [];
  let resizeObserver: ResizeObserver | null = null;

  const terminalInstances = new Map<string, Terminal>();

  function getOrCreateTerminal(): Terminal {
    if (terminalInstances.has(sessionId)) {
      return terminalInstances.get(sessionId)!;
    }

    const term = new Terminal({
      fontSize: $settings.fontSize,
      fontFamily: $settings.fontFamily,
      lineHeight: $settings.lineHeight,
      scrollback: $settings.scrollback,
      cursorStyle: $settings.cursorStyle as "block" | "underline" | "bar",
      cursorBlink: $settings.cursorBlink,
      theme: {
        background: "#0a0a0c",
        foreground: "#c8cad8",
        cursor: "#7aa2f7",
        selectionBackground: "#282b40",
        black: "#0a0a0c",
        red: "#f7768e",
        green: "#9ece6a",
        yellow: "#e0af68",
        blue: "#7aa2f7",
        magenta: "#bb9af7",
        cyan: "#7dcfff",
        white: "#c8cad8",
      },
    });

    terminalInstances.set(sessionId, term);
    return term;
  }

  async function attachListeners() {
    const outputUnlisten = await onPtyOutput(sessionId, (b64data) => {
      const bytes = Uint8Array.from(atob(b64data), (c) => c.charCodeAt(0));
      terminal?.write(bytes);
    });
    unlisteners.push(outputUnlisten);

    const statusUnlisten = await onSessionStatus(sessionId, (payload) => {
      updateSessionStatus(
        sessionId,
        payload.status as any,
        payload.model,
        payload.cost
      );
    });
    unlisteners.push(statusUnlisten);

    const exitUnlisten = await onSessionExit(sessionId, (_code) => {
      setSessionDisconnected(sessionId);
    });
    unlisteners.push(exitUnlisten);
  }

  function attach() {
    if (!containerEl) return;

    terminal = getOrCreateTerminal();

    if (!terminal.element) {
      // First time — open into the container
      terminal.open(containerEl);

      fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);

      try {
        terminal.loadAddon(new WebglAddon());
      } catch {
        // WebGL not available, fall back to canvas
      }

      terminal.loadAddon(new WebLinksAddon());

      terminal.onData((data) => {
        writeToSession(sessionId, data);
      });
    } else {
      // Re-attach existing terminal element
      containerEl.appendChild(terminal.element);
    }

    requestAnimationFrame(() => {
      fitAddon?.fit();
      const dims = fitAddon?.proposeDimensions();
      if (dims) {
        resizeSession(sessionId, dims.cols, dims.rows);
      }
    });
  }

  function detach() {
    if (terminal?.element && containerEl?.contains(terminal.element)) {
      containerEl.removeChild(terminal.element);
    }
  }

  onMount(async () => {
    await attachListeners();

    resizeObserver = new ResizeObserver(() => {
      if (active && fitAddon) {
        fitAddon.fit();
        const dims = fitAddon.proposeDimensions();
        if (dims) {
          resizeSession(sessionId, dims.cols, dims.rows);
        }
      }
    });
    resizeObserver.observe(containerEl);

    if (active) attach();
  });

  onDestroy(() => {
    for (const unlisten of unlisteners) unlisten();
    resizeObserver?.disconnect();
    detach();
  });

  $effect(() => {
    if (active) {
      attach();
    } else {
      detach();
    }
  });
</script>

<div
  bind:this={containerEl}
  class="flex-1 w-full h-full"
  class:hidden={!active}
></div>
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/components/Terminal.svelte
git commit -m "feat: add Terminal component with xterm.js attach/detach"
```

---

### Task 10: SessionCard Component

**Files:**
- Create: `src/lib/components/SessionCard.svelte`

- [ ] **Step 1: Create SessionCard.svelte**

Create `src/lib/components/SessionCard.svelte`:

```svelte
<script lang="ts">
  import type { Session } from "$lib/types";

  interface Props {
    session: Session;
    active: boolean;
    onselect: () => void;
    onclose: () => void;
    onrename: (newName: string) => void;
  }

  let { session, active, onselect, onclose, onrename }: Props = $props();

  let editing = $state(false);
  let editName = $state(session.name);

  function startEditing(e: MouseEvent) {
    e.stopPropagation();
    editName = session.name;
    editing = true;
  }

  function commitRename() {
    editing = false;
    const trimmed = editName.trim();
    if (trimmed && trimmed !== session.name) {
      onrename(trimmed);
    }
  }

  const statusClasses: Record<Session["status"], string> = {
    idle: "bg-green shadow-[0_0_6px_var(--color-green-dim)]",
    thinking: "bg-amber shadow-[0_0_6px_var(--color-amber-dim)] animate-pulse",
    generating: "bg-blue shadow-[0_0_6px_var(--color-blue-dim)] animate-[stream_1.5s_ease-in-out_infinite]",
    error: "bg-red shadow-[0_0_6px_var(--color-red-dim)]",
    disconnected: "bg-gray opacity-60",
  };

  const labelClasses: Record<Session["status"], string> = {
    idle: "text-green bg-green/10",
    thinking: "text-amber bg-amber/10",
    generating: "text-blue bg-blue/10",
    error: "text-red bg-red/10",
    disconnected: "text-gray bg-gray/15",
  };

  const labelText: Record<Session["status"], string> = {
    idle: "idle",
    thinking: "think",
    generating: "gen",
    error: "error",
    disconnected: "disc",
  };
</script>

<!-- Use div, not button, to avoid invalid nested <button> for the close control -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="w-full text-left p-2.5 rounded-lg cursor-pointer transition-all duration-150 relative border group
    {active
      ? 'bg-bg-active border-border'
      : 'border-transparent hover:bg-bg-hover'}"
  onclick={onselect}
  title={session.worktreePath}
>
  {#if active}
    <div class="absolute left-0 top-2 bottom-2 w-0.5 bg-accent rounded-r"></div>
  {/if}

  <div class="flex items-center gap-2 mb-1">
    <div class="w-2 h-2 rounded-full shrink-0 {statusClasses[session.status]}"></div>

    {#if editing}
      <input
        class="text-[13px] font-medium text-text-primary bg-bg-deep border border-accent-dim rounded px-1 py-0 flex-1 outline-none font-sans"
        bind:value={editName}
        onblur={commitRename}
        onkeydown={(e) => { if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') { editing = false; } }}
      />
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <span
        class="text-[13px] font-medium text-text-primary truncate flex-1"
        ondblclick={startEditing}
      >
        {session.name}
      </span>
    {/if}

    <span class="text-[10px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded {labelClasses[session.status]}">
      {labelText[session.status]}
    </span>
    <button
      class="opacity-0 group-hover:opacity-100 bg-transparent border-none text-text-muted hover:text-red hover:bg-bg-elevated text-sm p-0.5 rounded cursor-pointer transition-all duration-150"
      onclick={(e) => { e.stopPropagation(); onclose(); }}
    >
      &times;
    </button>
  </div>

  <div class="flex items-center gap-2 pl-4">
    <span class="font-mono text-[11px] text-accent flex items-center gap-1">
      <span class="text-[10px] opacity-70">&#9095;</span>
      {session.branch}
    </span>
    <span class="font-mono text-[10px] text-text-secondary ml-auto">
      {session.cost != null ? `$${session.cost.toFixed(2)}` : ""}
    </span>
  </div>
</div>

<style>
  @keyframes stream {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add src/lib/components/SessionCard.svelte
git commit -m "feat: add SessionCard component with status indicators"
```

---

### Task 11: SessionTabs, StatusBar, and Layout Components

**Files:**
- Create: `src/lib/components/SessionTabs.svelte`, `src/lib/components/StatusBar.svelte`, `src/lib/components/Layout.svelte`
- Modify: `src/App.svelte`

- [ ] **Step 1: Create SessionTabs.svelte**

Create `src/lib/components/SessionTabs.svelte`:

```svelte
<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { killSession } from "$lib/tauri";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  async function handleClose(id: string) {
    await killSession(id);
    removeSession(id);
  }
</script>

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle">
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto px-2 scrollbar-thin">
    {#each $sessionState.sessions as session (session.id)}
      <SessionCard
        {session}
        active={session.id === $sessionState.activeSessionId}
        onselect={() => setActiveSession(session.id)}
        onclose={() => handleClose(session.id)}
        onrename={(newName) => renameSession(session.id, newName)}
      />
    {/each}
  </div>

  <div class="p-2 border-t border-border-subtle flex gap-1">
    <button
      class="flex-1 py-2 bg-accent-dim border-none rounded-md text-accent text-xs font-sans cursor-pointer flex items-center justify-center gap-1.5 transition-all duration-150 hover:bg-accent hover:text-bg-deep"
      onclick={onNewSession}
    >
      <span class="text-sm">+</span> New
    </button>
    <button
      class="py-2 px-3 bg-bg-elevated border border-border-subtle rounded-md text-text-secondary text-xs cursor-pointer flex items-center justify-center transition-all duration-150 hover:bg-bg-hover hover:text-text-primary"
      onclick={onOpenSettings}
    >
      &#9881;
    </button>
  </div>
</div>
```

- [ ] **Step 2: Create StatusBar.svelte**

Create `src/lib/components/StatusBar.svelte`:

```svelte
<script lang="ts">
  import { activeSession } from "$lib/stores/sessions";

  const statusDotClass: Record<string, string> = {
    idle: "bg-green",
    thinking: "bg-amber animate-pulse",
    generating: "bg-blue",
    error: "bg-red",
    disconnected: "bg-gray",
  };
</script>

<div class="h-8 bg-bg-base border-t border-border-subtle flex items-center px-4 gap-4 font-mono text-[11px] text-text-secondary">
  {#if $activeSession}
    <div class="flex items-center gap-1.5">
      <div class="w-1.5 h-1.5 rounded-full {statusDotClass[$activeSession.status] ?? 'bg-gray'}"></div>
      <span>{$activeSession.name}</span>
    </div>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="text-accent">&#9095; {$activeSession.branch}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span>{$activeSession.model ?? "—"}</span>
    <span class="text-text-muted text-[8px]">&bull;</span>
    <span class="text-green">
      {$activeSession.cost != null ? `$${$activeSession.cost.toFixed(2)}` : "—"}
    </span>
  {:else}
    <span class="text-text-muted">No active session</span>
  {/if}
</div>
```

- [ ] **Step 3: Create Layout.svelte**

Create `src/lib/components/Layout.svelte`:

```svelte
<script lang="ts">
  import SessionTabs from "./SessionTabs.svelte";
  import Terminal from "./Terminal.svelte";
  import StatusBar from "./StatusBar.svelte";
  import { sessionState } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import type { Snippet } from "svelte";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
    settingsPanel?: Snippet;
  }

  let { onNewSession, onOpenSettings, settingsPanel }: Props = $props();
</script>

<div class="h-screen flex flex-col bg-bg-deep text-text-primary">
  <!-- Main area -->
  <div class="flex flex-1 min-h-0"
    class:flex-row={$settings.tabPosition === "left"}
    class:flex-row-reverse={$settings.tabPosition === "right"}
  >
    <!-- Sidebar -->
    <div style="width: {$settings.tabWidth}px" class="shrink-0">
      <SessionTabs {onNewSession} {onOpenSettings} />
    </div>

    <!-- Resize handle -->
    <div class="w-1 cursor-col-resize bg-transparent hover:bg-accent-dim transition-colors shrink-0"></div>

    <!-- Terminal area -->
    <div class="flex-1 relative flex flex-col min-w-0">
      {#if $sessionState.sessions.length === 0}
        <div class="flex-1 flex flex-col items-center justify-center gap-4 text-text-muted">
          <span class="text-5xl opacity-30">&#9636;</span>
          <span class="text-sm">No sessions</span>
          <span class="text-xs font-mono opacity-60">Click "+ New" to create a session</span>
        </div>
      {:else}
        {#each $sessionState.sessions as session (session.id)}
          <Terminal
            sessionId={session.id}
            active={session.id === $sessionState.activeSessionId}
          />
        {/each}
      {/if}

      <!-- Settings panel slot -->
      {#if settingsPanel}
        {@render settingsPanel()}
      {/if}
    </div>
  </div>

  <StatusBar />
</div>
```

- [ ] **Step 4: Update App.svelte**

Replace `src/App.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import Layout from "$lib/components/Layout.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { sessionState, addSession } from "$lib/stores/sessions";
  import { listSessions } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

  onMount(async () => {
    await initSettings();
    // Load persisted sessions
    const sessions = await listSessions();
    for (const s of sessions) {
      addSession(s);
    }
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
/>
```

- [ ] **Step 5: Verify build**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm run tauri dev
```

Expected: Window opens showing "No sessions" empty state with sidebar and status bar.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/SessionTabs.svelte src/lib/components/StatusBar.svelte src/lib/components/Layout.svelte src/App.svelte
git commit -m "feat: add Layout, SessionTabs, and StatusBar components"
```

---

### Task 12: NewSessionDialog Component

**Files:**
- Create: `src/lib/components/NewSessionDialog.svelte`
- Modify: `src/App.svelte`

- [ ] **Step 1: Create NewSessionDialog.svelte**

Create `src/lib/components/NewSessionDialog.svelte`:

```svelte
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { createSession, listWorktrees } from "$lib/tauri";
  import { addSession } from "$lib/stores/sessions";
  import { settings } from "$lib/stores/settings";
  import type { Worktree } from "$lib/types";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();

  let repoPath = $state($settings.defaultProjectPath ?? "");
  let mode = $state<"new" | "existing">("new");
  let branchName = $state("");
  let sessionName = $state("");
  let worktrees = $state<Worktree[]>([]);
  let selectedWorktree = $state<Worktree | null>(null);
  let error = $state("");
  let creating = $state(false);

  async function pickRepo() {
    const selected = await open({ directory: true, title: "Select Git Repository" });
    if (selected) {
      repoPath = selected as string;
      await loadWorktrees();
    }
  }

  async function loadWorktrees() {
    if (!repoPath) return;
    try {
      worktrees = await listWorktrees(repoPath);
      selectedWorktree = worktrees.find((w) => w.isMain) ?? worktrees[0] ?? null;
    } catch {
      worktrees = [];
    }
  }

  async function handleCreate() {
    if (!repoPath) {
      error = "Please select a repository";
      return;
    }
    if (mode === "new" && !branchName.trim()) {
      error = "Branch name is required for new worktrees";
      return;
    }
    error = "";
    creating = true;

    try {
      const name =
        sessionName ||
        repoPath.split("/").pop() +
          "-" +
          (mode === "new" ? branchName : selectedWorktree?.branch ?? "main");

      const session = await createSession(
        repoPath,
        name,
        mode === "existing" ? selectedWorktree?.path ?? null : null,
        mode === "new" ? branchName.trim() : null
      );

      addSession(session);
      resetAndClose();
    } catch (e) {
      error = String(e);
    } finally {
      creating = false;
    }
  }

  function resetAndClose() {
    branchName = "";
    sessionName = "";
    mode = "new";
    error = "";
    onclose();
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
    onclick={(e) => { if (e.target === e.currentTarget) resetAndClose(); }}
  >
    <div class="bg-bg-surface border border-border rounded-xl w-[480px] shadow-2xl">
      <!-- Header -->
      <div class="px-6 pt-5 pb-4 border-b border-border-subtle">
        <h2 class="text-base font-semibold text-text-primary mb-1">New Session</h2>
        <p class="text-xs text-text-muted">Create a new Claude Code session in a git repository</p>
      </div>

      <!-- Body -->
      <div class="px-6 py-5 flex flex-col gap-4">
        <!-- Repo picker -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Repository</label>
          <div class="flex gap-2">
            <input
              class="flex-1 bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              value={repoPath}
              oninput={(e) => (repoPath = e.currentTarget.value)}
              placeholder="~/src/my-project"
            />
            <button
              class="px-3 py-2 bg-bg-elevated border border-border rounded-md text-text-secondary text-xs cursor-pointer hover:bg-bg-hover"
              onclick={pickRepo}
            >
              Browse
            </button>
          </div>
        </div>

        <!-- Mode toggle -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Mode</label>
          <div class="flex bg-bg-deep rounded-md p-0.5 border border-border-subtle">
            <button
              class="flex-1 py-1.5 px-3 border-none text-xs font-medium rounded cursor-pointer transition-all
                {mode === 'new' ? 'bg-bg-active text-text-primary' : 'bg-transparent text-text-secondary'}"
              onclick={() => (mode = "new")}
            >
              New Worktree
            </button>
            <button
              class="flex-1 py-1.5 px-3 border-none text-xs font-medium rounded cursor-pointer transition-all
                {mode === 'existing' ? 'bg-bg-active text-text-primary' : 'bg-transparent text-text-secondary'}"
              onclick={() => { mode = "existing"; loadWorktrees(); }}
            >
              Existing Directory
            </button>
          </div>
        </div>

        <!-- New worktree: branch input -->
        {#if mode === "new"}
          <div class="flex flex-col gap-1.5">
            <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Branch name</label>
            <input
              class="bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
              bind:value={branchName}
              placeholder="feature/my-feature"
            />
          </div>
        {/if}

        <!-- Existing worktree: picker -->
        {#if mode === "existing"}
          <div class="flex flex-col gap-1.5">
            <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">Select worktree</label>
            <div class="flex flex-col gap-1 max-h-30 overflow-y-auto">
              {#each worktrees as wt}
                <button
                  class="flex items-center gap-2 px-2.5 py-2 rounded-md cursor-pointer transition-colors border text-left
                    {selectedWorktree?.path === wt.path
                      ? 'bg-bg-active border-accent-dim'
                      : 'border-transparent hover:bg-bg-hover'}"
                  onclick={() => (selectedWorktree = wt)}
                >
                  {#if wt.isMain}
                    <span class="text-[9px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded bg-green/10 text-green">main</span>
                  {/if}
                  <span class="font-mono text-xs text-accent">{wt.branch}</span>
                  <span class="font-mono text-[10px] text-text-muted ml-auto truncate max-w-40">{wt.path}</span>
                </button>
              {/each}
              {#if worktrees.length === 0}
                <p class="text-xs text-text-muted py-2 text-center">No worktrees found. Select a git repository first.</p>
              {/if}
            </div>
          </div>
        {/if}

        <!-- Session name -->
        <div class="flex flex-col gap-1.5">
          <label class="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
            Session name <span class="font-normal normal-case tracking-normal">(optional)</span>
          </label>
          <input
            class="bg-bg-deep border border-border rounded-md px-3 py-2 font-mono text-[13px] text-text-primary outline-none focus:border-accent-dim"
            bind:value={sessionName}
            placeholder="roux-my-feature"
          />
        </div>

        {#if error}
          <p class="text-xs text-red">{error}</p>
        {/if}
      </div>

      <!-- Footer -->
      <div class="px-6 py-4 border-t border-border-subtle flex justify-end gap-2">
        <button
          class="px-5 py-2 bg-bg-elevated border border-border rounded-md text-text-secondary text-[13px] font-medium cursor-pointer hover:bg-bg-hover"
          onclick={resetAndClose}
        >
          Cancel
        </button>
        <button
          class="px-5 py-2 bg-accent border-none rounded-md text-bg-deep text-[13px] font-medium cursor-pointer hover:brightness-110 disabled:opacity-50"
          onclick={handleCreate}
          disabled={creating}
        >
          {creating ? "Creating..." : "Create Session"}
        </button>
      </div>
    </div>
  </div>
{/if}
```

- [ ] **Step 2: Install Tauri dialog plugin**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm install @tauri-apps/plugin-dialog
```

Add to `src-tauri/Cargo.toml` dependencies:

```toml
tauri-plugin-dialog = "2"
```

Add to `main.rs` builder:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    // ... rest
```

Add to `src-tauri/capabilities/default.json` (create if needed):

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "default capability",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "dialog:default",
    "dialog:allow-open"
  ]
}
```

- [ ] **Step 3: Wire dialog into App.svelte**

Update `src/App.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession } from "$lib/stores/sessions";
  import { listSessions } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

  onMount(async () => {
    await initSettings();
    const sessions = await listSessions();
    for (const s of sessions) {
      addSession(s);
    }
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
/>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>
```

- [ ] **Step 4: Verify build**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm run tauri dev
```

Expected: Click "+ New" → dialog opens with repo picker, mode toggle, branch input.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/NewSessionDialog.svelte src/App.svelte src-tauri/Cargo.toml src-tauri/src/main.rs src-tauri/capabilities/
git commit -m "feat: add NewSessionDialog with worktree mode and repo picker"
```

---

### Task 13: SettingsPanel Component

**Files:**
- Create: `src/lib/components/SettingsPanel.svelte`
- Modify: `src/App.svelte`, `src/lib/components/Layout.svelte`

- [ ] **Step 1: Create SettingsPanel.svelte**

Create `src/lib/components/SettingsPanel.svelte`:

```svelte
<script lang="ts">
  import { settings, updateSetting } from "$lib/stores/settings";

  interface Props {
    visible: boolean;
    onclose: () => void;
  }

  let { visible, onclose }: Props = $props();
</script>

<div
  class="absolute top-0 right-0 bottom-0 w-[380px] bg-bg-surface border-l border-border z-50 flex flex-col shadow-[-8px_0_32px_rgba(0,0,0,0.3)] transition-transform duration-250
    {visible ? 'translate-x-0' : 'translate-x-full'}"
>
  <div class="px-5 py-4 border-b border-border-subtle flex items-center justify-between">
    <span class="text-sm font-semibold">Settings</span>
    <button
      class="bg-transparent border-none text-text-muted cursor-pointer text-base p-1 rounded hover:text-text-primary hover:bg-bg-hover"
      onclick={onclose}
    >&times;</button>
  </div>

  <div class="flex-1 overflow-y-auto px-5 py-4">
    <!-- Layout -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Layout</h3>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Tab position</span>
        <select
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none cursor-pointer appearance-none pr-6"
          value={$settings.tabPosition}
          onchange={(e) => updateSetting("tabPosition", e.currentTarget.value as "left" | "right")}
        >
          <option value="left">Left</option>
          <option value="right">Right</option>
        </select>
      </div>
    </section>

    <!-- Worktrees -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Worktrees</h3>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Base path</div>
          <div class="text-[11px] text-text-muted mt-0.5">Where to create new worktrees</div>
        </div>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.worktreeBasePath ?? ""}
          oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
          placeholder="~/worktrees"
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Cleanup on close</div>
          <div class="text-[11px] text-text-muted mt-0.5">Auto-remove worktrees when closing sessions</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.cleanupWorktreesOnClose
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("cleanupWorktreesOnClose", !$settings.cleanupWorktreesOnClose)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.cleanupWorktreesOnClose
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
    </section>

    <!-- Terminal -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Terminal</h3>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Font size</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-15 text-right focus:border-accent-dim"
          type="number"
          value={$settings.fontSize}
          oninput={(e) => updateSetting("fontSize", parseInt(e.currentTarget.value) || 14)}
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Font family</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.fontFamily}
          oninput={(e) => updateSetting("fontFamily", e.currentTarget.value)}
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Scrollback lines</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-20 text-right focus:border-accent-dim"
          type="number"
          value={$settings.scrollback}
          oninput={(e) => updateSetting("scrollback", parseInt(e.currentTarget.value) || 5000)}
        />
      </div>
    </section>

    <!-- Sessions -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Sessions</h3>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Confirm on close</div>
          <div class="text-[11px] text-text-muted mt-0.5">Prompt before closing active sessions</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.confirmOnClose
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("confirmOnClose", !$settings.confirmOnClose)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.confirmOnClose
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Restore on launch</div>
          <div class="text-[11px] text-text-muted mt-0.5">Show previous sessions on startup</div>
        </div>
        <button
          class="w-9 h-5 rounded-full relative cursor-pointer transition-all border
            {$settings.restoreSessionsOnLaunch
              ? 'bg-accent-dim border-accent'
              : 'bg-bg-deep border-border'}"
          onclick={() => updateSetting("restoreSessionsOnLaunch", !$settings.restoreSessionsOnLaunch)}
        >
          <div class="w-3.5 h-3.5 rounded-full absolute top-0.5 transition-all
            {$settings.restoreSessionsOnLaunch
              ? 'left-[18px] bg-accent'
              : 'left-0.5 bg-text-secondary'}"></div>
        </button>
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Default project path</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.defaultProjectPath ?? ""}
          oninput={(e) => updateSetting("defaultProjectPath", e.currentTarget.value || null)}
          placeholder="~/src"
        />
      </div>
    </section>

    <!-- Claude -->
    <section class="mb-6">
      <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-muted mb-3">Claude</h3>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Default model</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-25 text-right focus:border-accent-dim"
          value={$settings.defaultModel ?? ""}
          oninput={(e) => updateSetting("defaultModel", e.currentTarget.value || null)}
          placeholder="opus"
        />
      </div>
      <div class="flex items-center justify-between py-2">
        <span class="text-[13px]">Additional flags</span>
        <input
          class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-35 text-right focus:border-accent-dim"
          value={$settings.additionalFlags.join(" ")}
          oninput={(e) => updateSetting("additionalFlags", e.currentTarget.value.split(" ").filter(Boolean))}
          placeholder="--verbose"
        />
      </div>
    </section>
  </div>
</div>
```

- [ ] **Step 2: Wire SettingsPanel into App.svelte and Layout**

Update `src/App.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import Layout from "$lib/components/Layout.svelte";
  import NewSessionDialog from "$lib/components/NewSessionDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import { initSettings } from "$lib/stores/settings";
  import { addSession } from "$lib/stores/sessions";
  import { listSessions } from "$lib/tauri";

  let showNewSessionDialog = $state(false);
  let showSettings = $state(false);

  onMount(async () => {
    await initSettings();
    const sessions = await listSessions();
    for (const s of sessions) {
      addSession(s);
    }
  });
</script>

<Layout
  onNewSession={() => (showNewSessionDialog = true)}
  onOpenSettings={() => (showSettings = !showSettings)}
>
  {#snippet settingsPanel()}
    <SettingsPanel visible={showSettings} onclose={() => (showSettings = false)} />
  {/snippet}
</Layout>

<NewSessionDialog
  visible={showNewSessionDialog}
  onclose={() => (showNewSessionDialog = false)}
/>
```

- [ ] **Step 3: Verify build**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm run tauri dev
```

Expected: Gear icon opens settings panel sliding in from the right. Settings controls are interactive.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/SettingsPanel.svelte src/App.svelte
git commit -m "feat: add SettingsPanel with all V1 settings controls"
```

---

### Task 14: Missing Spec Features (Close Prompts, Reconnect, Drag Resize, Directory Pickers)

**Files:**
- Modify: `src/lib/components/SessionTabs.svelte`, `src/lib/components/Layout.svelte`, `src/lib/components/SettingsPanel.svelte`, `src/lib/stores/sessions.ts`, `src/App.svelte`

- [ ] **Step 1: Add confirm-on-close and worktree cleanup prompt to SessionTabs**

Update `handleClose` in `src/lib/components/SessionTabs.svelte`:

```svelte
<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { killSession, removeWorktree } from "$lib/tauri";
  import { settings } from "$lib/stores/settings";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;

    // Confirm if session is active (thinking/generating)
    if (
      $settings.confirmOnClose &&
      (session.status === "thinking" || session.status === "generating")
    ) {
      const confirmed = window.confirm(
        `"${session.name}" is currently ${session.status}. Close it?`
      );
      if (!confirmed) return;
    }

    await killSession(id);

    // Worktree cleanup
    if (session.isWorktree) {
      if ($settings.cleanupWorktreesOnClose) {
        await removeWorktree(session.worktreePath).catch(() => {});
      } else {
        const remove = window.confirm(
          `Also remove the worktree at ${session.worktreePath}?`
        );
        if (remove) {
          await removeWorktree(session.worktreePath).catch(() => {});
        }
      }
    }

    removeSession(id);
  }
</script>
```

Keep the rest of the template unchanged from Task 11.

- [ ] **Step 2: Add reconnect action to SessionCard**

Add reconnect callback to `SessionCard.svelte` Props:

```typescript
  interface Props {
    session: Session;
    active: boolean;
    onselect: () => void;
    onclose: () => void;
    onrename: (newName: string) => void;
    onreconnect: () => void;
  }

  let { session, active, onselect, onclose, onrename, onreconnect }: Props = $props();
```

Add a reconnect button in the template, after the close button div, inside the card header:

```svelte
    {#if session.status === "disconnected"}
      <button
        class="text-[10px] font-medium text-accent bg-accent/10 px-1.5 py-0.5 rounded cursor-pointer border-none hover:bg-accent/20"
        onclick={(e) => { e.stopPropagation(); onreconnect(); }}
      >
        reconnect
      </button>
    {/if}
```

- [ ] **Step 3: Wire reconnect in SessionTabs**

Add reconnect handler and pass to SessionCard:

```svelte
  async function handleReconnect(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    // Remove the old disconnected session
    removeSession(id);
    // Create a fresh session in the same directory (new ID, fresh PTY)
    const newSession = await createSession(
      session.repoRoot,
      session.name,
      session.worktreePath !== session.repoRoot ? session.worktreePath : null,
      null
    );
    addSession(newSession);
  }
```

Add import: `import { createSession } from "$lib/tauri";` and `import { addSession } from "$lib/stores/sessions";`

Pass to SessionCard:
```svelte
  onreconnect={() => handleReconnect(session.id)}
```

- [ ] **Step 4: Respect restoreSessionsOnLaunch in App.svelte**

Update the `onMount` in `src/App.svelte`:

```typescript
  onMount(async () => {
    const loadedSettings = await initSettings();
    // Only restore sessions if setting is enabled
    if (loadedSettings.restoreSessionsOnLaunch) {
      const sessions = await listSessions();
      for (const s of sessions) {
        addSession(s);
      }
    }
  });
```

Update `initSettings` in `src/lib/stores/settings.ts` to return the loaded settings:

```typescript
export async function initSettings(): Promise<RouxSettings> {
  const loaded = await getSettings();
  settings.set(loaded);
  await onSettingsChanged((updated) => {
    settings.set(updated);
  });
  return loaded;
}
```

- [ ] **Step 5: Add drag-to-resize sidebar in Layout.svelte**

Replace the resize handle div and add drag state to `src/lib/components/Layout.svelte`:

```svelte
<script lang="ts">
  // ... existing imports ...
  import { updateSetting } from "$lib/stores/settings";

  // ... existing props ...

  let dragging = $state(false);
  let sidebarWidth = $derived($settings.tabWidth);

  function onDragStart(e: MouseEvent) {
    dragging = true;
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      const w = $settings.tabPosition === "left" ? ev.clientX : window.innerWidth - ev.clientX;
      const clamped = Math.max(180, Math.min(500, w));
      updateSetting("tabWidth", clamped);
    };
    const onUp = () => {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>
```

Update the resize handle element:

```svelte
    <!-- Resize handle -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-1 cursor-col-resize bg-transparent hover:bg-accent-dim transition-colors shrink-0"
      class:bg-accent-dim={dragging}
      onmousedown={onDragStart}
    ></div>
```

And update the sidebar width binding:

```svelte
    <div style="width: {sidebarWidth}px" class="shrink-0">
```

- [ ] **Step 6: Add directory picker buttons to SettingsPanel**

In `src/lib/components/SettingsPanel.svelte`, add import and browse functions:

```typescript
  import { open } from "@tauri-apps/plugin-dialog";

  async function browseWorktreeBase() {
    const selected = await open({ directory: true, title: "Select Worktree Base Directory" });
    if (selected) updateSetting("worktreeBasePath", selected as string);
  }

  async function browseDefaultProject() {
    const selected = await open({ directory: true, title: "Select Default Project Directory" });
    if (selected) updateSetting("defaultProjectPath", selected as string);
  }
```

Update the worktree base path row to include a Browse button:

```svelte
      <div class="flex items-center justify-between py-2">
        <div>
          <div class="text-[13px]">Base path</div>
          <div class="text-[11px] text-text-muted mt-0.5">Where to create new worktrees</div>
        </div>
        <div class="flex gap-1">
          <input
            class="bg-bg-deep border border-border rounded px-2 py-1 font-mono text-xs text-text-primary outline-none w-28 text-right focus:border-accent-dim"
            value={$settings.worktreeBasePath ?? ""}
            oninput={(e) => updateSetting("worktreeBasePath", e.currentTarget.value || null)}
            placeholder="~/worktrees"
          />
          <button
            class="px-2 py-1 bg-bg-elevated border border-border rounded text-text-secondary text-[10px] cursor-pointer hover:bg-bg-hover"
            onclick={browseWorktreeBase}
          >...</button>
        </div>
      </div>
```

Do the same for default project path.

- [ ] **Step 7: Commit**

```bash
git add src/lib/components/SessionTabs.svelte src/lib/components/SessionCard.svelte src/lib/components/Layout.svelte src/lib/components/SettingsPanel.svelte src/lib/stores/sessions.ts src/lib/stores/settings.ts src/App.svelte
git commit -m "feat: add close prompts, reconnect flow, drag resize, directory pickers"
```

---

### Task 15: End-to-End Integration Test

**Files:**
- No new files — manual verification

- [ ] **Step 1: Run the full app**

```bash
cd /Users/sphinizy/src/github.com/phin-tech/roux
npm run tauri dev
```

- [ ] **Step 2: Test session creation**

1. Click "+ New"
2. Browse to a git repository (or type a path)
3. Select "New Worktree", enter a branch name like "test-session"
4. Click "Create Session"
5. Verify: tab appears in sidebar, terminal shows Claude Code launching, status dot shows activity

- [ ] **Step 3: Test session switching**

1. Create a second session (different branch or existing directory)
2. Click between tabs
3. Verify: terminal switches instantly, scrollback preserved in each

- [ ] **Step 4: Test settings**

1. Click gear icon
2. Toggle "Tab position" to "Right"
3. Verify: sidebar moves to the right side
4. Change font size
5. Verify: terminal font updates (on next session)
6. Close and reopen the app
7. Verify: settings are persisted

- [ ] **Step 5: Test session close with prompts**

1. Start a session that is actively generating
2. Click X on its tab
3. Verify: confirmation dialog appears ("currently generating. Close it?")
4. Click Cancel → session stays
5. Click X again, confirm → session closes
6. Create a worktree session, then close it
7. Verify: prompt asks "Also remove the worktree?"

- [ ] **Step 6: Test session name editing**

1. Double-click a session name in the sidebar
2. Verify: inline edit field appears
3. Type a new name, press Enter
4. Verify: name updates

- [ ] **Step 7: Test reconnect**

1. Close the app with active sessions
2. Reopen the app
3. Verify: previous sessions appear as "disconnected" in the tab list
4. Click "reconnect" on a disconnected session
5. Verify: new Claude Code session spawns in the same directory

- [ ] **Step 8: Test sidebar drag resize**

1. Hover over the sidebar edge — cursor changes to col-resize
2. Drag to resize sidebar
3. Verify: sidebar width changes, persists after app restart

- [ ] **Step 9: Commit final state**

```bash
git add -A
git commit -m "feat: Roux v1 — multi-session Claude Code terminal manager"
```

---

## Summary

| Task | Description | Key Files |
|------|-------------|-----------|
| 1 | Project scaffolding | Tauri + Svelte + Tailwind + deps |
| 2 | TypeScript types + IPC wrappers | `types.ts`, `tauri.ts` |
| 3 | Rust settings module | `settings.rs` |
| 4 | Rust worktree module | `worktree.rs` |
| 5 | Rust OSC parser | `osc.rs` |
| 6 | Rust PTY manager | `pty.rs` |
| 7 | Rust session store + create_session | `session.rs`, `main.rs` |
| 8 | Svelte stores | `sessions.ts`, `settings.ts` |
| 9 | Terminal component (xterm.js) | `Terminal.svelte` |
| 10 | SessionCard component (with editable name) | `SessionCard.svelte` |
| 11 | Layout + SessionTabs + StatusBar | `Layout.svelte`, etc. |
| 12 | NewSessionDialog | `NewSessionDialog.svelte` |
| 13 | SettingsPanel | `SettingsPanel.svelte` |
| 14 | Close prompts, reconnect, drag resize, dir pickers | Multiple components |
| 15 | End-to-end integration test | Manual verification |
