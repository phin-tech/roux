# Keymap

Roux's keyboard shortcuts are **data-driven**. Every binding — direct shortcuts, the leader chord tree, even the prefix that opens it — lives in a single KDL file you can edit. Built-in **presets** (`default`, `tmux`) ship with the app; user overrides go into `~/.config/roux/keymap.kdl`.

## Where the keymap lives

`~/.config/roux/keymap.kdl`. Created on first launch with the full default preset baked in so you can browse and edit individual bindings without copying anything.

After editing, run **Reload Keymap** from the command palette (++cmd+k++) — no restart required.

## Switching to a built-in preset

Replace the file with a single line:

```kdl
preset "tmux"
```

Then `Reload Keymap`. The preset name resolves to the bundled `tmux.kdl`, and you instantly get tmux-style bindings: ++ctrl+b++ prefix, `%` to split vertical, `"` to split horizontal, `c`/`n`/`p` for new/next/previous session, `hjkl` to navigate panes, etc.

To go back:

```kdl
preset "default"
```

## Mixing a preset with your own overrides

Anything you put after the `preset` line *overrides* that preset, key by key:

```kdl
preset "tmux"

// I want Alt+hjkl pane focus alongside tmux's Ctrl+B chords
bind "Alt+KeyH" "pane.focus-left"
bind "Alt+KeyJ" "pane.focus-down"
bind "Alt+KeyK" "pane.focus-up"
bind "Alt+KeyL" "pane.focus-right"

// And drop the tmux `d` → quit binding (I don't trust myself)
unbind "Cmd+KeyQ"
```

Composition rules:

- **`bind "X" "command.id"`** — overrides the preset's binding for `X`, or adds it if the preset didn't have one.
- **`unbind "X"`** — drops a top-level direct bind inherited from the preset.
- **`tree "name" { ... }`** — replaces the *whole* preset tree of that name. To tweak a single chord inside a preset tree, redeclare the entire tree.
- **`prefix "X" tree="..."`** — overrides the preset's prefix for that key.

## Two notations: physical and character

Keys can be specified two ways. The parser picks based on context but both are always accepted.

**Physical** — matches `KeyboardEvent.code`. Survives Mac Option's special-character output (++alt+j++ → `∆` would otherwise miss). Used for any binding with a modifier prefix.

```kdl
bind "Cmd+KeyK"      "app.command-palette"
bind "Alt+KeyH"      "pane.focus-left"
bind "Ctrl+Shift+KeyL" "pane.move-right"
bind "Alt+Digit1"    "pane.focus-index-1"
```

**Character** — matches `KeyboardEvent.key`. Natural for chord-tree binds where the logical character matters and shifted punctuation Just Works.

```kdl
tree "tmux" {
    bind "%" "pane.split-vertical"
    bind "\"" "pane.split-horizontal"
    bind "?" "app.command-palette"
}
```

Tmux-style aliases (`C-`, `M-`, `S-`) are normalized at parse time, so pasting from a `.tmux.conf` mostly works:

```kdl
bind "C-b"        // → Ctrl+KeyB
bind "M-Left"     // → Alt+ArrowLeft
bind "C-M-Right"  // → Ctrl+Alt+ArrowRight
```

## Trees, prefixes, and modes

A **tree** is a named group of chord bindings. A **prefix** binds a trigger key to a tree.

```kdl
tree "leader" {
    bind "w" { enter-tree "leader-panes" }
    bind "n" "ui.toggle-notes"
}

tree "leader-panes" {
    bind "h" "pane.focus-left"
    bind "s" "pane.split-horizontal"
}

prefix "Cmd+Semicolon" tree="leader"
```

Pressing the prefix arms the tree. Pressing a bound key inside the tree fires the action; the tree closes automatically. Pressing the prefix again exits.

### `enter-tree`: drilling deeper

`bind "w" { enter-tree "leader-panes" }` doesn't fire a command — it pushes the named tree onto the active path, keeping the HUD visible at the next level. The HUD breadcrumb shows where you are: `leader › leader-panes`.

### Sticky trees

Add a `sticky` child node and the tree stays armed after each chord. ++escape++ exits.

```kdl
tree "resize" {
    sticky
    bind "h" "pane.resize-left"
    bind "j" "pane.resize-down"
    bind "k" "pane.resize-up"
    bind "l" "pane.resize-right"
    bind "Escape" "keymap.exit-tree"
}

prefix "Cmd+KeyR" tree="resize"
```

### Passthrough trees

Add `passthrough` and unbound keys go through to the focused terminal. Useful for "locked" or "scroll" modes — the tree stays armed, but anything you don't bind reaches the shell.

```kdl
tree "locked" {
    sticky
    passthrough
    bind "Ctrl+KeyG" "keymap.exit-tree"
}
```

