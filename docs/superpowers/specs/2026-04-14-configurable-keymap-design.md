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
bind "Cmd+KeyK"      "palette.toggle"
bind "Cmd+KeyQ"      "app.quit"
bind "Alt+KeyH"      "pane.focus-left"
bind "Alt+Digit1"    "pane.focus-index-1"

// Remove a binding inherited from the preset.
unbind "Alt+Digit0"

// A named chord tree. One-shot (tmux-style) by default.
tree "leader" {
  bind "w" { enter-tree "leader-panes" }   // nested drill-down
  bind "n" "ui.toggle-notes"
  bind "Space" "palette.toggle"
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
- Explicitly-qualified codes (`KeyH`, `Digit1`) always resolve via `e.code`
  regardless of position.

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
- `unbind "X"` removes the preset's binding for `X`.
- `tree "<name>" { ... }` with a name already defined by the preset *replaces*
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
  options: { getPresetKdl: (name: string) => string | null; knownCommandIds: Set<string> },
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
  binds: Map<string /* key, as keyed by the resolver's normalization */, Action>;
}

type Action =
  | { kind: "command"; commandId: string }
  | { kind: "enterTree"; tree: string };

type HudMode = { kind: "always" } | { kind: "delayed"; ms: number } | { kind: "never" };
```

No Svelte or DOM imports; testable as a pure function.

### Store — `src/lib/keymap/store.ts`

Svelte writable holding both the parsed keymap and runtime state:

```ts
interface KeymapState {
  keymap: ParsedKeymap;
  activeTree: string | null;        // name of tree currently armed
  treePath: string[];               // for HUD display, the chain of trees entered from root (e.g. ["leader", "leader-panes"])
  hudVisibleSince: number | null;   // for delayed HUD timing
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
  | { kind: "none" }                                     // no binding; fall through
  | { kind: "enterTree"; tree: string }                  // prefix matched, or `enter-tree` action fired
  | { kind: "chord"; action: Action; keepTreeOpen: boolean }
  | { kind: "passthrough" }                              // passthrough tree, unbound key
  | { kind: "exit" };                                    // Escape in tree
```

Multi-step chords within a single tree are not supported. A two-key sequence
is modeled as `prefix` → tree with a bind whose action is
`enter-tree "<nested>"`, which promotes the resolution to `enterTree`.

Precedence, in order:

1. If a tree is active:
   a. If the event matches a `prefix` trigger, resolve to `enterTree`
      (prefix-within-prefix cancels and rearms; tmux behavior).
   b. Else if the event matches a bind in the active tree, resolve to
      `chord` with `keepTreeOpen = activeTree.sticky`. If the action is
      `enterTree`, resolve to `enterTree` instead.
   c. Else if the event is Escape, resolve to `exit`.
   d. Else if the active tree is passthrough, resolve to `passthrough`.
   e. Else resolve to `none` (unbound key in non-passthrough tree is dropped;
      the tree stays armed). Users who dislike this can bind Escape themselves.
2. If no tree is active:
   a. If the event matches a `prefix` trigger, resolve to `enterTree`.
   b. Else if the event matches a `directBind`, resolve to `chord` with
      `keepTreeOpen = false`.
   c. Else resolve to `none`.

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
  if (isMacPlatform() ? e.key === "Meta" : e.key === "Control") armSessionHints();
  if (e.key === "Alt") armPaneHints();

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
      return;  // don't preventDefault; terminal receives the key
    case "exit":
      e.preventDefault();
      keymapStore.exitTree();
      return;
  }
}

function dispatchAction(action: Action) {
  if (action.kind === "command") registry.execute(action.commandId);
  else keymapStore.enterTree(action.tree);
}
```

No hardcoded key branches remain. The command-surface-open state is not a
special case at this layer — commands like `palette.toggle` already handle
their own open/close via the registry.

### xterm veto

`src/lib/panes/terminalRegistry.ts` grows a helper:

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

Every xterm instance (both `Terminal.svelte` and `ShellTerminal.svelte`)
calls `installKeymapVeto` at creation. This guarantees keys consumed by the
keymap layer are never delivered to the PTY even if the window-level handler
loses the race with xterm's internal keydown path.

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

Single coordinated change:

1. Add new `keymap` module (parser, store, resolver).
2. Add Rust commands and embedded preset files.
3. Replace `src/App.svelte`'s hardcoded key branches with the new dispatch.
4. Rename `LeaderHud.svelte` → `KeymapHud.svelte`, update props and consumers.
5. Delete `src/lib/commands/leader.ts` and its tests (content moves into the
   `default` preset KDL).
6. Remove the unused `shortcut` field from `Command` in
   `src/lib/commands/registry.ts` and all call sites.
7. Update settings UI with the new Keymap section.

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
bind "Cmd+KeyK"      "palette.toggle"
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
  bind "Space" "palette.toggle"
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

Ships with intentional gaps pointing at commands that don't yet exist. The
parser warns at load time; firing a bind to an unknown command is a no-op.
Adding the underlying commands is tracked as follow-up work.

```kdl
hud "delayed 1000"

tree "tmux" {
  bind "c" "session.new"
  bind "n" "session.next"              // TODO: command not yet implemented
  bind "p" "session.prev"              // TODO: command not yet implemented
  bind "x" "pane.close"
  bind "%" "pane.split-vertical"
  bind "\"" "pane.split-horizontal"
  bind "o" "pane.focus-next"           // TODO: command not yet implemented
  bind "h" "pane.focus-left"
  bind "j" "pane.focus-down"
  bind "k" "pane.focus-up"
  bind "l" "pane.focus-right"
  bind "z" "pane.toggle-fullscreen"
  bind "d" "app.quit"
  bind "?" "palette.toggle"
  bind "[" { enter-tree "tmux-copy" }
}

tree "tmux-copy" sticky=true passthrough=true {
  bind "q" "keymap.exit-tree"
  bind "Escape" "keymap.exit-tree"
}

prefix "Ctrl+KeyB" tree="tmux"
```

### Commands to add for tmux parity

Tracked separately from this spec; enumerated here so the implementation plan
surfaces them:

- `session.next` — focus the next session in the sidebar order.
- `session.prev` — focus the previous session.
- `pane.focus-next` — focus the next pane in tree-traversal order within the
  active session.

These are small additions to `src/lib/commands/sessions.ts` and
`src/lib/commands/panes.ts` but are outside v1 scope for the keymap itself.

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
- **Escape in non-sticky tree**: always exits, even if unbound in the tree.
- **Escape in sticky tree without an Escape binding**: exits (safety net).
- **Escape in passthrough tree without an Escape binding**: stays active.
  Passthrough trees are expected to bind their own exit key; this is
  documented in the schema comments.
- **Reload while a tree is active**: the store exits the tree before
  swapping in the new keymap. No stale references survive.
- **Unknown command at fire time** (preset references a command that was
  removed between app launches): `registry.execute` is a no-op for unknown
  IDs; the warning already fired at load time.

## Testing

### Parser (`src/lib/keymap/__tests__/parse.test.ts`)

Pure Vitest, no DOM.

- Round-trip: parse each built-in preset KDL; assert no errors and expected
  structural shape (trees, prefixes, bind counts).
- Preset merging: user KDL with `preset "default"` plus overrides → merged
  `ParsedKeymap` equals the preset with overrides applied.
- `unbind` removes entries from both top-level direct binds and tree binds.
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
  `none` or `passthrough` per unbound-key rules.
- Prefix-within-prefix: tree active, new prefix → `enterTree` (not append).
- `Cmd` platform mapping: Mac `event.metaKey` matches; non-Mac
  `event.ctrlKey` matches.

### Store (`src/lib/keymap/__tests__/store.test.ts`)

- Load from valid KDL string → state populated, warnings exposed.
- Load from invalid KDL → state unchanged, notification fired (via mocked
  notification store).
- Reload while tree active → tree exits, new keymap takes effect.
- `enterTree` / `exitTree` mutate `activeTree` and `treePath` correctly.

### Behavioral regression (`src/lib/keymap/__tests__/default-preset-parity.test.ts`)

Port the existing leader tests (`src/lib/commands/__tests__/leader.test.ts`)
into a suite that loads the `default` preset and asserts every previously
working chord still resolves. This is the gate that prevents silent
regressions during migration.

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
