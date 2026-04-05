# Task Runner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-discover runnable tasks from repo config files (package.json, Taskfile.yml, Makefile, Justfile) and expose them via Cmd+K and a sidebar panel, executing in shell panes with configurable lifecycle.

**Architecture:** Rust backend discovers tasks by parsing config files through a trait-based plugin system. Frontend stores discovered tasks, tracks running task panes, and handles pane lifecycle (auto-close on success/error). UI surfaces tasks in both the command palette and a collapsible sidebar panel below sessions.

**Tech Stack:** Rust (serde_json, serde_yaml for parsing), Svelte 5 stores, existing PTY/shell pane infrastructure, Bits UI for context menu.

---

## File Structure

### New files
- `src-tauri/src/tasks.rs` — discovery trait + four discoverer implementations + Tauri commands
- `src/lib/types/tasks.ts` — TaskDefinition, TaskGroup, TaskRun, KeepOpen types
- `src/lib/stores/tasks.ts` — taskGroups, taskRuns, taskOverrides stores
- `src/lib/tasks/runner.ts` — runTask function, exit handling
- `src/lib/components/TaskPanel.svelte` — sidebar task panel with context menu
- `src-tauri/src/tasks/tests.rs` or inline `#[cfg(test)]` — Rust discovery tests
- `src/lib/stores/__tests__/tasks.test.ts` — store tests
- `src/lib/tasks/__tests__/runner.test.ts` — runner tests

### Modified files
- `src-tauri/Cargo.toml` — add `serde_yaml` dependency
- `src-tauri/src/main.rs:3` — add `mod tasks;`, register `discover_tasks` + override commands in invoke_handler
- `src/lib/types.ts` — re-export task types
- `src/lib/tauri.ts` — add `discoverTasks`, `loadTaskOverrides`, `saveTaskOverrides` bindings
- `src/lib/commands/index.ts` — register `task.run` command
- `src/lib/components/SessionTabs.svelte` — integrate TaskPanel below session list
- `src/lib/stores/settings.ts` — add `taskPanelSplit` default
- `src-tauri/src/settings.rs` — add `task_panel_split` field

---

### Task 1: Rust task discovery — types and NPM discoverer

**Files:**
- Create: `src-tauri/src/tasks.rs`
- Modify: `src-tauri/Cargo.toml:26-36`
- Modify: `src-tauri/src/main.rs:3-8`

- [ ] **Step 1: Add serde_yaml dependency**

In `src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
serde_yaml = "0.9"
```

- [ ] **Step 2: Create tasks.rs with types and NPM discoverer**

Create `src-tauri/src/tasks.rs`:

```rust
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub runner: String,
    pub command: String,
    pub keep_open: String, // "always" | "on-error" | "never"
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGroup {
    pub runner: String,
    pub config_file: String,
    pub tasks: Vec<TaskDefinition>,
}

trait TaskDiscoverer {
    fn config_file(&self) -> &str;
    fn runner_name(&self) -> &str;
    fn discover(&self, dir: &Path) -> Option<TaskGroup>;
}

struct NpmDiscoverer;

impl TaskDiscoverer for NpmDiscoverer {
    fn config_file(&self) -> &str {
        "package.json"
    }

    fn runner_name(&self) -> &str {
        "npm scripts"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let scripts = json.get("scripts")?.as_object()?;

        let mut tasks: Vec<TaskDefinition> = scripts
            .keys()
            .map(|name| TaskDefinition {
                id: format!("npm:{}", name),
                name: name.clone(),
                description: String::new(),
                runner: "npm".to_string(),
                command: format!("npm run {}", name),
                keep_open: "on-error".to_string(),
            })
            .collect();

        tasks.sort_by(|a, b| a.name.cmp(&b.name));

        if tasks.is_empty() {
            return None;
        }

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}

pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
    ];
    discoverers.iter().filter_map(|d| d.discover(dir)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_npm_discovers_scripts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "vite build", "test": "vitest run", "dev": "vite"}}"#,
        )
        .unwrap();

        let discoverer = NpmDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.runner, "npm scripts");
        assert_eq!(group.tasks.len(), 3);
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].command, "npm run build");
        assert_eq!(group.tasks[0].id, "npm:build");
    }

    #[test]
    fn test_npm_returns_none_without_scripts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "foo"}"#).unwrap();

        let discoverer = NpmDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }

    #[test]
    fn test_npm_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = NpmDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }
}
```

- [ ] **Step 3: Add tempfile dev-dependency to Cargo.toml**

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Register module in main.rs**

In `src-tauri/src/main.rs`, add `mod tasks;` after the existing module declarations (line 3-8):

```rust
mod hooks;
mod pty;
mod session;
mod settings;
mod status_watcher;
mod tasks;
mod worktree;
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: All tests pass including the new `tasks::tests::*` tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/tasks.rs src-tauri/src/main.rs
git commit -m "feat(tasks): add task discovery types and NPM discoverer"
```

---

### Task 2: Taskfile.yml discoverer

**Files:**
- Modify: `src-tauri/src/tasks.rs`

- [ ] **Step 1: Add Taskfile discoverer and tests**

Add to `src-tauri/src/tasks.rs`, after `NpmDiscoverer`:

