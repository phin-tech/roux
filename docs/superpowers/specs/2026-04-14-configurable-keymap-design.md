# Configurable Keymap — Design

## Goal

Replace Roux's hardcoded key handling (leader chords, direct-mod shortcuts,
palette/quit bindings) with a single data-driven keymap loaded from
`~/.config/roux/keymap.kdl`. Users can swap between built-in presets
(`default`, `tmux`, eventually `zellij`) or author their own bindings. The
design generalizes the existing leader (prefix → chord tree) to support
multiple prefixes, multiple named trees, sticky (Zellij-style) modes, and
passthrough (Zellij "locked") modes — all as data.

## Non-goals

- In-UI chord-tree editor. File is the editor in v1.
- File watcher / hot reload. v1 uses a command-triggered reload.
- Reading native `tmux.conf`. A future `roux import-tmux-conf` may translate a
  subset, but the authoritative format is Roux's own KDL schema.
- Per-pane or per-session keymaps. Keymap is global.
- Migrating `RouxSettings` to KDL. Tracked separately.
- Shipping a complete Zellij preset in v1. Schema supports it; KDL deferred.

## Command registry additions

Some shortcuts that are today hardcoded in `App.svelte` bypass the
registry entirely. Turning them into data-driven bindings requires them to
exist as registered commands. Added in the same change as the keymap
module:

- `app.quit` — currently `Cmd+Q` hardcoded in `App.svelte`. Wraps the
  existing `handleQuitRequested()` helper.
- `pane.focus-index-1` … `pane.focus-index-9`, `pane.focus-index-10` —
  currently the `Alt+Digit*` block in `App.svelte`. One command per index;
  each wraps the existing pane-index focus logic.

The registry already has `app.command-palette`, `app.leader-mode`,
`app.settings`, `pane.focus-left/right/up/down`, `pane.split-horizontal`,
`pane.split-vertical`, `pane.rename`, `pane.close`,
`pane.toggle-fullscreen`, `pane.toggle-stack`, `session.new`,
`session.close`, `session.reconnect`, `session.open-in-editor`,
`ui.toggle-notes`, `ui.toggle-notifications`, `ui.toggle-watches`. These
are referenced by the default preset as-is.

Three additional commands are added to match tmux-preset parity:

- `session.next` — focus the next session in the sidebar ordering.
- `session.prev` — focus the previous session.
- `pane.focus-next` — focus the next pane within the active session's pane
  tree (traversal order matches the existing `Alt+Digit` index sequence).

Two are owned by the keymap module itself:

- `keymap.exit-tree` — exits the active tree. Available only when a tree is
  active.
- `keymap.reload` — re-reads `~/.config/roux/keymap.kdl`.

**`app.command-palette` semantics.** This command already exists but
currently only opens the palette via its own logic. For the keymap to
bind `Cmd+K` to it, `execute()` must be wired to the palette-toggle
behavior (open if closed, close if open). Verify at implementation time
and fix if the existing `execute` just opens.

## UX

### File

On first launch after the update, Roux writes the `default` preset to
`~/.config/roux/keymap.kdl` if no file exists. Users edit the file directly.

### Settings

A new "Keymap" section in the Settings panel contains:

- **Preset** dropdown (`default`, `tmux`, `custom`). Selecting a built-in
  preset rewrites (or inserts) the first non-comment statement in
  `keymap.kdl` to `preset "<name>"` and reloads. All other user content
  (`bind`, `unbind`, `tree`, `prefix` declarations, comments) is preserved in
  place. Selecting `custom` is informational only — it is shown when the
  file has no `preset` line and the dropdown is non-interactive in that
  state.
- **Open keymap.kdl** button — opens the file in the user's configured editor
  via the existing `cmd_open_in_editor` command.
- **Reload** button — equivalent to the `keymap.reload` palette command.

### Command palette

A new palette entry `Reload keymap` (id: `keymap.reload`) re-reads and reparses
the file. No default key binding in v1 — users who want one bind it in their
keymap.

### HUD

The existing leader HUD becomes a general keymap HUD. It renders when a tree
is active and obeys the tree's `hud` attribute:

- `always` — appears immediately on tree entry.
- `delayed <ms>` — appears after the specified delay if no chord has fired.
- `never` — invisible. Tree still works; feedback is tmux-style silent.

Sticky trees additionally show a persistent mode indicator ("PANE MODE",
"RESIZE MODE", etc.) even after a chord fires, until the tree is exited.
Passthrough trees use a visually distinct indicator so users understand their
keys are going to the terminal except where bound.

### Error surfaces

