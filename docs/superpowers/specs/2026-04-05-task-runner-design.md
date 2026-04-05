# Task Runner System Design

Auto-discover runnable tasks from repo config files, expose them via Cmd+K and a sidebar panel, and execute them in shell panes with configurable lifecycle behavior.

## Goals

1. **Auto-discover tasks** from `package.json`, `Taskfile.yml`, `Makefile`, and `Justfile`
2. **Run tasks in shell panes** — each task spawns its own shell pane in the active session
3. **Cmd+K integration** — tasks appear as runnable commands in the command palette
4. **Sidebar task panel** — collapsible section below sessions showing tasks for the active session's repo
5. **Configurable pane lifecycle** — tasks can keep their pane open always, on error, or never
6. **Extensible for custom task definitions** — architecture supports a future `roux.tasks.json` discoverer without changes

## Data Model

### TaskDefinition

```typescript
interface TaskDefinition {
  id: string;              // "npm:build", "make:clean", "task:deploy", "just:fmt"
  name: string;            // "build", "clean", "deploy", "fmt"
  description: string;     // from config file, or empty string
  runner: string;          // "npm" | "make" | "task" | "just"
  command: string;         // full shell command: "npm run build", "make clean"
  keepOpen: "always" | "on-error" | "never";  // default "on-error"
}
```

### TaskGroup

```typescript
interface TaskGroup {
  runner: string;          // display label: "npm scripts", "Makefile", "Taskfile", "Justfile"
  configFile: string;      // relative path: "package.json", "Makefile"
  tasks: TaskDefinition[];
}
```

### TaskRun

```typescript
interface TaskRun {
  taskId: string;          // matches TaskDefinition.id
  paneId: string;          // shell pane hosting this run
  ptyId: string;           // PTY session ID
  status: "running" | "succeeded" | "failed";
  exitCode: number | null;
  startedAt: number;       // unix timestamp
}
```

### KeepOpen Override

Per-repo overrides stored in `~/.config/roux/task-overrides.json`:

```json
{
  "/Users/me/projects/roux": {
    "npm:build": "always",
    "make:clean": "never"
  }
}
```

Keyed by repo root path. Only stores overrides — tasks without an entry use the discovered default (`"on-error"`).

## Backend: Rust

### New module: `src-tauri/src/tasks.rs`

#### Discovery trait

```rust
trait TaskDiscoverer {
    fn config_file(&self) -> &str;
    fn discover(&self, dir: &Path) -> Option<TaskGroup>;
}
```

#### Implementations