```rust
struct TaskfileDiscoverer;

impl TaskDiscoverer for TaskfileDiscoverer {
    fn config_file(&self) -> &str {
        "Taskfile.yml"
    }

    fn runner_name(&self) -> &str {
        "Taskfile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&path).ok()?;
        let yaml: serde_yaml::Value = serde_yaml::from_str(&content).ok()?;
        let tasks_map = yaml.get("tasks")?.as_mapping()?;

        let mut tasks: Vec<TaskDefinition> = tasks_map
            .keys()
            .filter_map(|key| {
                let name = key.as_str()?;
                let value = tasks_map.get(key)?;
                let desc = value
                    .get("desc")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(TaskDefinition {
                    id: format!("task:{}", name),
                    name: name.to_string(),
                    description: desc,
                    runner: "task".to_string(),
                    command: format!("task {}", name),
                    keep_open: "on-error".to_string(),
                })
            })
            .collect();

        tasks.sort_by(|a, b| a.name.cmp(&b.name));

        if tasks.is_empty() {
            return None;
        }

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}
```

Add `TaskfileDiscoverer` to the `discover_tasks` function's discoverers vec:

```rust
pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
        Box::new(TaskfileDiscoverer),
    ];
    discoverers.iter().filter_map(|d| d.discover(dir)).collect()
}
```

Add tests:

```rust
    #[test]
    fn test_taskfile_discovers_tasks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Taskfile.yml"),
            r#"
version: "3"
tasks:
  build:
    desc: "Build the project"
    cmds:
      - go build
  test:
    desc: "Run tests"
    cmds:
      - go test ./...
"#,
        )
        .unwrap();

        let discoverer = TaskfileDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.runner, "Taskfile");
        assert_eq!(group.tasks.len(), 2);
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Build the project");
        assert_eq!(group.tasks[0].command, "task build");
    }

    #[test]
    fn test_taskfile_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = TaskfileDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/tasks.rs
git commit -m "feat(tasks): add Taskfile.yml discoverer"
```

---

### Task 3: Makefile discoverer

**Files:**
- Modify: `src-tauri/src/tasks.rs`

- [ ] **Step 1: Add Makefile discoverer and tests**

Add to `src-tauri/src/tasks.rs`, after `TaskfileDiscoverer`:

```rust
struct MakeDiscoverer;

impl TaskDiscoverer for MakeDiscoverer {
    fn config_file(&self) -> &str {
        "Makefile"
    }

    fn runner_name(&self) -> &str {
        "Makefile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        let path = dir.join(self.config_file());
        let content = std::fs::read_to_string(&path).ok()?;
        let mut tasks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            // Match target lines: "name:" or "name: deps"
            // Skip lines starting with . or _ (internal targets)
            // Skip lines with tabs (recipe lines)
            if let Some(colon_pos) = line.find(':') {
                let target = line[..colon_pos].trim();
                if target.is_empty()
                    || target.starts_with('.')
                    || target.starts_with('_')
                    || target.starts_with('\t')
                    || target.starts_with('#')
                    || target.contains(' ')
                    || target.contains('$')
                    || target.contains('%')
                {
                    continue;
                }
                // Check for '=' which indicates a variable, not a target
                if line.contains('=') && !line[colon_pos..].contains('=') {
                    // colon is before any '=', likely a target
                } else if line.contains('=') {
                    continue;
                }
                // Look for ## comment on preceding line
                let desc = if i > 0 {
                    let prev = lines[i - 1].trim();
                    if let Some(comment) = prev.strip_prefix("##") {
                        comment.trim().to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                tasks.push(TaskDefinition {
                    id: format!("make:{}", target),
                    name: target.to_string(),
                    description: desc,
                    runner: "make".to_string(),
                    command: format!("make {}", target),
                    keep_open: "on-error".to_string(),
                });
            }
        }

        tasks.sort_by(|a, b| a.name.cmp(&b.name));

        if tasks.is_empty() {
            return None;
        }

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: self.config_file().to_string(),
            tasks,
        })
    }
}
```

Add `MakeDiscoverer` to `discover_tasks`:

```rust
pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
        Box::new(TaskfileDiscoverer),
        Box::new(MakeDiscoverer),
    ];
    discoverers.iter().filter_map(|d| d.discover(dir)).collect()
}
```

Add tests:

```rust
    #[test]
    fn test_make_discovers_targets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Makefile"),
            "## Build the project\nbuild:\n\tgo build\n\n## Run tests\ntest:\n\tgo test\n\n.PHONY: build test\n",
        )
        .unwrap();

        let discoverer = MakeDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.runner, "Makefile");
        assert_eq!(group.tasks.len(), 2);
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Build the project");
        assert_eq!(group.tasks[0].command, "make build");
    }

    #[test]
    fn test_make_skips_internal_targets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Makefile"),
            ".PHONY: all\n_internal:\n\techo hi\nbuild:\n\tgo build\n",
        )
        .unwrap();

        let discoverer = MakeDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.tasks.len(), 1);
        assert_eq!(group.tasks[0].name, "build");
    }
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/tasks.rs
git commit -m "feat(tasks): add Makefile discoverer"
```

---

### Task 4: Justfile discoverer

**Files:**
- Modify: `src-tauri/src/tasks.rs`

- [ ] **Step 1: Add Justfile discoverer and tests**

Add to `src-tauri/src/tasks.rs`, after `MakeDiscoverer`:

