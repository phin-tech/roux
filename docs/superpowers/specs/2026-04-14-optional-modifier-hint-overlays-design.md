# Optional modifier-key hint overlays

## Problem

Holding Option reveals a pane-number overlay; holding Command reveals a session-hint overlay. Both are always-on. Users who find either overlay noisy (or who trigger it accidentally while typing chords) have no way to silence it. The underlying chord shortcuts (`Option+1…9`, `Option+H/J/K/L`, `Cmd+K`, `Cmd+;`, `Cmd+digit`, etc.) are unrelated to the visual reveal.

## Goals

- Let users independently disable each modifier's hint overlay.
- Expose toggles via a new **Keyboard & Controls** section in Settings.
- Do **not** change which chord shortcuts are available — the toggles gate overlay reveal only.

## Non-goals

- Rebinding shortcuts.
- A shortcut-reference list (may come later).
- Disabling overlays from anywhere other than Settings.

## Design

### Settings schema (`crates/roux-core/src/models/settings.rs`)

Add two fields to `RouxSettings`:

```rust
#[serde(default)]
pub show_pane_hints_on_option: bool,          // default: false
#[serde(default = "default_true")]
pub show_session_hints_on_command: bool,       // default: true
```

Update `Default for RouxSettings` to match. `#[serde(default)]` / `default_true` ensures old settings.json files load cleanly (Option defaults off for everyone; Command defaults on, preserving current behavior).

Regenerate `src/lib/bindings.ts` via the existing specta pipeline.

### Frontend store (`src/lib/stores/settings.ts`)

The settings store already mirrors `RouxSettings`. No shape work beyond picking up the two new fields.

### Wiring (`src/App.svelte`)

`handleKeyDown` currently calls `armSessionHints()` on Meta/Control down (line 207) and `armPaneHints()` on Alt down (line 212). Guard each with the corresponding setting:

```ts
if (settings.showSessionHintsOnCommand && isPrimaryModifierKey(e)) armSessionHints();
if (settings.showPaneHintsOnOption && e.key === "Alt")             armPaneHints();
```

The `handleKeyUp` side (lines 364, 367) that calls `hideSessionHints()` / `hidePaneHints()` is left unconditional so a toggle flipped off mid-hold still cleans up any visible overlay.

Chord handlers further down in `handleKeyDown` (the `Alt+1..9` pane-focus block at line 321, `Cmd+K`, `Cmd+;`, etc.) are **not** touched.

### Settings UI (`src/lib/components/SettingsPanel.svelte`)

Add a new section titled **Keyboard & Controls** with two checkbox rows, matching the existing checkbox-row pattern used for `notifications_enabled` / `confirm_on_quit`:

- **Show pane hint overlay when holding Option** — "Reveals pane numbers while ⌥ is held. Option+digit shortcuts still work either way."
- **Show session hint overlay when holding Command** — "Reveals session shortcuts while ⌘ is held. Command chord shortcuts still work either way."

Place the section near the existing keyboard-adjacent prefs; if none exist, insert above the "Advanced" / logging section.

## Persistence & migration

No migration needed — `#[serde(default)]` handles missing keys. Existing users:

- Option overlay: silently turns **off** on next launch (behavior change).
- Command overlay: unchanged (still **on**).

The Option default is intentional per the feature request; no release note required beyond the CHANGELOG line.

## Testing

- Rust unit test in `crates/roux-core/src/models/settings.rs`: deserializing a settings.json without the new fields yields `show_pane_hints_on_option == false` and `show_session_hints_on_command == true`.
- Svelte component test for `SettingsPanel.svelte`: toggling each checkbox dispatches a settings update with the expected field.
- Manual verification:
  1. Fresh install → hold Option → no overlay.
  2. Press Option+1 → still jumps to pane 1.
  3. Hold Cmd → session overlay appears.
  4. Toggle "Show session hint overlay" off → hold Cmd → no overlay; Cmd+K still opens palette.

## Files touched

- `crates/roux-core/src/models/settings.rs` — new fields + defaults + test.
- `src/lib/bindings.ts` — regenerated.
- `src/App.svelte` — guard the two `arm*Hints()` calls.
- `src/lib/components/SettingsPanel.svelte` — new section, two checkboxes.
- `CHANGELOG.md` — one-line entry.
