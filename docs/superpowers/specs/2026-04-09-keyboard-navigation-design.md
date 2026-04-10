# Keyboard Navigation Design

## Context

Keyboard navigation in Roux is currently inconsistent. Pane management has solid vim-style shortcuts (Alt+HJKL, Cmd+D, etc.) and the command palette has full keyboard support, but sidebar lists (sessions, tasks, watches) are mouse-only. There is no unified interaction model that a user can learn once and apply everywhere.

This design establishes a region-modal keyboard navigation system targeting vim-first users. The same pattern will ship as a shared TypeScript/Svelte package (`@phin-tech/keynav`) reusable across Phin Tech apps (Roux, RedPen).

Resolves: https://github.com/phin-tech/roux/issues/6

---

## Interaction Model: Region-Modal

The app is divided into **focus regions**. Exactly one region is active at any time. Each region owns a local keybinding vocabulary while active. The pane area (terminal mode) is the default and home region.

### Modes

There are three distinct keyboard modes:

| Mode | Entry | Exit | Key routing |
|------|-------|------|-------------|
| **Terminal mode** | Default / Escape from anywhere | Enter command mode or a region | Keys pass through to xterm |
| **Command mode** | Ctrl+Space from terminal mode | Escape / i / Enter → terminal mode | Bare keys control panes |
| **List region** | Toggle key (Cmd+E, Cmd+Shift+W, etc.) or bare key in command mode | Escape → terminal mode | Bare keys navigate the list |

### Escape behavior

Escape **always** returns to terminal mode, regardless of current state. There is no intermediate stop in command mode when exiting a list region.

- List region → Escape → Terminal mode
- Command mode → Escape → Terminal mode
- Command mode → `e` (enter sessions) → List region → Escape → Terminal mode

---

## Focus Regions

| Region | Enter (global) | Enter (command mode) | Exit | List type |
|--------|---------------|---------------------|------|-----------|
| **Pane area** | Escape from anywhere | Escape / i / Enter | Toggle into another region | Spatial (Alt+HJKL) |
| **Sessions** | Cmd+E | `e` | Escape → terminal | Tree (groups + items) |
| **Tasks** | Cmd+Shift+T | `t` | Escape → terminal | Tree (groups + items) |
| **Watches** | Cmd+Shift+W | `w` | Escape → terminal | Expandable list |
| **Notes** | Cmd+B | `b` | Escape → terminal | Text editor (standard keys) |
| **Settings** | Cmd+, | `,` | Escape → terminal | Form (Tab between fields) |
| **Command palette** | Cmd+K / `:` in command mode | `:` | Escape → terminal | Search list (already implemented) |

### Region behavior rules

- **Toggle:** If sessions region is focused and Cmd+E is pressed again, return to terminal mode. Pressing a different region's key switches directly to that region.
- **Auto-open:** If a panel is closed and its toggle key is pressed, the panel opens AND the region takes focus.
- **Cursor reset:** Entering a list region places the cursor on the currently active item (e.g. the running session). If no active item, cursor goes to the first item.
- **Both paths work:** Cmd+E enters the sessions region from terminal mode AND from command mode. Bare `e` only works in command mode. Two paths, same destination.

---

## Keybinding Vocabulary

### Universal list navigation (Sessions, Tasks, Watches)

These keys work identically in every list/tree region:

| Key | Action | Notes |
|-----|--------|-------|
| `j` / `↓` | Move cursor down | Wraps to top at end |
| `k` / `↑` | Move cursor up | Wraps to bottom at start |
| `l` / `→` | Expand group / enter children | No-op on leaf items |
| `h` / `←` | Collapse group / jump to parent | On a child, moves cursor to parent group header |
| `Enter` | Activate item | Switch session, run task, toggle watch detail |
| `gg` | Jump to first item | Vim double-tap, 300ms timeout |
| `G` | Jump to last item | Shift+G |
| `/` | Focus filter input | Escape from filter returns to list navigation |
| `Escape` | Exit region → terminal mode | Always, no exceptions |

### Region-specific extensions

| Region | Key | Action |
|--------|-----|--------|
| Sessions | `r` | Rename session |
| Sessions | `x` | Close/delete session (with confirmation) |
| Tasks | `r` | Re-run task |
| Watches | `p` | Pause/resume watch |
| Watches | `d` | Delete watch (with confirmation) |

### Pane command mode keys

Active when in command mode (entered via Ctrl+Space):

| Key | Action | Equivalent modifier shortcut |
|-----|--------|------------------------------|
| `h` / `j` / `k` / `l` | Focus pane in direction | Alt+H/J/K/L |
| `H` / `J` / `K` / `L` | Move pane in direction | Ctrl+Shift+H/J/K/L |
| `+` / `-` | Resize focused pane larger/smaller along the parent split axis | Ctrl+Alt+H/J/K/L |
| `d` | Split horizontal | Cmd+D |
| `D` | Split vertical | Cmd+Shift+D |
| `x` | Close pane | Cmd+W |
| `f` | Toggle fullscreen | Cmd+Shift+F |
| `s` | Toggle stack mode | Cmd+Shift+S |
| `r` | Rename pane | — |
| `1`–`0` | Jump to session by sidebar position | Cmd+1–0 |
| `e` | Enter sessions region | Cmd+E |
| `t` | Enter tasks region | Cmd+Shift+T |
| `w` | Enter watches region | Cmd+Shift+W |
| `b` | Enter notes region | Cmd+B |
| `:` | Open command palette | Cmd+K |
| `i` / `Enter` / `Escape` | Return to terminal mode | — |

### Global shortcuts (work from any mode)