```rust
struct JustDiscoverer;

impl TaskDiscoverer for JustDiscoverer {
    fn config_file(&self) -> &str {
        "justfile"
    }

    fn runner_name(&self) -> &str {
        "Justfile"
    }

    fn discover(&self, dir: &Path) -> Option<TaskGroup> {
        // Try case-insensitive: "justfile", "Justfile"
        let path = dir.join("justfile");
        let path = if path.exists() {
            path
        } else {
            let alt = dir.join("Justfile");
            if alt.exists() {
                alt
            } else {
                return None;
            }
        };

        let content = std::fs::read_to_string(&path).ok()?;
        let mut tasks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            // Recipe lines: "name:" or "name arg:" — no leading whitespace
            if line.starts_with(char::is_whitespace) || line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(colon_pos) = line.find(':') {
                let before_colon = line[..colon_pos].trim();
                // Recipe name is the first word
                let name = before_colon.split_whitespace().next().unwrap_or("");
                if name.is_empty() || name.starts_with('@') {
                    // @name is a silent recipe — still include it but strip @
                    let name = name.strip_prefix('@').unwrap_or(name);
                    if name.is_empty() {
                        continue;
                    }
                }
                let recipe_name = before_colon
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('@');

                if recipe_name.is_empty() {
                    continue;
                }

                // Look for # comment on preceding line
                let desc = if i > 0 {
                    let prev = lines[i - 1].trim();
                    if let Some(comment) = prev.strip_prefix('#') {
                        comment.trim().to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                tasks.push(TaskDefinition {
                    id: format!("just:{}", recipe_name),
                    name: recipe_name.to_string(),
                    description: desc,
                    runner: "just".to_string(),
                    command: format!("just {}", recipe_name),
                    keep_open: "on-error".to_string(),
                });
            }
        }

        tasks.sort_by(|a, b| a.name.cmp(&b.name));

        if tasks.is_empty() {
            return None;
        }

        Some(TaskGroup {
            runner: self.runner_name().to_string(),
            config_file: "justfile".to_string(),
            tasks,
        })
    }
}
```

Add `JustDiscoverer` to `discover_tasks`:

```rust
pub fn discover_tasks(dir: &Path) -> Vec<TaskGroup> {
    let discoverers: Vec<Box<dyn TaskDiscoverer>> = vec![
        Box::new(NpmDiscoverer),
        Box::new(TaskfileDiscoverer),
        Box::new(MakeDiscoverer),
        Box::new(JustDiscoverer),
    ];
    discoverers.iter().filter_map(|d| d.discover(dir)).collect()
}
```

Add tests:

```rust
    #[test]
    fn test_just_discovers_recipes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("justfile"),
            "# Build the project\nbuild:\n    cargo build\n\n# Run tests\ntest:\n    cargo test\n",
        )
        .unwrap();

        let discoverer = JustDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.runner, "Justfile");
        assert_eq!(group.tasks.len(), 2);
        assert_eq!(group.tasks[0].name, "build");
        assert_eq!(group.tasks[0].description, "Build the project");
        assert_eq!(group.tasks[0].command, "just build");
    }

    #[test]
    fn test_just_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Justfile"), "build:\n    go build\n").unwrap();

        let discoverer = JustDiscoverer;
        let group = discoverer.discover(dir.path()).unwrap();
        assert_eq!(group.tasks.len(), 1);
    }

    #[test]
    fn test_just_returns_none_without_file() {
        let dir = tempfile::tempdir().unwrap();
        let discoverer = JustDiscoverer;
        assert!(discoverer.discover(dir.path()).is_none());
    }
```

- [ ] **Step 2: Add integration test for discover_tasks**

```rust
    #[test]
    fn test_discover_tasks_finds_multiple_runners() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "vite build"}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("Makefile"),
            "test:\n\tgo test\n",
        )
        .unwrap();

        let groups = discover_tasks(dir.path());
        assert_eq!(groups.len(), 2);
        let runners: Vec<&str> = groups.iter().map(|g| g.runner.as_str()).collect();
        assert!(runners.contains(&"npm scripts"));
        assert!(runners.contains(&"Makefile"));
    }
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/tasks.rs
git commit -m "feat(tasks): add Justfile discoverer and integration test"
```

---

### Task 5: Tauri command + override persistence

**Files:**
- Modify: `src-tauri/src/tasks.rs`
- Modify: `src-tauri/src/main.rs:266-280`

- [ ] **Step 1: Add Tauri commands and override persistence to tasks.rs**

Add at the bottom of `src-tauri/src/tasks.rs` (before `#[cfg(test)]`):

```rust
use std::collections::HashMap;

fn overrides_path() -> std::path::PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("roux").join("task-overrides.json")
}

#[tauri::command]
pub fn cmd_discover_tasks(dir: String) -> Vec<TaskGroup> {
    discover_tasks(Path::new(&dir))
}

#[tauri::command]
pub fn cmd_load_task_overrides() -> HashMap<String, HashMap<String, String>> {
    let path = overrides_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

#[tauri::command]
pub fn cmd_save_task_overrides(
    overrides: HashMap<String, HashMap<String, String>>,
) -> Result<(), String> {
    let path = overrides_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&overrides).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Register commands in main.rs**

In `src-tauri/src/main.rs`, add the three new commands to the `invoke_handler` (around line 266-280):

```rust
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            cmd_create_worktree,
            cmd_remove_worktree,
            cmd_list_worktrees,
            write_to_session,
            resize_session,
            spawn_shell,
            kill_session,
            create_session,
            list_sessions,
            read_file,
            list_docs,
            tasks::cmd_discover_tasks,
            tasks::cmd_load_task_overrides,
            tasks::cmd_save_task_overrides,
        ])
