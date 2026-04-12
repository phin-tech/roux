# Layouts

A **layout** is a file-based session template that defines a pane tree with spawn profiles. Layouts are inputs, not state — they describe what to create, and user edits to panes after creation don't write back to the layout file.

## Where layout files live

- **Built-in layouts** ship with the binary (two are included).
- **User layouts** go in `~/.config/roux/layouts/*.kdl`. The directory is created automatically on first launch.

User layouts override built-in layouts on id collision (the id is the filename stem, lowercased).

## Using layouts

1. Open the new-session dialog (++cmd+n++).
2. Pick a layout from the **Layout** dropdown. The description appears below.
3. When a layout is selected, the spawn-profile picker is hidden — the layout defines its own panes.
4. Click **Create Session**. Roux creates the session, then splits and populates panes according to the layout.

Sessions created from a layout show a small "layout: ..." badge in the session sidebar.

## KDL schema

Layouts are written in [KDL](https://kdl.dev). The top-level node is `layout`:

```kdl
layout {
    name "My Layout"
    description "Optional one-liner shown in the picker"

    // pane tree goes here
}
```

### Leaf panes

Reference a registered spawn profile by id:

```kdl
pane profile="claude"
```

Or define an inline profile:

```kdl
pane name="my agent" {
    display_name "My Agent"
    kind "shell"
    setup_command "export FOO=bar"
    startup_command "my-agent start"
    startup_behavior "run"
    env {
        KEY "value"
    }
}
```

Inline profile fields:

| Field | Required | Description |
|---|---|---|
| `display_name` | no | Shown in the pane title bar |
| `kind` | no | `shell` (default), `claude`, or `codex` |
| `setup_command` | no | Typed into the shell before the startup command |
| `startup_command` | no | The main command to launch |
| `startup_behavior` | no | `run` (default), `auto_run`, or `type_only` |
| `env` | no | Child node with `KEY "value"` entries for environment variables |

### Split containers

Wrap child panes in a split container:

```kdl
pane split_direction="horizontal" {
    pane profile="claude"      size=60
    pane profile="plain-shell" size=40
}
```

- `split_direction` — `horizontal` (left/right) or `vertical` (top/bottom).
- `size` — proportional weight (0–100). Sibling sizes are normalized to fractions, so `60`/`40` gives a 60/40 split. If omitted, siblings share space equally.

Splits can nest arbitrarily.

## Built-in layouts

### Claude + shell

Claude on the left (60%), plain shell on the right (40%).

```kdl
layout {
    name "Claude + shell"
    description "Claude on the left, plain shell on the right"

    pane split_direction="horizontal" {
        pane profile="claude"      size=60
        pane profile="plain-shell" size=40
    }
}
```

### Agent comparison

Claude and Codex side-by-side (50/50).

```kdl
layout {
    name "Agent comparison"
    description "Claude and Codex side-by-side (nono wrapping not applied in v1)"

    pane split_direction="horizontal" {
        pane profile="claude" name="claude" size=50
        pane profile="codex"  name="codex"  size=50
    }
}
```

## Full example

A three-pane layout with Claude on the left, a shell and a test runner stacked vertically on the right:

```kdl
layout {
    name "Dev setup"
    description "Claude + shell + tests"

    pane split_direction="horizontal" {
        pane profile="claude" size=60

        pane split_direction="vertical" size=40 {
            pane profile="plain-shell" size=50
            pane name="tests" size=50 {
                kind "shell"
                startup_command "npm run test:watch"
                startup_behavior "run"
            }
        }
    }
}
```

Save this as `~/.config/roux/layouts/dev_setup.kdl` and it will appear in the layout dropdown next time you open the new-session dialog.

## See also

- [Panes](panes.md) — how splits and stacking work once the layout is applied
- [Sessions](sessions.md) — session lifecycle and persistence

## Known v1 limitations

- Claude panes inside layouts are not nono-wrapped.
- `cwd` attribute is reserved but not honored yet.
- Layouts only apply at session creation — no "apply to existing session."
- No "save current layout as file" — edit the `.kdl` files directly.
- No inline layout editor in the UI.
- The layout badge on the session card is cosmetic and does not persist across restarts.