These use modifier keys and are never intercepted by region navigation:

| Key | Action | Status |
|-----|--------|--------|
| Cmd+1–0 | Jump to session by visible sidebar position | New |
| Cmd+K | Command palette | Exists |
| Cmd+E | Enter/toggle sessions region | New |
| Cmd+Shift+T | Enter/toggle tasks region | New |
| Cmd+Shift+W | Enter/toggle watches region | Exists (toggle only → now also focuses) |
| Cmd+B | Enter/toggle notes region | Exists (toggle only → now also focuses) |
| Cmd+, | Enter/toggle settings region | Exists (toggle only → now also focuses) |
| Cmd+N | New session | Exists |
| Cmd+Q | Quit | Exists |
| Cmd+W | Close pane | Exists |
| Cmd+D / Cmd+Shift+D | Split H/V | Exists |
| Cmd+Shift+F | Toggle fullscreen pane | Exists |
| Cmd+Shift+S | Toggle stack mode | Exists |
| Alt+H/J/K/L | Navigate panes | Exists |
| Ctrl+Shift+H/J/K/L | Move panes | Exists |
| Ctrl+Alt+H/J/K/L | Resize panes | Exists |
| Ctrl+Space | Toggle command mode | New |

---

## Session Jump Shortcuts (Cmd+1–0)

- `Cmd+1` targets the first visible session in the sidebar, `Cmd+2` the second, and so on through `Cmd+0` for the tenth.
- Visible order means: sessions currently shown in the sidebar after group expand/collapse state is applied. Collapsed groups hide their children from the count.
- These shortcuts work from any mode (terminal, command, or list region) and always switch to the target session + return to terminal mode.

---

## Visual Indicators

### Mode badge

The focused pane's titlebar displays a mode indicator:
- **Terminal mode:** "TERM" in muted/default styling
- **Command mode:** "CMD" in accent color (e.g. amber/yellow), with a subtle accent border on the pane

### Region highlight

When a list region is focused:
- The region's container header gets an accent border or highlight (e.g. blue)
- Only one region is highlighted at a time

### Cursor

- A `▸` marker and background highlight on the item at the cursor position
- Distinct from "active" item styling (active session = bold text or subtle background, cursor = prominent highlight)
- The cursor and active item can be different: active = which session is running, cursor = which item you're about to act on

---

## Shared Package: `@phin-tech/keynav`

A TypeScript/Svelte package providing vim-style region-modal keyboard navigation, designed for reuse across Phin Tech apps.

### Layer 1: Key Sequence Engine

Framework-agnostic TypeScript. Detects multi-key sequences (gg, leader keys) with configurable timeout.

```typescript
createKeySequence({
  sequences: { 'gg': () => jumpToFirst(), 'G': () => jumpToLast() },
  timeout: 300,
}): (event: KeyboardEvent) => boolean
```

### Layer 2: List Navigator

Manages cursor position, expand/collapse, wrapping, and activation for any list/tree.

```typescript
const nav = createListNavigator({
  items: () => flatVisibleItems,
  onActivate: (item) => switchSession(item.id),
  onExpand: (item) => expandGroup(item.id),
  onCollapse: (item) => collapseGroup(item.id),
  wrap: true,
  initialItem: () => activeSessionId,
})

// Reactive state (Svelte 5 runes)
nav.cursorIndex   // current position
nav.cursorItem    // item at cursor
nav.isActive      // whether receiving keys

// Methods
nav.handleKey(event): boolean
nav.reset()       // snap to initialItem
nav.activate()    // mark active, reset cursor
nav.deactivate()  // mark inactive
```

### Layer 3: Focus Region Manager

Svelte-aware. Coordinates active region, routes keys, handles enter/exit/toggle.

```typescript
const regionManager = createRegionManager({
  regions: {
    pane:     { kind: 'passthrough' },
    command:  { kind: 'command' },
    sessions: { kind: 'list', navigator: sessionNav },
    tasks:    { kind: 'list', navigator: taskNav },
    watches:  { kind: 'list', navigator: watchNav },
    notes:    { kind: 'passthrough' },
    settings: { kind: 'passthrough' },
  },
  defaultRegion: 'pane',
})

// Reactive state
regionManager.active         // current region name
regionManager.isCommandMode  // shorthand

// Methods
regionManager.enter('sessions')
regionManager.exit()          // always → 'pane'
regionManager.toggle('sessions')
regionManager.handleKey(event): boolean
```

### What stays in the consuming app

- Command registry and global shortcut matching
- Pane tree operations (split, close, resize, navigate)
- Terminal focus management (xterm disableStdin)
- Region-specific extension keys (r, x, p, d)
- Visual indicator components and styles

---

## Exceptions

| Surface | Behavior | Reason |
|---------|----------|--------|
| Notes region | Standard text editing keys, not vim list nav | It's a textarea, not a list |
| Settings region | Tab between form fields, not vim nav | Standard form UX |
| Command palette | Existing vim bindings via bits-ui | Already implemented, no changes needed |
| Terminal panes in terminal mode | All keys pass to xterm | Core terminal functionality |

---

## Verification

1. **Unit tests** for the `@phin-tech/keynav` package: key sequence detection, list navigator cursor behavior (wrap, expand/collapse, reset), region manager transitions
2. **Integration tests** in Roux: mode transitions (terminal → command → list → terminal), Escape always returns to terminal, toggle behavior, session jump shortcuts
3. **Manual verification**: focus a terminal, Ctrl+Space to command mode, hjkl to navigate panes, `e` to enter sessions, j/k to move cursor, Enter to switch, Escape back to terminal. Full round-trip.