Parse errors and validation warnings surface through the existing notification
system. See [Error Handling](#error-handling).

## Config schema

File: `~/.config/roux/keymap.kdl`. Parsed in TypeScript. Full grammar by
example follows; every element is optional except at least one `tree` or one
top-level `bind`.

```kdl
// Optional. Inherits everything from a named built-in preset as the starting
// point. Omit to start empty.
preset "default"

// Global HUD default. Each tree may override.
hud "always"   // "always" | "delayed <ms>" | "never"

// Top-level direct binds. Fire without any prefix. Replace the currently
// hardcoded Cmd+K, Cmd+Q, Opt+hjkl, Opt+1..9 branches.
bind "Cmd+KeyK"      "app.command-palette"
bind "Cmd+KeyQ"      "app.quit"
bind "Alt+KeyH"      "pane.focus-left"
bind "Alt+Digit1"    "pane.focus-index-1"

// Remove a binding inherited from the preset.
unbind "Alt+Digit0"

// A named chord tree. One-shot (tmux-style) by default.
tree "leader" {
  bind "w" { enter-tree "leader-panes" }   // nested drill-down
  bind "n" "ui.toggle-notes"
  bind "Space" "app.command-palette"
}

tree "leader-panes" {
  bind "h" "pane.focus-left"
  bind "s" "pane.split-horizontal"
  bind "v" "pane.split-vertical"
}

// Sticky (Zellij-style) tree. Stays active after firing a chord. Escape exits.
tree "resize" sticky=true hud="always" {
  bind "h" "pane.resize-left"
  bind "l" "pane.resize-right"
  bind "Escape" "keymap.exit-tree"
}

// Passthrough ("locked") tree. Unbound keys are sent to the focused terminal.
tree "locked" sticky=true passthrough=true {
  bind "Ctrl+KeyG" "keymap.exit-tree"
}

// Prefixes bind a trigger key to a tree. Multiple prefixes per tree are
// allowed; multiple trees on distinct prefixes are allowed.
prefix "Cmd+Semicolon" tree="leader"
prefix "Ctrl+KeyB"     tree="leader"
prefix "Ctrl+KeyR"     tree="resize"
```

### Key notation

Two forms; both always accepted.

- **Physical**: `<mods>+<KeyboardEvent.code>` — e.g. `Alt+KeyH`, `Cmd+Digit1`,
  `Ctrl+KeyB`. Resolved against `e.code`. Required wherever keyboard-layout
  or dead-key-modifier issues matter (all macOS `Alt+letter` bindings).
- **Character**: bare character or named key — e.g. `"h"`, `"%"`, `"Escape"`,
  `"Tab"`, `"ArrowLeft"`. Resolved against `e.key`. Natural for chord trees
  where the logical character matters (`%`, `"`, `?`).

Defaulting rules:

- Top-level `bind` with a modifier prefix (`Cmd+`, `Ctrl+`, `Alt+`, `Shift+`)
  defaults to **physical** notation. Resolve via `e.code`.
- Inside `tree` blocks, a bare `bind "h"` defaults to **character** notation.
  Resolve via `e.key`.
- Inside `tree` blocks, a bind with a modifier prefix (`bind "Ctrl+KeyX"`,
  `bind "Alt+KeyH"`) defaults to **physical** notation — same rule as
  top-level. The modifier presence is the disambiguator.
- Explicitly-qualified codes (`KeyH`, `Digit1`) always resolve via `e.code`
  regardless of position.

**Shifted punctuation.** Character bindings match `e.key` after the
browser has applied Shift. `bind "%"` matches a keydown whose `e.key === "%"`
(Shift+5 on US layouts; different physical key on AZERTY). This is the
right semantics for chord-tree binds where tmux users write `%` and expect
"the percent key" regardless of what produces it. If a user needs to bind
"the physical 5 key regardless of shift," they use `Digit5` or
`Shift+Digit5`.

**Modifier matching.** Character binds require exact modifier match:
`bind "h"` matches `e.key === "h"` with no modifiers pressed other than
Shift implicit in the character itself. `bind "Shift+h"` matches the shifted
character `"H"` — in practice users write `bind "H"` for that case, which
is equivalent. Physical binds (`Alt+KeyH`) require the listed modifiers
exactly; extra modifiers do not match.

### Modifier tokens and aliases

Canonical: `Cmd` (Meta/Win), `Ctrl`, `Alt`, `Shift`.

Tmux-style aliases normalized at parse time:

- `C-` → `Ctrl+`
- `M-` → `Alt+`
- `S-` → `Shift+`

So `"C-b"` parses to `Ctrl+KeyB`, `"M-Left"` to `Alt+ArrowLeft`. This lets
users paste familiar tmux bindings without translation.

### Platform semantics for `Cmd`

`Cmd` is a platform-dispatched modifier mirroring the existing
`hasPrimaryModifier()` helper: on macOS it binds to Meta; elsewhere it binds
to Ctrl. Keymap authors write `Cmd+KeyK` once and it works on both.

### Command IDs

Any ID in the frontend command registry (`src/lib/commands/registry.ts`) is a
valid bind target. The existing registration path in
`src/lib/commands/index.ts` remains authoritative. Two new IDs are added for
keymap control:

- `keymap.exit-tree` — exits the active tree. Sticky trees must bind this (or
  Escape, which exits by default when unbound in non-sticky trees).
- `keymap.reload` — re-reads the keymap file.

A third value, `enter-tree "<tree-name>"`, is a bindable **action** (not a
command ID) that promotes a chord to a new tree. Used for nested drill-down
(`bind "w" { enter-tree "leader-panes" }`).

### Preset composition

`preset "<name>"` at the top of the file loads all of that preset's `hud`,
`bind`, `tree`, and `prefix` declarations as the starting state. Subsequent
statements in the user's file add to or replace preset entries:

- `bind "X" "..."` with a key `X` already bound by the preset replaces it.
- `unbind "X"` at the top level removes the preset's **direct bind** for
  `X`. It does not reach into trees — trees are only modified by redeclaring
  them.
- `tree "<name>" { ... }` with a name already defined by the preset _replaces_
  the whole tree. There is no tree-level merge in v1 — users who want to tweak
  two keys of a preset tree redeclare it.
- `prefix "..." tree="..."` with a key already bound as a prefix replaces it.
  The preset's entry is dropped.

Rationale for "replace-whole-tree": tree-level merge semantics (add this bind,
remove that one, keep the rest) add parser complexity and ambiguity around
ordering; most practical edits either override a single chord or replace an
entire mode. If users demand per-bind tree patching later, it's a pure
additive schema change.

## Architecture

Three layers under a new `src/lib/keymap/` package.

### Parser — `src/lib/keymap/parse.ts`

Pure function. Signature:

```ts
function parseKeymap(
  kdlText: string,
  options: {
    getPresetKdl: (name: string) => string | null;
    knownCommandIds: Set<string>;
  },
): ParseResult;

type ParseResult =
  | { kind: "ok"; keymap: ParsedKeymap; warnings: KeymapWarning[] }
  | { kind: "error"; errors: KeymapParseError[] };
```

Responsibilities:

1. Tokenize and parse the KDL.
2. Resolve `preset "<name>"` by fetching the preset's KDL text via
   `getPresetKdl` and recursively parsing it as the base state. Presets may
   not themselves declare `preset`; attempting to chain is a parse error.
3. Apply `unbind`, bind replacement, tree replacement, prefix replacement on
   top of the preset-provided state.
4. Normalize tmux aliases (`C-`, `M-`, `S-`).
5. Validate command IDs against `knownCommandIds`; unknown IDs become
   warnings and the binding is dropped from the runtime keymap.
6. Validate tree references from `prefix` and from `enter-tree` actions;
   unknown references become warnings and the binding is dropped.
7. Emit warnings for: prefix-collides-with-direct-bind (direct bind dropped),
   duplicate prefix triggers (second one dropped), and the command-ID / tree
   validation above.

Output `ParsedKeymap` is a fully resolved, runtime-ready shape with no
remaining preset references:

```ts
interface ParsedKeymap {
  hudDefault: HudMode;
  directBinds: Map<NormalizedKey, Action>;
  trees: Map<string, Tree>;
  prefixes: Map<NormalizedKey, string /* tree name */>;
}

interface Tree {
  name: string;
  sticky: boolean;
  passthrough: boolean;
  hud: HudMode;
  binds: Map<
    string /* key, as keyed by the resolver's normalization */,
    Action
  >;
}

type Action =
  | { kind: "command"; commandId: string }
  | { kind: "enterTree"; tree: string };

type HudMode =
  | { kind: "always" }
  | { kind: "delayed"; ms: number }
  | { kind: "never" };
```

No Svelte or DOM imports; testable as a pure function.

### Store — `src/lib/keymap/store.ts`

Svelte writable holding both the parsed keymap and runtime state:

```ts
interface KeymapState {
  keymap: ParsedKeymap;
  treePath: string[]; // chain of trees entered from root, e.g. ["leader", "leader-panes"]. Empty when no tree is active. Tail element is the currently-armed tree.
  hudVisibleSince: number | null; // for delayed HUD timing
}
```

Exports:

- `keymapStore` — the writable.
- `keymapState` — a derived read-only store that resolvers and components read.
- `loadKeymap()` — reads KDL from disk (via `get_keymap`), parses, validates,
  publishes. On parse error, state is unchanged and a notification fires.
- `enterTree(name)`, `exitTree()` — state mutators invoked from the keydown
  handler. `enterTree` appends to `treePath`; `exitTree` clears it.

### Resolver — `src/lib/keymap/resolve.ts`

Pure function:

```ts
function resolveKey(
  event: KeyboardEvent,
  state: KeymapState,
  isCommandAvailable: (id: string) => boolean,
): Resolution;

type Resolution =
  | { kind: "none" } // no binding; fall through
  | { kind: "enterTree"; tree: string } // prefix matched, or `enter-tree` action fired
  | { kind: "chord"; action: Action; keepTreeOpen: boolean }
  | { kind: "passthrough" } // passthrough tree, unbound key
  | { kind: "exit" }; // Escape in tree
```

Multi-step chords within a single tree are not supported. A two-key sequence
is modeled as `prefix` → tree with a bind whose action is
`enter-tree "<nested>"`, which promotes the resolution to `enterTree`.

Precedence, in order:

1. If a tree is active:
   a. If the event matches a bind in the active tree, resolve to
   `chord` with `keepTreeOpen = tree.sticky` (where `tree` is the tail
   element of `treePath`). If the action is
   `enterTree`, resolve to `enterTree` instead. **Tree binds match before
   prefixes** — this lets users bind `"C-b c"` inside the tmux tree even
   though `C-b` is the tree's own prefix; the second `C-b` fires the
   bind rather than rearming. If the user wants "rearm on double-prefix"
   they simply leave that key unbound in the tree.
   b. Else if the event matches a `prefix` trigger, resolve to `enterTree`
   (prefix-within-prefix cancels and rearms; tmux behavior).
   c. Else if the event is Escape, resolve to `exit`. Escape exits
   unconditionally — in both sticky and non-sticky, passthrough or not.
   The only exception is when the active tree explicitly binds Escape to
   something, in which case 1a runs first and the bind fires.
   d. Else if the active tree is passthrough, resolve to `passthrough`.
   e. Else resolve to `none` (unbound key in non-passthrough tree is dropped;
   the tree stays armed).
2. If no tree is active:
   a. If the event matches a `prefix` trigger, resolve to `enterTree`.
   b. Else if the event matches a `directBind`, resolve to `chord` with
   `keepTreeOpen = false`.
   c. Else resolve to `none`.

**Consequence for passthrough trees and terminal control chars.** A
passthrough tree passes unbound keys through to the focused pane. A bound
key always wins, even when that key is something the terminal would
normally consume (`Ctrl+C` → SIGINT, `Ctrl+D` → EOF). Users authoring a
"locked" / passthrough tree should not bind keys they want the terminal to
receive. The schema doesn't need a separate "send-through" action — just
don't bind it.

Command availability is checked via the `isCommandAvailable` callback passed
to `resolveKey`. Bindings whose command is currently unavailable resolve to
`none` so the key falls through rather than firing a greyed-out action. This
preserves the existing `available()` semantics in the command registry.

## Runtime wiring

### Keydown path (`App.svelte`)

The current `handleKeyDown` block in `src/App.svelte:202-330` is replaced:

```ts
function handleKeyDown(e: KeyboardEvent) {
  // Hint-overlay arming preserved; it's UI, not a binding.
  if (isMacPlatform() ? e.key === "Meta" : e.key === "Control")
    armSessionHints();
  if (e.key === "Alt") armPaneHints();

  // Command surfaces (palette open, leader HUD with prompt visible) own
  // keyboard focus. The keymap must not fire bindings while the user is
  // typing in a search field, entering a rename, etc. The only exception is
  // entering the leader itself — but that is handled by the prefix match
  // below triggering on the global keydown before the surface "owns" the
  // key, so no special case is needed for leader entry.
  const surface = get(commandSurface);
  if (surface.open && surface.mode !== "leader") return;
  if (
    surface.open &&
    surface.mode === "leader" &&
    surface.leaderPromptCommandId
  ) {
    // Leader prompt is active (onInput flow). Let the palette's Enter /
    // Escape handlers run; the global keymap stays out.
    return;
  }

  // Preserve the existing Escape-in-terminal focus fix from the old handler:
  // WebKit will otherwise blur xterm's hidden textarea and lose focus.
  if (e.key === "Escape") {
    const focused = get(focusedPaneId);
    if (focused && get(paneInstances).get(focused)?.terminal) {
      e.preventDefault();
    }
  }

  const state = get(keymapState);
  const resolution = resolveKey(e, state, (id) => isCommandAvailable(id));
  switch (resolution.kind) {
    case "none":
      return;
    case "enterTree":
      e.preventDefault();
      keymapStore.enterTree(resolution.tree);
      return;
    case "chord":
      e.preventDefault();
      dispatchAction(resolution.action);
      if (!resolution.keepTreeOpen) keymapStore.exitTree();
      return;
    case "passthrough":
      return; // don't preventDefault; terminal receives the key
    case "exit":
      e.preventDefault();
      keymapStore.exitTree();
      return;
  }
}

function dispatchAction(action: Action): void {
  if (action.kind === "enterTree") {
    keymapStore.enterTree(action.tree);
    return;
  }
  const cmd = registry.get(action.commandId);
  if (!cmd) return; // unknown: warning already fired at load
  if (cmd.execute) {
    cmd.execute();
    return;
  }
  if (cmd.onInput) {
    // Open the leader-prompt UI for this command, preserving the current
    // App.svelte `openLeaderPrompt` flow (see src/App.svelte:171-196).
    openLeaderPrompt(
      action.commandId,
      getLeaderPromptInitialValue(action.commandId),
    );
    return;
  }
  // Commands with only getItems drill into a command-palette surface; this
  // mirrors the existing behaviour for `app.leader-mode` and similar.
  if (cmd.getItems) {
    openCommandPalette(action.commandId);
    return;
  }
}
```

The command-surface-open gate preserves current behavior: typing in the
palette search, rename input, or watch filter never accidentally fires a
keymap binding. `dispatchAction` handles all three execution flows
(`execute`, `onInput` → leader prompt, `getItems` → drill-in palette)
rather than only calling `registry.execute`.

### xterm veto (defense-in-depth)

The window-level keydown listener is registered with `capture: true`
(`App.svelte:414`), so it runs **before** xterm.js sees the event. Calling
`e.preventDefault()` in the global handler is sufficient to block the key
from reaching the PTY in all normal cases.

The xterm veto is an additional layer for robustness (e.g. if xterm ever
attaches its own capture-phase listener in a future upgrade, or if some
browser quirk changes dispatch order). `src/lib/panes/terminals.ts` — the
existing module that owns xterm lifecycle — grows a helper:

```ts
export function installKeymapVeto(term: Terminal): void {
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    const state = get(keymapState);
    const r = resolveKey(e, state, (id) => isCommandAvailable(id));
    return r.kind === "none" || r.kind === "passthrough";
  });
}
```

`installKeymapVeto` is called at xterm-instance creation inside
`src/lib/panes/terminals.ts`, so both Claude and shell panes (both of which
go through `PaneShell.svelte`) pick it up via the shared terminal-creation
path.

### HUD

The existing `src/lib/components/LeaderHud.svelte` is renamed to
`KeymapHud.svelte` and generalized:

- Props: `treeName`, `sticky`, `passthrough`, `hudMode`, `sequence`, `hints`.
- Renders:
  - Nothing when `hudMode` is `never`.
  - Nothing before `delayed.ms` elapses when `hudMode` is `delayed`.
  - Immediately when `hudMode` is `always`.
- Sticky trees additionally render a persistent mode badge in the top-right
  corner (outside the hint panel) styled distinctly for passthrough trees.

The hints list is derived from the active tree's binds with
command-availability filtering, mirroring the current
`getVisibleLeaderHints` logic. Unavailable bindings are hidden unless the
tree opts out (no schema support for opt-out in v1 — always filtered).

### Palette shortcut display

The existing palette (`src/lib/components/CommandPalette.svelte:270`)
renders `cmd.shortcut` next to each command. Removing the `shortcut` field
from `Command` requires a replacement path: the keymap store exposes
`shortcutFor(commandId): string | null` that walks the loaded keymap and
returns the user-visible shortcut label for a command. Precedence:

1. First matching direct bind (`bind "X" "id"`) → render the key label
   (`Cmd+K`, `Alt+H`).
2. Else first `prefix` → `tree` bind that targets this command ID
   (`Cmd+;  w h` for a two-step leader chord).
3. Else `null` (no shortcut registered).

`CommandPalette.svelte:270` switches from `cmd.shortcut` to
`shortcutFor(cmd.id)`, unchanged rendering logic. When the user reloads
their keymap, the store publishes a new identity; the palette reactively
re-renders.

### File I/O

Two new Tauri commands in `src-tauri/src/keymap.rs`:

```rust
// Reads ~/.config/roux/keymap.kdl. Creates it from the `default` preset if
// missing, so callers never have to handle "file not found."
#[tauri::command]
fn get_keymap() -> Result<String, String>;

// Atomically writes the given text to the keymap file. Used by the Settings
// UI's preset dropdown when it rewrites the `preset` line.
#[tauri::command]
fn set_keymap(contents: String) -> Result<(), String>;

// Returns the embedded KDL text for a built-in preset.
#[tauri::command]
fn get_builtin_keymap_preset(name: String) -> Result<String, String>;
```

Presets are embedded via `include_str!` from
`src-tauri/src/keymap/presets/default.kdl` and `tmux.kdl`. Parsing stays in
TypeScript; Rust only handles I/O, path resolution, and first-run bootstrap.

### Migration

Single coordinated change, ordered so each step leaves the app in a
working state:

1. **Register new commands** in `src/lib/commands/` for what's currently
   hardcoded: `app.command-palette`, `app.quit`, `app.leader-mode`,
   `pane.focus-index-1` … `pane.focus-index-10`. Add `session.next`,
   `session.prev`, `pane.focus-next` for tmux parity. At this step the new
   commands exist in the registry but nothing fires them — the hardcoded
   branches still do their job.
2. **Add the keymap module** (parser, store, resolver) in
   `src/lib/keymap/`. Pure; no wiring yet. Tests pass.
3. **Add Rust commands and embedded preset files** in
   `src-tauri/src/keymap/` and `src-tauri/src/keymap/presets/`. At this
   step `get_keymap` / `set_keymap` / `get_builtin_keymap_preset` work but
   no TS reads them.
4. **Replace `src/App.svelte`'s hardcoded key handling** with the dispatch
   described above. Load the keymap at startup. Keep the Escape
   terminal-focus fix inline (not delegated to the keymap). This is the
   cutover step — default preset must produce identical behavior to the
   old handler.
5. **Rename `LeaderHud.svelte` → `KeymapHud.svelte`**, update props and
   consumers. Delete `src/lib/commands/leader.ts` and its tests (content
   moves into the default preset KDL; behavioral coverage moves to
   `default-preset-parity.test.ts`).
6. **Replace `cmd.shortcut` in `CommandPalette.svelte:270`** with
   `keymapStore.shortcutFor(cmd.id)`. Remove the `shortcut` field from
   `Command` in `registry.ts` and all registration call sites in the same
   commit.
7. **Update the Settings panel** with the new Keymap section (preset
   dropdown, "Open keymap.kdl", "Reload").
8. **Install the xterm veto** in `src/lib/panes/terminals.ts` for
   defense-in-depth.

No backwards-compat shim. The keymap file is created on first launch; users
who had customized shortcuts via any other means (none currently exist) would
need to re-author in the new format.

## Built-in presets

Presets ship in-tree under `src-tauri/src/keymap/presets/` and are embedded
at compile time. Exposed to the frontend via
`get_builtin_keymap_preset(name)`.

### `default`

Mirrors current behavior exactly. Relevant structure:

```kdl
hud "always"

// Replace current hardcoded App.svelte branches.
bind "Cmd+KeyK"      "app.command-palette"
bind "Cmd+KeyQ"      "app.quit"
bind "Alt+KeyH"      "pane.focus-left"
bind "Alt+KeyJ"      "pane.focus-down"
bind "Alt+KeyK"      "pane.focus-up"
bind "Alt+KeyL"      "pane.focus-right"
bind "Alt+Digit1"    "pane.focus-index-1"
bind "Alt+Digit2"    "pane.focus-index-2"
bind "Alt+Digit3"    "pane.focus-index-3"
bind "Alt+Digit4"    "pane.focus-index-4"
bind "Alt+Digit5"    "pane.focus-index-5"
bind "Alt+Digit6"    "pane.focus-index-6"
bind "Alt+Digit7"    "pane.focus-index-7"
bind "Alt+Digit8"    "pane.focus-index-8"
bind "Alt+Digit9"    "pane.focus-index-9"
bind "Alt+Digit0"    "pane.focus-index-10"

tree "leader" {
  bind "w" { enter-tree "leader-panes" }
  bind "b" { enter-tree "leader-sessions" }
  bind "n" "ui.toggle-notes"
  bind "i" "ui.toggle-notifications"
  bind "t" "ui.toggle-watches"
  bind "," "app.settings"
  bind "Space" "app.command-palette"
}

tree "leader-panes" {
  bind "h" "pane.focus-left"
  bind "j" "pane.focus-down"
  bind "k" "pane.focus-up"
  bind "l" "pane.focus-right"
  bind "s" "pane.split-horizontal"
  bind "v" "pane.split-vertical"
  bind "r" "pane.rename"
  bind "d" "pane.close"
  bind "f" "pane.toggle-fullscreen"
  bind "t" "pane.toggle-stack"
}

tree "leader-sessions" {
  bind "n" "session.new"
  bind "d" "session.close"
  bind "r" "session.reconnect"
  bind "e" "session.open-in-editor"
}

prefix "Cmd+Semicolon" tree="leader"
```

### `tmux`

All referenced commands are registered in the registry as part of this
project — no shipped warnings. See "Command registry additions" above for
the three new commands (`session.next`, `session.prev`, `pane.focus-next`).

```kdl
hud "delayed 1000"

tree "tmux" {
  bind "c" "session.new"
  bind "n" "session.next"
  bind "p" "session.prev"
  bind "x" "pane.close"
  bind "%" "pane.split-vertical"
  bind "\"" "pane.split-horizontal"
  bind "o" "pane.focus-next"
  bind "h" "pane.focus-left"
  bind "j" "pane.focus-down"
  bind "k" "pane.focus-up"
  bind "l" "pane.focus-right"
  bind "z" "pane.toggle-fullscreen"
  bind "d" "app.quit"
  bind "?" "app.command-palette"
  bind "[" { enter-tree "tmux-copy" }
}

tree "tmux-copy" sticky=true passthrough=true {
  bind "q" "keymap.exit-tree"
  bind "Escape" "keymap.exit-tree"
}

prefix "Ctrl+KeyB" tree="tmux"
```

Built-in presets are required to be warning-clean at load time; a CI check
(`npm run test` covers it via `default-preset-parity.test.ts` and
`tmux-preset-parity.test.ts`) asserts no warnings when loading an
unmodified built-in preset. This prevents regressions where a refactor
renames a command ID without updating the presets.

### `zellij`

Deferred. Schema supports it (sticky and passthrough trees); no KDL shipped
in v1.

## Error handling

### Parse errors

Malformed KDL or structural errors (chained `preset`, unknown tree attribute,
malformed key notation) return `{ kind: "error" }` from the parser. The store
rejects the update and retains the previously-loaded keymap. A notification
fires:

- Level: `error`
- Title: `Keymap parse error`
- Body: `<filename>:<line>: <message>`
- Actions: `Open keymap` (invokes `cmd_open_in_editor` on the keymap path),
  `Dismiss`.
- Dedup key: `keymap-parse-error` so repeated failed reloads do not spam.

On first-launch bootstrap, if the embedded `default` preset itself fails to
parse, Roux logs a fatal error and surfaces a blocking dialog — that would be
a programming bug, not a user error.

### Validation warnings

Unknown command IDs, unknown preset or tree references, prefix-direct-bind
collisions, and duplicate prefix triggers do not fail the parse. They
accumulate into a `warnings` list on the `ParsedKeymap`. One aggregate
notification fires at load time:

- Level: `warning`
- Title: `Keymap loaded with N warnings`
- Body: first three warnings inline; full list available via `Show details`.
- Actions: `Open keymap`, `Dismiss`.
- Dedup key: `keymap-load-warnings`.

### Runtime edge cases

- **Prefix-within-prefix**: pressing a prefix key while a tree is already
  active cancels the current tree and arms the new one. Matches tmux.
  Exception: if the active tree explicitly binds that key to an action,
  the bind wins (per resolver precedence 1a).
- **Prefix trigger also bound inside a tree**: the tree's bind wins while
  the tree is active; outside the tree, the prefix fires. Document so users
  who bind `C-b c` inside the tmux tree know it will not rearm.
- **Escape in any tree without an Escape binding**: exits. Applies to
  sticky and passthrough trees too — Escape is a universal safety net.
  Users who want Escape to pass through bind it explicitly (e.g. to
  `keymap.exit-tree` in a passthrough tree, or to a custom action).
- **Reload while a tree is active**: the store exits the tree before
  swapping in the new keymap. No stale references survive.
- **Unknown command at fire time** (preset references a command that was
  removed between app launches): `dispatchAction` no-ops for unknown IDs;
  the warning already fired at load time. Shipped built-in presets are
  warning-clean (CI enforced).
- **Ctrl+C / SIGINT in a passthrough tree**: bound keys always win. If a
  user authors a passthrough tree that binds `Ctrl+KeyC`, they will not be
  able to send SIGINT while the tree is active. This is intentional — don't
  bind keys you want the terminal to receive.

## Testing

### Parser (`src/lib/keymap/__tests__/parse.test.ts`)

Pure Vitest, no DOM.

- Round-trip: parse each built-in preset KDL; assert no errors and expected
  structural shape (trees, prefixes, bind counts).
- Preset merging: user KDL with `preset "default"` plus overrides → merged
  `ParsedKeymap` equals the preset with overrides applied.
- `unbind "X"` removes the matching top-level direct bind only; trees are
  untouched. Redeclaring a tree replaces the whole tree.
- Tmux aliases normalize: `"C-b"` → `Ctrl+KeyB`, `"M-Left"` → `Alt+ArrowLeft`,
  `"S-Tab"` → `Shift+Tab`.
- Notation defaults:
  - Top-level `bind "Alt+h"` → physical `Alt+KeyH`.
  - Inside `tree`, `bind "h"` → character `h`.
  - Explicit `KeyH` always physical regardless of context.
- Warnings emitted for unknown command IDs, unknown preset names, unknown
  tree references, prefix-direct-bind collisions, duplicate prefix triggers.
- Parse errors: malformed KDL, chained presets, unknown tree attributes →
  `{ kind: "error", errors: [...] }` without throwing.

### Resolver (`src/lib/keymap/__tests__/resolve.test.ts`)

Pure, synthetic `KeyboardEvent` fixtures.

- Direct bind: `Alt+KeyH` keydown with no active tree → `chord` resolution.
- Prefix match: `Cmd+Semicolon` with no active tree → `enterTree`.
- Chord inside tree: tree armed, keydown `h` → chord with
  `keepTreeOpen: false` for non-sticky tree.
- Sticky tree: same chord → `keepTreeOpen: true`.
- Passthrough tree: unbound key → `passthrough`; bound key → `chord`.
- Nested `enter-tree` action: tree-armed keydown bound to `enter-tree "x"` →
  `enterTree` resolution.
- Command availability: bind targets an unavailable command → `none`.
- Escape semantics: non-sticky → `exit`; sticky with Escape bound → `chord`;
  sticky without Escape bound → `exit`; passthrough without Escape bound →
  `exit` (Escape is universal safety net).
- Prefix-within-prefix: tree active, new prefix (unbound in tree) →
  `enterTree`. Tree active, new prefix (bound in tree) → the tree's bind
  wins.
- Tree bind on a modifier combo the terminal would consume
  (e.g. `Ctrl+KeyC` in a passthrough tree) → `chord`, overriding
  passthrough.
- `Cmd` platform mapping: Mac `event.metaKey` matches; non-Mac
  `event.ctrlKey` matches.
- Shifted punctuation: `bind "%"` matches `e.key === "%"` regardless of
  physical key.

### Store (`src/lib/keymap/__tests__/store.test.ts`)

- Load from valid KDL string → state populated, warnings exposed.
- Load from invalid KDL → state unchanged, notification fired (via mocked
  notification store).
- Reload while tree active → tree exits, new keymap takes effect.
- `enterTree(name)` appends to `treePath`. `exitTree()` clears `treePath`.
  Nested `enter-tree` actions push; reload and prefix-rearm reset.

### Behavioral regression

- `src/lib/keymap/__tests__/default-preset-parity.test.ts` — loads the
  `default` preset and asserts every previously working chord still
  resolves. Ports the existing leader tests
  (`src/lib/commands/__tests__/leader.test.ts`). This is the gate that
  prevents silent regressions during migration.
- `src/lib/keymap/__tests__/builtin-presets-warning-clean.test.ts` —
  loads each built-in preset unmodified and asserts the `warnings` list is
  empty. Prevents regressions where a rename breaks a preset reference.

### Rust (`src-tauri/src/keymap/tests.rs`)

- `get_keymap_path` resolves correctly on macOS, Linux, Windows.
- First-run bootstrap writes the default preset when the file is missing.
- `get_builtin_keymap_preset("default")` returns the embedded content;
  unknown names return a clear error.

### Manual verification

Not easily automated; checked before declaring the feature done.

- Ctrl+B in a focused terminal pane does **not** emit `\x02` to the shell
  when the tmux preset is loaded — verify by typing `cat` and pressing the
  prefix.
- Opt+j still produces `∆` in a shell when no binding exists for
  `Alt+KeyJ`.
- `hud "delayed 1000"`: HUD stays hidden for fast chords (<1s), appears for
  held prefixes.
- Sticky tree badge persists across action fires; Escape dismisses it.
- Passthrough tree: typing in an active passthrough tree produces characters
  in the focused terminal; bound exit key still fires.
- Palette `> Reload keymap` picks up a just-saved file without restart.
- Multi-prefix: user keymap with both `Cmd+Semicolon` and `Ctrl+KeyB`
  arming distinct trees — both work, neither interferes.

## Out of scope (v1)

Collected for clarity; to be picked up as follow-up work if demand warrants:

- Complete Zellij preset.
- File watcher for hot reload.
- In-UI chord-tree editor.
- `roux import-tmux-conf` translator.
- Per-pane / per-session / per-project keymaps.
- Tree-level merge semantics (bind-level patching inside an existing tree).
- Opt-out of command-availability filtering in the HUD.
- Key sequences longer than two levels of nesting (unlimited via
  `enter-tree` chains already, but not explicitly tested beyond two).