```

- [ ] **Step 3: Run tests and check compilation**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/tasks.rs src-tauri/src/main.rs
git commit -m "feat(tasks): add Tauri commands for discovery and override persistence"
```

---

### Task 6: Frontend types and Tauri bindings

**Files:**
- Create: `src/lib/types/tasks.ts`
- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts:96`

- [ ] **Step 1: Create task types**

Create `src/lib/types/tasks.ts`:

```typescript
export type KeepOpen = "always" | "on-error" | "never";

export interface TaskDefinition {
  id: string;
  name: string;
  description: string;
  runner: string;
  command: string;
  keepOpen: KeepOpen;
}

export interface TaskGroup {
  runner: string;
  configFile: string;
  tasks: TaskDefinition[];
}

export interface TaskRun {
  taskId: string;
  paneId: string;
  ptyId: string;
  status: "running" | "succeeded" | "failed";
  exitCode: number | null;
  startedAt: number;
}
```

- [ ] **Step 2: Re-export from types.ts**

Add to the bottom of `src/lib/types.ts`:

```typescript
export type { KeepOpen, TaskDefinition, TaskGroup, TaskRun } from "./types/tasks";
```

- [ ] **Step 3: Add Tauri bindings**

Add to `src/lib/tauri.ts`, before the `// Events` comment (around line 96):

```typescript
import type { TaskGroup } from "./types";

// Task discovery
export async function discoverTasks(dir: string): Promise<TaskGroup[]> {
  return invoke("cmd_discover_tasks", { dir });
}

export async function loadTaskOverrides(): Promise<Record<string, Record<string, string>>> {
  return invoke("cmd_load_task_overrides");
}

export async function saveTaskOverrides(
  overrides: Record<string, Record<string, string>>
): Promise<void> {
  return invoke("cmd_save_task_overrides", { overrides });
}
```

- [ ] **Step 4: Verify compilation**

Run: `npm run check`
Expected: PASS (no type errors)

- [ ] **Step 5: Commit**

```bash
git add src/lib/types/tasks.ts src/lib/types.ts src/lib/tauri.ts
git commit -m "feat(tasks): add frontend types and Tauri bindings"
```

---

### Task 7: Task stores

**Files:**
- Create: `src/lib/stores/tasks.ts`
- Create: `src/lib/stores/__tests__/tasks.test.ts`

- [ ] **Step 1: Write failing tests**

Create `src/lib/stores/__tests__/tasks.test.ts`:

```typescript
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  discoverTasks: vi.fn(),
  loadTaskOverrides: vi.fn(),
  saveTaskOverrides: vi.fn(),
}));

import {
  taskGroups,
  taskRuns,
  taskOverrides,
  refreshTasks,
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  getEffectiveKeepOpen,
  setKeepOpenOverride,
  initTaskOverrides,
} from "../tasks";
import { discoverTasks, loadTaskOverrides } from "$lib/tauri";
import type { TaskGroup, TaskRun } from "$lib/types/tasks";

describe("task stores", () => {
  beforeEach(() => {
    taskGroups.set([]);
    taskRuns.set(new Map());
    taskOverrides.set({});
    vi.mocked(discoverTasks).mockReset();
    vi.mocked(loadTaskOverrides).mockReset();
  });

  describe("refreshTasks", () => {
    it("discovers tasks and updates store", async () => {
      const groups: TaskGroup[] = [
        {
          runner: "npm scripts",
          configFile: "package.json",
          tasks: [
            { id: "npm:build", name: "build", description: "", runner: "npm", command: "npm run build", keepOpen: "on-error" },
          ],
        },
      ];
      vi.mocked(discoverTasks).mockResolvedValue(groups);

      await refreshTasks("/repo");

      expect(discoverTasks).toHaveBeenCalledWith("/repo");
      expect(get(taskGroups)).toEqual(groups);
    });

    it("caches results per repo root", async () => {
      const groups: TaskGroup[] = [
        { runner: "npm scripts", configFile: "package.json", tasks: [] },
      ];
      vi.mocked(discoverTasks).mockResolvedValue(groups);

      await refreshTasks("/repo");
      await refreshTasks("/repo");

      expect(discoverTasks).toHaveBeenCalledTimes(1);
    });
  });

  describe("task runs", () => {
    it("adds and retrieves a task run", () => {
      const run: TaskRun = {
        taskId: "npm:build",
        paneId: "pane-1",
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        startedAt: 1000,
      };
      addTaskRun("session-1", run);

      const runs = get(taskRuns).get("session-1");
      expect(runs).toEqual([run]);
    });

    it("updates a task run status", () => {
      addTaskRun("session-1", {
        taskId: "npm:build",
        paneId: "pane-1",
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        startedAt: 1000,
      });

      updateTaskRun("session-1", "pty-1", 0);

      const runs = get(taskRuns).get("session-1")!;
      expect(runs[0].status).toBe("succeeded");
      expect(runs[0].exitCode).toBe(0);
    });

    it("marks nonzero exit as failed", () => {
      addTaskRun("session-1", {
        taskId: "npm:test",
        paneId: "pane-1",
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        startedAt: 1000,
      });

      updateTaskRun("session-1", "pty-1", 1);

      const runs = get(taskRuns).get("session-1")!;
      expect(runs[0].status).toBe("failed");
    });

    it("removes a task run", () => {
      addTaskRun("session-1", {
        taskId: "npm:build",
        paneId: "pane-1",
        ptyId: "pty-1",
        status: "running",
        exitCode: null,
        startedAt: 1000,
      });
      removeTaskRun("session-1", "pty-1");

      const runs = get(taskRuns).get("session-1");
      expect(runs).toEqual([]);
    });
  });

  describe("keepOpen overrides", () => {
    it("returns default when no override exists", () => {
      expect(getEffectiveKeepOpen("/repo", "npm:build", "on-error")).toBe("on-error");
    });

    it("returns override when set", () => {
      setKeepOpenOverride("/repo", "npm:build", "always");
      expect(getEffectiveKeepOpen("/repo", "npm:build", "on-error")).toBe("always");
    });
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- --reporter verbose 2>&1 | head -40`
Expected: FAIL (module not found)

