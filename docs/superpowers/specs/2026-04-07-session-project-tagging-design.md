# Session Project Tagging

## Summary

Add the ability to tag sessions with a "project" -- a named entity that can be shared across multiple sessions and repos. The sidebar can toggle between grouping sessions by repo (current behavior) or by project.

## Data Model

### Project type

```rust
// src-tauri/src/projects.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,       // nanoid/uuid
    pub name: String,
}
```

```ts
// src/lib/types.ts
export interface Project {
  id: string;
  name: string;
}
```

### Storage

Projects persist to `~/Library/Application Support/roux/projects.json` (or platform equivalent via `dirs::config_dir()`). Uses the same dirty-flag + background-thread pattern as `SessionStore`.

### Session changes

Add `project_id: Option<String>` to the Rust `Session` struct and `projectId: string | null` to the TS `Session` interface. Default `null`. Persists automatically with the existing session store.

## Rust Backend

### ProjectStore (`src-tauri/src/projects.rs`)

Same pattern as `SessionStore`:
- `load_persisted()` -- reads `projects.json` from config dir
- Background persist thread (500ms dirty check)
- `add(project) -> Project`
- `remove(id)`
- `rename(id, name)`
- `list() -> Vec<Project>`

### Tauri Commands

```rust
#[tauri::command]
fn list_projects(state: State<ProjectStore>) -> Vec<Project>;

#[tauri::command]
fn create_project(state: State<ProjectStore>, name: String) -> Project;

#[tauri::command]
fn remove_project(state: State<ProjectStore>, id: String);

#[tauri::command]
fn rename_project(state: State<ProjectStore>, id: String, name: String);

#[tauri::command]
fn set_session_project(session_state: State<SessionStore>, session_id: String, project_id: Option<String>);
```

`set_session_project` updates the session's `project_id` field in the session store.

### Registration

Add `ProjectStore` as managed state in `main.rs`. Register all new commands in the invoke handler.

## Frontend

### Projects Store (`src/lib/stores/projects.ts`)

```ts
export const projects = writable<Project[]>([]);

export async function loadProjects(): Promise<void>;    // calls list_projects
export async function createProject(name: string): Promise<Project>;
export async function removeProject(id: string): Promise<void>;
export async function renameProject(id: string, name: string): Promise<void>;
```

Load projects on app init alongside session restore.

### Session Store Changes

Add to `src/lib/stores/sessions.ts`:

```ts
export function setSessionProject(id: string, projectId: string | null) {
  sessionState.update((state) => ({
    ...state,
    sessions: state.sessions.map((s) =>
      s.id === id ? { ...s, projectId } : s
    ),
  }));
  // Also call Tauri command to persist
  invoke("set_session_project", { sessionId: id, projectId });
}
```

### Settings Change

Add `groupBy: "repo" | "project"` to `RouxSettings` (both Rust and TS), default `"repo"`.

### SessionTabs.svelte Grouping

The existing `grouped` derived groups by `repoRoot`. Add a second derived that groups by project:

```ts
let groupedByProject = $derived.by(() => {
  const map = new Map<string, { name: string; projectId: string | null; sessions: Session[]; latest: number }>();
  for (const s of $sessionState.sessions) {
    const key = s.projectId ?? "__untagged__";
    let group = map.get(key);
    if (!group) {
      const project = $projects.find(p => p.id === s.projectId);
      group = {
        name: project?.name ?? "Untagged",
        projectId: s.projectId,
        sessions: [],
        latest: 0,
      };
      map.set(key, group);
    }
    group.sessions.push(s);
    if (s.createdAt > group.latest) group.latest = s.createdAt;
  }
  const groups = [...map.values()].sort((a, b) => b.latest - a.latest);
  // Move "Untagged" to the bottom
  const untaggedIdx = groups.findIndex(g => g.projectId === null);
  if (untaggedIdx > 0) {
    const [untagged] = groups.splice(untaggedIdx, 1);
    groups.push(untagged);
  }
  return groups;
});
```

Switch between `grouped` and `groupedByProject` based on `$settings.groupBy`.

### Sidebar Toggle

Add a small toggle control in the sidebar header (next to the session count badge) that switches between repo and project grouping. Clicking it toggles `settings.groupBy`.

### Context Menu

Add a "Set Project" option to the existing right-click context menu in `SessionTabs.svelte`. When clicked, it shows a submenu with:
- List of existing projects (clicking one assigns it)
- "New Project..." option that shows inline text input (same pattern as worktree branch input)
- "Remove Project" option (only shown if session has a project) that sets `projectId` to `null`

### Command Palette

Add a `session.set-project` command in `src/lib/commands/index.ts`:

```ts
registry.register({
  id: "session.set-project",
  label: "Set Project",
  category: "Sessions",
  available: () => !!queries.activeSession(),
  getItems: async () => {
    const projectList = await listProjects();
    const items = projectList.map(p => ({
      id: p.id,
      label: p.name,
      action: () => {
        const session = queries.activeSession();
        if (session) setSessionProject(session.id, p.id);
      },
    }));
    // Add "Remove" option if session has a project
    const session = queries.activeSession();
    if (session?.projectId) {
      items.unshift({
        id: "__remove__",
        label: "Remove Project",
        action: () => setSessionProject(session.id, null),
      });
    }
    return items;
  },
  inputPlaceholder: "Pick a project or type to create...",
  onInput: async (name: string) => {
    const session = queries.activeSession();
    if (!session) return;
    const project = await createProject(name);
    setSessionProject(session.id, project.id);
  },
});
```

### Tauri Bridge

Add to `src/lib/tauri.ts`:

```ts
export function listProjects(): Promise<Project[]>;
export function createProject(name: string): Promise<Project>;
export function removeProject(id: string): Promise<void>;
export function renameProject(id: string, name: string): Promise<void>;
export function setSessionProject(sessionId: string, projectId: string | null): Promise<void>;
```

## Testing

- Unit tests for `ProjectStore` in Rust (add/remove/rename/persist round-trip)
- Unit tests for frontend `projects` store (create, assign, remove)
- Test grouping logic: sessions with projects group correctly, untagged goes to bottom
- Test that removing a project from a session moves it to "Untagged" group

## Files to Create

- `src-tauri/src/projects.rs` -- ProjectStore and Tauri commands

## Files to Modify

- `src-tauri/src/main.rs` -- register ProjectStore state and commands
- `src-tauri/src/session.rs` -- add `project_id` field to Session struct
- `src-tauri/src/settings.rs` -- add `group_by` field to RouxSettings
- `src/lib/types.ts` -- add Project interface, projectId to Session, groupBy to RouxSettings
- `src/lib/stores/projects.ts` (new) -- frontend projects store
- `src/lib/stores/sessions.ts` -- add setSessionProject function
- `src/lib/stores/settings.ts` -- if groupBy needs defaults
- `src/lib/tauri.ts` -- add project-related Tauri invoke wrappers
- `src/lib/commands/index.ts` -- add session.set-project command
- `src/lib/components/SessionTabs.svelte` -- grouping toggle, context menu additions, project grouping derived
- `src/lib/components/SessionCard.svelte` -- show project name badge (subtle, in the metadata row)
