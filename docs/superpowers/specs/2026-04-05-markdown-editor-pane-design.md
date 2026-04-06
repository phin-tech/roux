# Markdown Editor Pane

## Summary

Replace the existing read-only `DocPane` with a full CodeMirror 6 markdown editor pane. Supports editing files on disk, scratchpad tabs, and optional vim keybindings.

## Pane Type

- New type: `"markdown"` replaces `"doc"` in the `Pane.type` union
- Existing `DocPane.svelte` is removed
- All references to `type: "doc"` are updated to `type: "markdown"`

## Component: `MarkdownPane.svelte`

### Layout

```
+----------------------------------------------+
| [file.md] [Untitled] [+]              [vim] |
|----------------------------------------------|
|                                              |
|  CodeMirror editor                           |
|  (full height, markdown syntax highlighting) |
|                                              |
+----------------------------------------------+
```

### Tab Bar

- Minimal tab bar at top showing filename or "Untitled" for scratchpads
- Close button (x) per tab
- `+` button to create a new scratchpad tab
- Dirty indicator (dot before filename) when tab has unsaved changes
- Clicking a tab switches the editor to that tab's content

### Editor

- CodeMirror 6 with `@codemirror/lang-markdown` for syntax highlighting
- Line wrapping enabled
- Follows the app's existing font settings (family and size from settings store)
- Theme derived from the app's current theme

### Vim Mode

- Toggle button in the tab bar (right side) to enable/disable vim keybindings
- Uses `@replit/codemirror-vim` package
- Vim mode preference persisted (component-local, or settings store if one exists for editor prefs)
- Visual indicator when vim mode is active

### Tabs State

Each tab:
```typescript
interface EditorTab {
  id: string;
  filePath: string | null;  // null = scratchpad
  content: string;
  dirty: boolean;
}
```

- State is component-local (not a global store)
- Scratchpads have `filePath: null`
- On save of a scratchpad, prompt for file path via Tauri save dialog

### File Operations

- **Open file**: File picker via Tauri dialog API, opens as new tab
- **Save** (`Cmd+S`): Write to `filePath`, or prompt for path if scratchpad
- **Close tab**: If dirty, no confirmation for now (keep it simple)

## Pane Integration

### stores/panes.ts

- Update `Pane.type` union: replace `"doc"` with `"markdown"`
- `Pane` interface keeps `docPath?: string` for opening a specific file on pane creation

### SplitPane.svelte

- Replace `DocPane` rendering block with `MarkdownPane`
- Same props pattern: `active`, `onClose`, pane data

### Opening the Pane

- Command palette action to open markdown editor
- Keyboard shortcut (reuse whatever DocPane used, or assign new one)
- When opened with a `docPath`, that file loads in the first tab

## Dependencies

New npm packages:
- `codemirror` (core)
- `@codemirror/lang-markdown`
- `@codemirror/language-data` (for fenced code block highlighting)
- `@replit/codemirror-vim`

Tauri APIs used:
- `@tauri-apps/plugin-dialog` (open/save file dialogs)
- `@tauri-apps/plugin-fs` (read/write files) — or existing Tauri commands if the app wraps these

## What This Does NOT Include

- No live preview / split preview mode
- No LSP or autocomplete
- No git integration or diff view
- No tab reordering via drag-and-drop
- No confirmation dialog on closing dirty tabs (keep simple for now)

## Migration

- Remove `DocPane.svelte`
- Remove any doc-pane-specific auto-refresh logic
- Update all code that creates `type: "doc"` panes to create `type: "markdown"` instead
- If DocPane had a file browser/picker UI, replace with file-open dialog