- [ ] **Step 3: Implement the store**

Create `src/lib/stores/tasks.ts`:

```typescript
import { writable, get } from "svelte/store";
import { discoverTasks, loadTaskOverrides, saveTaskOverrides } from "$lib/tauri";
import type { TaskGroup, TaskRun, KeepOpen } from "$lib/types/tasks";

export const taskGroups = writable<TaskGroup[]>([]);
export const taskRuns = writable<Map<string, TaskRun[]>>(new Map());
export const taskOverrides = writable<Record<string, Record<string, string>>>({});

const discoveryCache = new Map<string, TaskGroup[]>();

export async function refreshTasks(repoRoot: string, force = false) {
  if (!force && discoveryCache.has(repoRoot)) {
    taskGroups.set(discoveryCache.get(repoRoot)!);
    return;
  }
  const groups = await discoverTasks(repoRoot);
  discoveryCache.set(repoRoot, groups);
  taskGroups.set(groups);
}

export async function initTaskOverrides() {
  const overrides = await loadTaskOverrides();
  taskOverrides.set(overrides);
}

export function addTaskRun(sessionId: string, run: TaskRun) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId) ?? [];
    runs.push(run);
    map.set(sessionId, runs);
    return new Map(map);
  });
}

export function updateTaskRun(sessionId: string, ptyId: string, exitCode: number | null) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    const run = runs.find((r) => r.ptyId === ptyId);
    if (run) {
      run.exitCode = exitCode;
      run.status = exitCode === 0 ? "succeeded" : "failed";
    }
    return new Map(map);
  });
}

export function removeTaskRun(sessionId: string, ptyId: string) {
  taskRuns.update((map) => {
    const runs = map.get(sessionId);
    if (!runs) return map;
    map.set(
      sessionId,
      runs.filter((r) => r.ptyId !== ptyId)
    );
    return new Map(map);
  });
}

export function getEffectiveKeepOpen(
  repoRoot: string,
  taskId: string,
  defaultKeepOpen: KeepOpen
): KeepOpen {
  const overrides = get(taskOverrides);
  const repoOverrides = overrides[repoRoot];
  if (repoOverrides && repoOverrides[taskId]) {
    return repoOverrides[taskId] as KeepOpen;
  }
  return defaultKeepOpen;
}

export function setKeepOpenOverride(repoRoot: string, taskId: string, keepOpen: KeepOpen) {
  taskOverrides.update((overrides) => {
    const repoOverrides = overrides[repoRoot] ?? {};
    repoOverrides[taskId] = keepOpen;
    overrides[repoRoot] = repoOverrides;
    return { ...overrides };
  });
  // Persist asynchronously
  const current = get(taskOverrides);
  saveTaskOverrides(current).catch(() => {});
}

export function getTaskRun(sessionId: string, taskId: string): TaskRun | undefined {
  const runs = get(taskRuns).get(sessionId);
  return runs?.find((r) => r.taskId === taskId && r.status === "running");
}
```

- [ ] **Step 4: Run tests**

Run: `npm test -- --reporter verbose`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/stores/tasks.ts src/lib/stores/__tests__/tasks.test.ts
git commit -m "feat(tasks): add task stores with discovery cache and override persistence"
```

---

### Task 8: Task runner module

**Files:**
- Create: `src/lib/tasks/runner.ts`
- Create: `src/lib/tasks/__tests__/runner.test.ts`

- [ ] **Step 1: Write failing tests**

Create `src/lib/tasks/__tests__/runner.test.ts`:

```typescript
import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("$lib/tauri", () => ({
  spawnShell: vi.fn(),
  writeToSession: vi.fn(),
  onSessionExit: vi.fn(),
  saveTaskOverrides: vi.fn(),
  loadTaskOverrides: vi.fn(),
  discoverTasks: vi.fn(),
}));

import { runTask } from "../runner";
import { spawnShell, writeToSession, onSessionExit } from "$lib/tauri";
import { taskRuns, taskOverrides } from "$lib/stores/tasks";
import { paneTrees, focusedPaneId } from "$lib/stores/panes";
import type { TaskDefinition } from "$lib/types/tasks";

