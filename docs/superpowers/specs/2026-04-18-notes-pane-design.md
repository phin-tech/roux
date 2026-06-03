# Notes Pane Design

## Context

The notes panel currently exists as a sidebar component. Users want the same functionality available as a pane type, allowing notes to be split alongside terminals and other content in the layout tree.

## Requirements

- Notes pane functions like the sidebar panel but lives in the pane system
- Each pane has independent scope selection (session/repo/project/global)
- Each pane has independent view mode (edit/read)
- Session scope shows notes for the session that owns the layout
- Created via command palette with keybinding support
- Split direction (horizontal/vertical) configurable

## Architecture

### Component Extraction

Extract shared logic from `NotesPanel.svelte` into `NotesContent.svelte`:

```
NotesContent.svelte
├── Scope selector (dropdown)
├── View mode toggle (edit/read)
├── Edit mode: textarea with debounced auto-save
├── Read mode: rendered entries with collapsible cards
└── Live reload on "notes-changed" event

NotesPanel.svelte (sidebar wrapper)
├── Uses NotesContent
├── Close button
└── Sidebar-specific styling

NotesPane.svelte (pane wrapper)
├── Uses NotesContent
└── Pane-specific styling
```

### Props Interface for NotesContent

```typescript
interface Props {
  sessionId: string;
  projectId: string | null;
  projectName: string | null;
  repoRoot: string | null;
  scope: NotesScope;
  viewMode: "edit" | "read";
  onScopeChange: (scope: NotesScope) => void;
  onViewModeChange: (mode: "edit" | "read") => void;
}
```

## Data Model

### Pane Type Addition

File: `src/lib/panes/instances.ts`

```typescript
export type PaneType = "shell" | "markdown" | "command" | "notes";
```

### PaneInstance Fields

For `type === "notes"`:

```typescript
{
  id: string;
  type: "notes";
  ptyId: "";                          // empty, no terminal
  name?: string;                       // e.g. "Notes"
  notesScope?: NotesScope;             // "session" | "repo" | "project" | "global"
  notesViewMode?: "edit" | "read";
}
```

### Persistence

Scope and view mode are persisted with the pane descriptor and restored on session reconnect. The sidebar continues using the existing `notesUi.ts` store (session-keyed).

## Pane Rendering

File: `src/lib/components/PaneShell.svelte`

Add conditional block for notes type:

```svelte
{:else if instance.type === "notes"}
  <NotesPane
    paneId={instance.id}
    sessionId={session.id}
    projectId={session.projectId}
    projectName={session.projectName}
    repoRoot={session.repoRoot}
    scope={instance.notesScope ?? "session"}
    viewMode={instance.notesViewMode ?? "edit"}
  />
```

File: `src/lib/components/PaneShell.svelte` (label mapping)

```typescript
case "notes": return "notes";
```

## State Updates

When user changes scope or view mode in the notes pane UI:

1. `NotesPane` calls `updateInstance(paneId, { notesScope: newScope })` or `updateInstance(paneId, { notesViewMode: newMode })`
2. `updateInstance` updates the store and calls `syncPaneRecord()` to persist to backend
3. `NotesContent` receives new prop values and reloads content if scope changed

Scope/mode commands follow the same flow, targeting the focused pane's ID.

## Session Reconnect

File: `src/lib/sessions/reconnect.ts`

Add handling in `rehydratePane()`:

```typescript
if (descriptor.type === "notes") {
  createPane({
    id: paneId,
    type: "notes",
    ptyId: "",
    name: descriptor.name ?? "Notes",
    notesScope: descriptor.notesScope ?? "session",
    notesViewMode: descriptor.notesViewMode ?? "edit",
  });
  return;
}
```

Update `knownTypes` set:

```typescript
const knownTypes = new Set(["shell", "command", "markdown", "notes"]);
```

## Commands

File: `src/lib/commands/panes.ts`

### Creation Commands

| Command ID                   | Label                        | Behavior                |
| ---------------------------- | ---------------------------- | ----------------------- |
| `pane.open-notes-horizontal` | Open Notes Pane (Horizontal) | Split H, new notes pane |
| `pane.open-notes-vertical`   | Open Notes Pane (Vertical)   | Split V, new notes pane |

**Defaults for new panes:** `notesScope: "session"`, `notesViewMode: "edit"`

### Scope/Mode Commands

| Command ID                    | Label                   | Behavior                          |
| ----------------------------- | ----------------------- | --------------------------------- |
| `pane.notes-show-session`     | Notes: Session Scope    | Set focused notes pane to session |
| `pane.notes-show-repo`        | Notes: Repo Scope       | Set focused notes pane to repo    |
| `pane.notes-show-project`     | Notes: Project Scope    | Set focused notes pane to project |
| `pane.notes-show-global`      | Notes: Global Scope     | Set focused notes pane to global  |
| `pane.notes-toggle-view-mode` | Notes: Toggle Edit/Read | Toggle mode on focused notes pane |

### Command Behavior

- Scope/mode commands are no-op if focused pane isn't a notes pane
- Repo command disabled when session lacks `repoRoot`
- Project command disabled when session lacks `projectId`

## Files to Modify

| File                                     | Change                                                               |
| ---------------------------------------- | -------------------------------------------------------------------- |
| `src/lib/panes/instances.ts`             | Add `"notes"` to `PaneType`, add `notesScope`/`notesViewMode` fields |
| `src/lib/panes/persistence.ts`           | Add notes fields to `PaneDescriptor`                                 |
| `src/lib/tauri.ts`                       | Add notes fields to `PaneDescriptorPayload`                          |
| `src/lib/components/NotesPanel.svelte`   | Extract core logic to `NotesContent.svelte`                          |
| `src/lib/components/NotesContent.svelte` | New: shared notes UI component                                       |
| `src/lib/components/NotesPane.svelte`    | New: pane wrapper for `NotesContent`                                 |
| `src/lib/components/PaneShell.svelte`    | Add notes type rendering + label                                     |
| `src/lib/sessions/reconnect.ts`          | Add notes type to rehydration + known types                          |
| `src/lib/commands/panes.ts`              | Add creation and scope/mode commands                                 |

## Verification

1. Create a notes pane via command palette (both H and V directions)
2. Verify scope selector works (session/repo/project/global)
3. Verify edit/read toggle works
4. Verify content persists across scope changes
5. Verify two notes panes in same session can have different scopes
6. Verify pane state (scope, mode) persists across session reconnect
7. Verify scope/mode commands work on focused notes pane
8. Verify sidebar panel still works independently
9. Run `npm run check` and `npm run test`
