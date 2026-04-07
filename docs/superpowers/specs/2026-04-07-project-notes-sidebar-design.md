# Project Notes Sidebar

## Overview

A plain-text notes panel that slides in from the right side of the screen, scoped per-project. All sessions sharing a project see the same notes. Toggled via `Cmd+B` or the command palette.

## Backend

### Storage

Notes are stored as individual text files in the Roux config directory alongside `projects.json`:

```
~/Library/Application Support/roux/notes/<project_id>.txt
```

Each project gets one file. The file is created on first write. No file = empty notes.

### Tauri Commands

Two new commands in a `notes` module (or added to `projects.rs`):

```rust
#[tauri::command]
fn get_project_notes(project_id: String) -> Result<String, String>
```

Reads `notes/<project_id>.txt` from the config directory. Returns empty string if file doesn't exist.

```rust
#[tauri::command]
fn set_project_notes(project_id: String, content: String) -> Result<(), String>
```

Writes `content` to `notes/<project_id>.txt`. Creates the `notes/` directory if needed.

No in-memory caching or dirty-flag thread needed — notes are simple read/write operations and the frontend debounces saves.

## Frontend

### NotesPanel.svelte

New component following the same pattern as `SettingsPanel.svelte`:

- **Position**: Absolute, right side, full height, 380px wide, z-50
- **Animation**: `translate-x` transition (same as SettingsPanel)
- **Header**: Title "Notes" with close button (same style as SettingsPanel header)
- **Body**: A `<textarea>` that fills the remaining space, styled to match the app theme (bg-bg-deep, text-text-primary, monospace font)
- **Props**: `visible: boolean`, `projectId: string | null`, `onclose: () => void`

Behavior:
- When `projectId` is non-null and panel becomes visible: call `get_project_notes(projectId)` to load content
- On text change: debounced save (500ms) via `set_project_notes(projectId, content)`
- When `projectId` is null: show a centered message "Assign a project to this session to use notes"
- When `projectId` changes while visible: load the new project's notes

### Command Registration

In `src/lib/commands/index.ts`, register:

```ts
registry.register({
  id: "ui.toggle-notes",
  label: "Toggle Notes",
  shortcut: "cmd+b",
  category: "App",
  available: () => !!queries.activeSession()?.projectId,
  execute: () => { /* toggle showNotes state */ },
});
```

### App.svelte Wiring

- New `showNotes` state variable (same pattern as `showSettings`)
- In `handleKeyDown`, the existing shortcut dispatch handles `cmd+b` via the registry
- The `ui.toggle-notes` command's `execute` callback toggles `showNotes`
- `NotesPanel` rendered alongside `SettingsPanel` in the template, receiving `visible={showNotes}`, the active session's `projectId`, and an `onclose` handler
- Opening notes closes settings, and vice versa (they occupy the same screen space)

## What This Does NOT Include

- Rich text or markdown rendering (plain text only)
- Multiple notes per project
- Note titles or folder organization
- Real-time sync between windows (single-window app)
- Note content search