**NpmDiscoverer**
- Looks for `package.json`
- Parses JSON, reads `scripts` object keys
- No descriptions (npm scripts don't have them)
- Command: `npm run {name}`

**TaskfileDiscoverer**
- Looks for `Taskfile.yml`
- Parses YAML, reads `tasks` map keys + `desc` fields
- Command: `task {name}`

**MakeDiscoverer**
- Looks for `Makefile`
- Regex scan for target lines: `^([a-zA-Z0-9_-]+):` (excluding lines starting with `.` or `_`)
- Description from `## comment` line immediately preceding the target
- Filters out common internal targets (`.PHONY`, `.DEFAULT`, etc.)
- Command: `make {name}`

**JustDiscoverer**
- Looks for `Justfile` (case-insensitive)
- Regex scan for recipe lines: `^([a-zA-Z0-9_-]+):` or `^([a-zA-Z0-9_-]+) .*:`
- Description from `# comment` line immediately preceding the recipe
- Command: `just {name}`

#### Top-level function

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

### New Tauri commands

- `discover_tasks(dir: String) -> Vec<TaskGroup>` — runs all discoverers on the directory
- `load_task_overrides() -> HashMap<String, HashMap<String, String>>` — reads override file
- `save_task_overrides(overrides: HashMap<String, HashMap<String, String>>)` — writes override file

### Override persistence

File: `~/.config/roux/task-overrides.json`
Read/write with serde_json, same pattern as `settings.rs`. No in-memory caching on the Rust side — the frontend owns the state and calls save when overrides change.

## Frontend

### New store: `src/lib/stores/tasks.ts`

```typescript
import { writable } from "svelte/store";

// Discovered task groups for the active session's repo
export const taskGroups = writable<TaskGroup[]>([]);

// Active task runs per session: Map<sessionId, TaskRun[]>
export const taskRuns = writable<Map<string, TaskRun[]>>(new Map());

// Per-repo keepOpen overrides: Map<repoRoot, Record<taskId, KeepOpen>>
export const taskOverrides = writable<Map<string, Record<string, KeepOpen>>>(new Map());

// Discovery cache: Map<repoRoot, TaskGroup[]>
const discoveryCache = new Map<string, TaskGroup[]>();
```

- `refreshTasks(repoRoot)` — calls `discover_tasks`, updates `taskGroups`, populates cache
- `getEffectiveKeepOpen(repoRoot, taskId, defaultKeepOpen)` — returns override if set, else default
- On active session change: load tasks from cache or discover fresh
- On override change: debounced save to backend

### New module: `src/lib/tasks/runner.ts`

```typescript
export async function runTask(sessionId: string, task: TaskDefinition): Promise<void> {
  const ptyId = `task-${sessionId}-${task.id}-${Date.now()}`;

  // 1. Spawn shell pane via existing infrastructure
  await spawnShell(ptyId, session.worktreePath);
  addSplit(sessionId, "horizontal", {
    id: ptyId,
    type: "shell",
    ptyId,
  });

  // 2. Write task command to the PTY
  await writeToSession(ptyId, task.command + "\n");

  // 3. Track the run
  addTaskRun(sessionId, {
    taskId: task.id,
    paneId: ptyId,
    ptyId,
    status: "running",
    exitCode: null,
    startedAt: Date.now(),
  });

  // 4. Listen for exit
  await onSessionExit(ptyId, (code) => {
    updateTaskRun(sessionId, ptyId, code);
    const keepOpen = getEffectiveKeepOpen(session.repoRoot, task.id, task.keepOpen);
    if (keepOpen === "never" || (keepOpen === "on-error" && code === 0)) {
      closePane(sessionId, ptyId);
    }
  });
}
```

### New component: `src/lib/components/TaskPanel.svelte`

Lives inside `SessionTabs.svelte`, below the session list, above the bottom buttons.

**Layout:**
```
├─────────────────────────┤  <- draggable divider
| TASKS                   |
├─────────────────────────┤
| > npm scripts           |  <- collapsible group header
|   build        [>] [*]  |  <- [>] play on hover, [*] running indicator
|   lint         [>]      |
|   test         [>]      |
| > Makefile              |
|   clean        [>]      |
|   deploy       [>]      |
└─────────────────────────┘
```

**Interactions:**
- Click play button (or click the task row): runs the task
- Running tasks show a spinner; completed tasks flash green (success) or red (failure) briefly
- Click a running task row: focuses its shell pane
- Right-click any task row: context menu with keepOpen options ("Keep open: Always / On Error / Never") with checkmark on current value
- Groups default to expanded; collapse state is ephemeral (not persisted)
- Empty state: "No tasks found" with subtle text

**Draggable divider:**
- Same pattern as the existing sidebar resize handle
- Persists relative split as a percentage in settings (e.g., `taskPanelSplit: 0.5`)

### Cmd+K integration

In `src/lib/commands/index.ts`, register a new command:

```typescript
registry.register({
  id: "task.run",
  label: "Run Task",
  shortcut: undefined,
  category: "Tasks",
  available: () => get(taskGroups).length > 0,
  getItems: () => {
    const groups = get(taskGroups);
    return groups.flatMap((group) =>
      group.tasks.map((task) => ({
        id: task.id,
        label: task.name,
        description: `${group.runner} — ${task.description || task.command}`,
        action: () => {
          const activeId = queries.activeSessionId();
          if (activeId) runTask(activeId, task);
        },
      }))
    );
  },
});
```

Tasks appear when the user types in the palette and are grouped under the "Tasks" category. The description line shows the runner and either the task description or the raw command.

### Exit handling

The `onSessionExit` listener for task PTYs:
1. Updates `TaskRun.status` to `"succeeded"` (code 0) or `"failed"` (non-zero)
2. Sets `TaskRun.exitCode`
3. Checks effective `keepOpen` for this task
4. If pane should close, calls `closePane` (which handles terminal disposal via existing infrastructure)
5. Removes the `TaskRun` entry after a short delay (2s) so the UI can show the result briefly

### Settings additions

Add to `RouxSettings`:
- `taskPanelSplit: number` — percentage of sidebar height for the task panel (default `0.4`)
- `taskPanelCollapsed: boolean` — whether the panel is collapsed (default `false`)

## File inventory

### New files
- `src-tauri/src/tasks.rs` — discovery trait, four discoverers, Tauri commands
- `src/lib/stores/tasks.ts` — task groups, runs, overrides stores
- `src/lib/tasks/runner.ts` — runTask, exit handling
- `src/lib/components/TaskPanel.svelte` — sidebar task panel with context menu
- `src/lib/stores/__tests__/tasks.test.ts` — store unit tests
- `src/lib/tasks/__tests__/runner.test.ts` — runner unit tests
- `src-tauri/src/tasks/` tests alongside the module

### Modified files
- `src-tauri/src/main.rs` — add `mod tasks`, register new Tauri commands
- `src/lib/types.ts` — add TaskDefinition, TaskGroup, TaskRun, KeepOpen types
- `src/lib/tauri.ts` — add discover_tasks, load/save_task_overrides bindings
- `src/lib/commands/index.ts` — register `task.run` command
- `src/lib/components/SessionTabs.svelte` — integrate TaskPanel below sessions
- `src/lib/components/Layout.svelte` — no changes expected (TaskPanel is inside SessionTabs)
- `src/lib/stores/settings.ts` — add taskPanelSplit, taskPanelCollapsed defaults
- `src-tauri/src/settings.rs` — add new setting fields

## Future: Custom task definitions (goal 3)

Adding `roux.tasks.json` support requires only:
1. A new `RouxTaskDiscoverer` implementing the same `TaskDiscoverer` trait
2. Add it to the discoverers list in `discover_tasks`
3. The `keepOpen` field is already part of `TaskDefinition`, so custom tasks can set it directly

No architectural changes needed. The override system still works — user overrides take precedence over the file's declared defaults.