describe("runTask", () => {
  beforeEach(() => {
    taskRuns.set(new Map());
    taskOverrides.set({});
    paneTrees.set(new Map());
    focusedPaneId.set(null);
    vi.mocked(spawnShell).mockReset().mockResolvedValue(undefined);
    vi.mocked(writeToSession).mockReset().mockResolvedValue(undefined);
    vi.mocked(onSessionExit).mockReset().mockResolvedValue(() => {});
  });

  const task: TaskDefinition = {
    id: "npm:build",
    name: "build",
    description: "Build the project",
    runner: "npm",
    command: "npm run build",
    keepOpen: "on-error",
  };

  it("spawns a shell and writes the command", async () => {
    // Init a session pane tree so addSplit works
    const { initSessionPanes } = await import("$lib/stores/panes");
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    expect(spawnShell).toHaveBeenCalledTimes(1);
    expect(writeToSession).toHaveBeenCalledWith(
      expect.any(String),
      "npm run build\n"
    );
  });

  it("adds a task run to the store", async () => {
    const { initSessionPanes } = await import("$lib/stores/panes");
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    const runs = get(taskRuns).get("session-1");
    expect(runs).toHaveLength(1);
    expect(runs![0].taskId).toBe("npm:build");
    expect(runs![0].status).toBe("running");
  });

  it("listens for session exit", async () => {
    const { initSessionPanes } = await import("$lib/stores/panes");
    initSessionPanes("session-1");

    await runTask("session-1", "/repo", task);

    expect(onSessionExit).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm test -- --reporter verbose 2>&1 | head -40`
Expected: FAIL (module not found)

- [ ] **Step 3: Implement the runner**

Create `src/lib/tasks/runner.ts`:

```typescript
import { spawnShell, writeToSession, onSessionExit } from "$lib/tauri";
import { addSplit } from "$lib/stores/panes";
import {
  addTaskRun,
  updateTaskRun,
  removeTaskRun,
  getEffectiveKeepOpen,
} from "$lib/stores/tasks";
import { closePane } from "$lib/panes/actions";
import type { TaskDefinition } from "$lib/types/tasks";

export async function runTask(
  sessionId: string,
  repoRoot: string,
  task: TaskDefinition
): Promise<void> {
  const ptyId = `task-${sessionId}-${task.id}-${Date.now()}`;
  const paneId = ptyId;

  await spawnShell(ptyId, repoRoot);
  addSplit(sessionId, "horizontal", {
    id: paneId,
    type: "shell",
    ptyId,
  });

  await writeToSession(ptyId, task.command + "\n");

  addTaskRun(sessionId, {
    taskId: task.id,
    paneId,
    ptyId,
    status: "running",
    exitCode: null,
    startedAt: Date.now(),
  });

  await onSessionExit(ptyId, (code) => {
    updateTaskRun(sessionId, ptyId, code);
    const keepOpen = getEffectiveKeepOpen(repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && code === 0)) {
      // Delay briefly so the user sees the result
      setTimeout(() => {
        closePane(sessionId, paneId);
        removeTaskRun(sessionId, ptyId);
      }, 2000);
    }
  });
}
```

- [ ] **Step 4: Run tests**

Run: `npm test -- --reporter verbose`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/tasks/runner.ts src/lib/tasks/__tests__/runner.test.ts
git commit -m "feat(tasks): add task runner with pane lifecycle management"
```

---

### Task 9: Cmd+K integration

**Files:**
- Modify: `src/lib/commands/index.ts`

- [ ] **Step 1: Register the task.run command**

In `src/lib/commands/index.ts`, add imports at the top:

```typescript
import { get } from "svelte/store";
import { taskGroups } from "$lib/stores/tasks";
import { runTask } from "$lib/tasks/runner";
```

Add the command registration before the `// -- Simple commands` section (around line 152):

```typescript
  // -- Tasks --
  registry.register({
    id: "task.run",
    label: "Run Task",
    category: "Tasks",
    available: () => get(taskGroups).length > 0,
    getItems: () => {
      const session = queries.activeSession();
      if (!session) return [];
      const groups = get(taskGroups);
      return groups.flatMap((group) =>
        group.tasks.map((task) => ({
          id: task.id,
          label: task.name,
          description: `${group.runner} — ${task.description || task.command}`,
          action: () => {
            const activeId = queries.activeSessionId();
            if (activeId) void runTask(activeId, session.worktreePath, task);
          },
        }))
      );
    },
  });
```

- [ ] **Step 2: Verify compilation**

Run: `npm run check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/lib/commands/index.ts
git commit -m "feat(tasks): register task.run command in command palette"
```

---

### Task 10: TaskPanel component

**Files:**
- Create: `src/lib/components/TaskPanel.svelte`

- [ ] **Step 1: Create TaskPanel.svelte**

Create `src/lib/components/TaskPanel.svelte`:

```svelte
<script lang="ts">
  import { taskGroups, taskRuns, getTaskRun, setKeepOpenOverride, getEffectiveKeepOpen } from "$lib/stores/tasks";
  import { sessionState } from "$lib/stores/sessions";
  import { runTask } from "$lib/tasks/runner";
  import type { TaskDefinition } from "$lib/types/tasks";

  let collapsedGroups = $state(new Set<string>());
  let contextMenu = $state<{ x: number; y: number; task: TaskDefinition; repoRoot: string } | null>(null);

  const activeSession = $derived(
    $sessionState.sessions.find((s) => s.id === $sessionState.activeSessionId)
  );

  function toggleGroup(runner: string) {
    collapsedGroups = new Set(collapsedGroups);
    if (collapsedGroups.has(runner)) {
      collapsedGroups.delete(runner);
    } else {
      collapsedGroups.add(runner);
    }
  }

  function handleRun(task: TaskDefinition) {
    if (!activeSession || !$sessionState.activeSessionId) return;
    void runTask($sessionState.activeSessionId, activeSession.worktreePath, task);
  }

  function handleContextMenu(e: MouseEvent, task: TaskDefinition) {
    e.preventDefault();
    if (!activeSession) return;
    contextMenu = { x: e.clientX, y: e.clientY, task, repoRoot: activeSession.repoRoot };
  }

  function setKeepOpen(value: "always" | "on-error" | "never") {
    if (!contextMenu) return;
    setKeepOpenOverride(contextMenu.repoRoot, contextMenu.task.id, value);
    contextMenu = null;
  }

  function handleClickOutside() {
    contextMenu = null;
  }

  function getRunStatus(taskId: string): "running" | "succeeded" | "failed" | null {
    if (!$sessionState.activeSessionId) return null;
    const run = getTaskRun($sessionState.activeSessionId, taskId);
    return run?.status ?? null;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<div class="flex flex-col h-full">
  <div class="px-4 pt-2.5 pb-2 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Tasks</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$taskGroups.reduce((n, g) => n + g.tasks.length, 0)}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto px-2 scrollbar-thin">
    {#if $taskGroups.length === 0}
      <p class="text-xs text-text-muted text-center py-4">No tasks found</p>
    {:else}
      {#each $taskGroups as group (group.runner)}
        <button
          class="w-full flex items-center gap-1.5 px-2 py-1.5 text-[11px] font-semibold text-text-secondary uppercase tracking-wide cursor-pointer bg-transparent border-none hover:text-text-primary"
          onclick={() => toggleGroup(group.runner)}
        >
          <span class="text-[10px] transition-transform {collapsedGroups.has(group.runner) ? '' : 'rotate-90'}">&#9654;</span>
          {group.runner}
          <span class="font-mono text-[10px] text-text-muted font-normal normal-case tracking-normal ml-auto">{group.tasks.length}</span>
        </button>

        {#if !collapsedGroups.has(group.runner)}
          {#each group.tasks as task (task.id)}
            {@const status = getRunStatus(task.id)}
            <button
              class="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-text-secondary bg-transparent border-none cursor-pointer rounded hover:bg-bg-hover hover:text-text-primary group"
              onclick={() => handleRun(task)}
              oncontextmenu={(e) => handleContextMenu(e, task)}
              title={task.description || task.command}
            >
              <span class="flex-1 text-left truncate font-mono text-[12px]">{task.name}</span>
              {#if status === "running"}
                <span class="w-2 h-2 rounded-full bg-blue-400 animate-pulse shrink-0"></span>
              {:else if status === "succeeded"}
                <span class="w-2 h-2 rounded-full bg-green-400 shrink-0"></span>
              {:else if status === "failed"}
                <span class="w-2 h-2 rounded-full bg-red-400 shrink-0"></span>
              {:else}
                <span class="text-text-muted opacity-0 group-hover:opacity-100 text-[10px] shrink-0">&#9654;</span>
              {/if}
            </button>
          {/each}
        {/if}
      {/each}
    {/if}
  </div>
</div>

{#if contextMenu}
  <div
    class="fixed z-50 bg-bg-elevated border border-border rounded-md shadow-lg py-1 min-w-40"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
  >
    <div class="px-3 py-1.5 text-[11px] text-text-muted uppercase tracking-wide">Keep open</div>
    {#each [["always", "Always"], ["on-error", "On Error"], ["never", "Never"]] as [value, label]}
      {@const current = getEffectiveKeepOpen(contextMenu.repoRoot, contextMenu.task.id, contextMenu.task.keepOpen)}
      <button
        class="w-full text-left px-3 py-1.5 text-xs bg-transparent border-none cursor-pointer hover:bg-bg-hover text-text-secondary hover:text-text-primary flex items-center gap-2"
        onclick={() => setKeepOpen(value as "always" | "on-error" | "never")}
      >
        <span class="w-3 text-accent text-[10px]">{current === value ? "✓" : ""}</span>
        {label}
      </button>
    {/each}
  </div>
{/if}
```

- [ ] **Step 2: Verify compilation**

Run: `npm run check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/TaskPanel.svelte
git commit -m "feat(tasks): add TaskPanel sidebar component with context menu"
```

---

### Task 11: Integrate TaskPanel into SessionTabs

**Files:**
- Modify: `src/lib/components/SessionTabs.svelte`
- Modify: `src/lib/stores/settings.ts`
- Modify: `src-tauri/src/settings.rs:7-24`
- Modify: `src/lib/types.ts:21-38`

- [ ] **Step 1: Add taskPanelSplit to settings**

In `src-tauri/src/settings.rs`, add to the `RouxSettings` struct (after `additional_flags`):

```rust
    pub task_panel_split: f64,
    pub task_panel_collapsed: bool,
```

And in the `Default` impl:

```rust
            task_panel_split: 0.4,
            task_panel_collapsed: false,
```

In `src/lib/types.ts`, add to the `RouxSettings` interface (after `additionalFlags`):

```typescript
  taskPanelSplit: number;
  taskPanelCollapsed: boolean;
```

And to `DEFAULT_SETTINGS`:

```typescript
  taskPanelSplit: 0.4,
  taskPanelCollapsed: false,
```

- [ ] **Step 2: Integrate TaskPanel into SessionTabs**

Replace the content of `src/lib/components/SessionTabs.svelte` with:

```svelte
<script lang="ts">
  import SessionCard from "./SessionCard.svelte";
  import TaskPanel from "./TaskPanel.svelte";
  import { sessionState, setActiveSession, removeSession, renameSession } from "$lib/stores/sessions";
  import { removeSessionPanes } from "$lib/stores/panes";
  import { killSession, removeWorktree, writeToSession } from "$lib/tauri";
  import { settings, updateSetting } from "$lib/stores/settings";
  import { closeAuxiliaryPanes } from "$lib/panes/actions";
  import { disposeClaudeTerminal } from "$lib/panes/terminalRegistry";
  import { reconnectSession } from "$lib/sessions/reconnect";
  import { refreshTasks, initTaskOverrides } from "$lib/stores/tasks";

  interface Props {
    onNewSession: () => void;
    onOpenSettings: () => void;
  }

  let { onNewSession, onOpenSettings }: Props = $props();

  let dragging = $state(false);
  let containerEl: HTMLDivElement | undefined = $state();

  // Refresh tasks when active session changes
  $effect(() => {
    const session = $sessionState.sessions.find((s) => s.id === $sessionState.activeSessionId);
    if (session) {
      void refreshTasks(session.worktreePath);
    }
  });

  // Load overrides on mount
  $effect(() => {
    void initTaskOverrides();
  });

  async function handleClose(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;

    if (
      $settings.confirmOnClose &&
      (session.status === "thinking" || session.status === "generating")
    ) {
      const confirmed = window.confirm(
        `"${session.name}" is currently ${session.status}. Close it?`
      );
      if (!confirmed) return;
    }

    await closeAuxiliaryPanes(id);
    await disposeClaudeTerminal(id);
    await killSession(id);

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

    removeSessionPanes(id);
    removeSession(id);
  }

  async function handleApprove(id: string) {
    await writeToSession(id, "\r");
  }

  async function handleAlways(id: string) {
    await writeToSession(id, "\x1b[Z");
  }

  async function handleDeny(id: string) {
    await writeToSession(id, "\x1b[B\x1b[B\r");
  }

  async function handleReconnect(id: string) {
    const session = $sessionState.sessions.find((s) => s.id === id);
    if (!session) return;
    await reconnectSession(session);
  }

  function handleDividerDown(e: MouseEvent) {
    e.preventDefault();
    dragging = true;

    function onMove(ev: MouseEvent) {
      if (!containerEl) return;
      const rect = containerEl.getBoundingClientRect();
      const ratio = (ev.clientY - rect.top) / rect.height;
      const clamped = Math.max(0.15, Math.min(0.85, ratio));
      updateSetting("taskPanelSplit", 1 - clamped);
    }

    function onUp() {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>

<div class="h-full flex flex-col bg-bg-base border-r border-border-subtle" bind:this={containerEl}>
  <div class="px-4 pt-3.5 pb-2.5 flex items-center justify-between">
    <span class="text-[11px] font-semibold uppercase tracking-widest text-text-muted">Sessions</span>
    <span class="font-mono text-[10px] text-text-muted bg-bg-elevated px-1.5 py-0.5 rounded">
      {$sessionState.sessions.length}
    </span>
  </div>

  <div class="overflow-y-auto px-2 scrollbar-thin" style="flex: {1 - $settings.taskPanelSplit};">
    {#each $sessionState.sessions as session (session.id)}
      <SessionCard
        {session}
        active={session.id === $sessionState.activeSessionId}
        onselect={() => setActiveSession(session.id)}
        onclose={() => handleClose(session.id)}
        onrename={(newName) => renameSession(session.id, newName)}
        onreconnect={() => handleReconnect(session.id)}
        onapprove={() => handleApprove(session.id)}
        onalways={() => handleAlways(session.id)}
        ondeny={() => handleDeny(session.id)}
      />
    {/each}
  </div>

  {#if !$settings.taskPanelCollapsed}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="h-px bg-border-subtle cursor-row-resize hover:bg-accent-dim transition-colors shrink-0 {dragging ? 'bg-accent' : ''}"
      onmousedown={handleDividerDown}
    ></div>

    <div style="flex: {$settings.taskPanelSplit}; min-height: 0;">
      <TaskPanel />
    </div>
  {/if}

  <div class="p-2 border-t border-border-subtle flex gap-1 shrink-0">
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

- [ ] **Step 3: Check updateSetting export exists in settings store**

Verify `src/lib/stores/settings.ts` exports `updateSetting`. If it uses a different name, adjust the import accordingly.

- [ ] **Step 4: Verify compilation**

Run: `npm run check && cd src-tauri && RUSTFLAGS="-D warnings" cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/settings.rs src/lib/types.ts src/lib/components/SessionTabs.svelte src/lib/components/TaskPanel.svelte src/lib/stores/settings.ts
git commit -m "feat(tasks): integrate TaskPanel into sidebar with draggable divider"
```

---

### Task 12: End-to-end verification

**Files:** (none new)

- [ ] **Step 1: Run all Rust tests**

Run: `cd src-tauri && RUSTFLAGS="-D warnings" cargo test`
Expected: All tests pass, zero warnings.

- [ ] **Step 2: Run all frontend tests**

Run: `npm test`
Expected: All tests pass.

- [ ] **Step 3: Run svelte-check**

Run: `npm run check`
Expected: No errors or warnings.

- [ ] **Step 4: Verify Tauri builds**

Run: `npm run build`
Expected: Frontend builds successfully.

- [ ] **Step 5: Manual smoke test**

Start the dev server with `npm run tauri dev` and verify:
1. Task panel appears below sessions in sidebar
2. Tasks from the repo's `package.json` and `Taskfile.yml` are listed
3. Clicking a task spawns a shell pane and runs the command
4. Cmd+K → "Run Task" shows available tasks
5. Right-click on a task shows keepOpen context menu
6. Dragging the divider between sessions and tasks works

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "feat(tasks): task runner system — auto-discovery, Cmd+K, sidebar panel"
```