⚠️ Bound keys always win, even ones the terminal would normally consume. If you bind ++ctrl+c++ inside a passthrough tree, the shell won't see SIGINT while the tree is active.

## HUD modes

Each tree (and the document overall) can choose how visible the chord HUD is.

```kdl
hud "always"           // default — HUD appears immediately when a tree arms
hud "delayed 1000"     // HUD stays hidden for 1000 ms; reveals only if you pause
hud "never"            // HUD never renders (purely keyboard, no visual feedback)

tree "tmux" {
    hud "delayed 1000"  // override the document default for this tree
    // ...
}
```

Tmux users typically want delayed or never; first-time Roux users want always. The `tmux` preset ships with `delayed 1000` baked in.

## Multiple prefixes

Same tree, multiple triggers — useful when you want both old and new muscle memory live simultaneously:

```kdl
prefix "Cmd+Semicolon" tree="leader"
prefix "Ctrl+KeyB"     tree="leader"
```

Different trees on different prefixes also work; each prefix arms its own tree.

## Command IDs

Bindings target commands by id. Every action in the command palette has an id you can bind. The full list is searchable via ++cmd+k++; common ones:

| Category | Id | What it does |
|---|---|---|
| Panes | `pane.focus-left` / `-down` / `-up` / `-right` | Move focus between panes |
| Panes | `pane.focus-index-1` … `pane.focus-index-10` | Focus the Nth pane (DFS order) |
| Panes | `pane.focus-next` | Focus the next pane in tree-traversal order |
| Panes | `pane.split-horizontal` / `pane.split-vertical` | Split the focused pane |
| Panes | `pane.close` | Close the focused pane |
| Panes | `pane.toggle-fullscreen` / `pane.toggle-stack` | Layout toggles |
| Panes | `pane.rename` | Open the inline rename input |
| Panes | `pane.resize-left` / `-down` / `-up` / `-right` | Resize splits |
| Panes | `pane.move-left` / `-down` / `-up` / `-right` | Reorder panes |
| Sessions | `session.new` / `session.close` / `session.reconnect` | Lifecycle |
| Sessions | `session.next` / `session.prev` | Cycle sessions |
| Sessions | `session.focus-index-1` … `session.focus-index-10` | Jump to Nth session |
| Sessions | `session.open-in-editor` / `session.rename` | Sidebar actions |
| App | `app.command-palette` | Open the palette |
| App | `app.quit` | Quit Roux |
| App | `app.settings` | Open settings |
| UI | `ui.toggle-notes` / `ui.toggle-watches` / `ui.toggle-notifications` | Sidebars |
| Keymap | `keymap.reload` | Re-read `~/.config/roux/keymap.kdl` |
| Keymap | `keymap.exit-tree` | Exit the active tree (for sticky/passthrough trees) |

## Built-in presets

### `default`

Mirrors the hardcoded shortcuts from earlier Roux versions — ++cmd+;++ leader, ++cmd+k++ palette, ++alt+hjkl++ pane focus, ++alt+1++ … ++alt+0++ pane index, ++cmd+1++ … ++cmd+0++ session index, etc. This is what you get on first launch.

### `tmux`

Tmux-style: ++ctrl+b++ prefix; `%` / `"` for splits; `hjkl` / `o` for pane focus; `c` / `n` / `p` for session new/next/previous; `x` to close, `z` for fullscreen, `?` for the palette, `d` for quit. Delayed HUD so chords feel fast.

### `zellij` *(planned)*

Schema supports it (sticky and passthrough trees), but no preset KDL ships in v1. If you want it sooner, the schema is documented above and you can author it yourself in the meantime.

## Error handling

If your `keymap.kdl` has a parse error, Roux keeps the previous keymap loaded and fires a notification with the line/column of the failure. You can keep using the previous binds while you fix the file.

If a binding references a command id that doesn't exist (typo, removed command), it's silently dropped at load time and shows up as a warning in the notification panel.

## Examples

### Vim-style mode prefix
Trigger a sticky resize mode with ++ctrl+r++ that stays armed until ++escape++:

```kdl
tree "resize" {
    sticky
    bind "h" "pane.resize-left"
    bind "j" "pane.resize-down"
    bind "k" "pane.resize-up"
    bind "l" "pane.resize-right"
    bind "Escape" "keymap.exit-tree"
}

prefix "Ctrl+KeyR" tree="resize"
```

### Quick session switcher with no prefix
Bind ++ctrl+tab++ / ++ctrl+shift+tab++ to cycle sessions directly:

```kdl
preset "default"
bind "Ctrl+Tab"       "session.next"
bind "Ctrl+Shift+Tab" "session.prev"
```

### Hide the HUD entirely (silent power-user mode)
You know your bindings; no help needed:

```kdl
preset "default"
hud "never"
```
