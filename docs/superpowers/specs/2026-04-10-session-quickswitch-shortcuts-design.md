# Session Quick-Switch Shortcuts

Date: 2026-04-10

## Summary

Add `Cmd+1` through `Cmd+9` and `Cmd+0` to switch directly to the Nth session in
the sidebar, where N follows the sidebar's visual top-to-bottom order. Holding
`Cmd` for ~200ms displays a centered overlay on each session card showing the
digit you'd press to switch to it, similar in spirit to the macOS application
switcher.

## Goals

- Let the user jump to any of the first ten sessions with a single chord.
- Make the mapping discoverable: a brief hold of `Cmd` reveals which digit maps
  to which session without requiring documentation.
- Keep the visible mapping in sync with the sidebar so "what I see" equals
  "what I press".

## Non-goals

- No shortcut coverage for sessions beyond slot 10 in this iteration.
- No reordering UI for slots. The slot is a positional consequence of the
  existing sidebar order; it is not a user-editable assignment.
- No cross-app global shortcut. These bindings only fire while the Roux window
  is focused.

## Slot assignment rules

- Slots are assigned by walking the sidebar in the same order it renders:
  iterate the groups (in the order produced by the current `groupBy` setting,
  repo or project), then iterate the sessions within each group in their
  existing in-group order.
- Numbering starts at 1 and runs through 10. Slot N is reachable via `Cmd+N`
  for N in 1..9, and slot 10 is reachable via `Cmd+0`.
- Sessions in slots 11+ receive no shortcut and no overlay badge.
- Collapsed groups are still counted. Collapsing a group must not renumber the
  shortcuts of sessions that remain visible. Rationale: numbering that changes
  based on collapse state would make the shortcut unstable.
- When fewer than N sessions exist, `Cmd+N` is a no-op (the key is still
  consumed with `preventDefault` so nothing else reacts to it).

## Architecture

### Extract the grouping logic

The grouped-sessions logic currently lives inside `SessionTabs.svelte` as
`$derived` state (`groupedByRepo` and `groupedByProject`). This logic is not
reachable from `App.svelte`, where the keyboard handler lives, so it must be
lifted into a pure module.

- **New module**: `src/lib/sessions/order.ts`
  - `getGroupedSessions(sessions, projects, groupBy)` — returns the same group
    structure `SessionTabs.svelte` builds today: an array of
    `{ name, key, sessions, latest }`. Pure, no store reads.
  - `getVisualSessionOrder(sessions, projects, groupBy)` — returns the flat
    `Session[]` in sidebar top-to-bottom order (groups in order, sessions in
    each group in order). Built by flattening `getGroupedSessions`.
- **Modify**: `src/lib/components/SessionTabs.svelte`
  - Replace the inline `$derived.by` blocks with calls to `getGroupedSessions`.
  - No behavioral change; this is a deduplication step so that the overlay and
    the sidebar cannot drift.

### Keyboard handler

All of the work happens in `src/App.svelte`'s existing `handleKeyDown` (and a
new `handleKeyUp`), which already runs in the capture phase and owns the other
app-level shortcuts (`cmd+q`, `cmd+k`, palette state, etc.).

- In `handleKeyDown`, ahead of the generic `registry.getByShortcut` lookup, add
  a branch that matches `metaKey` with digit keys and no other modifiers:

  ```ts
  if (
    e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey &&
    /^[0-9]$/.test(e.key)
  ) {
    const slot = e.key === "0" ? 10 : parseInt(e.key, 10);
    const order = getVisualSessionOrder(
      get(sessionState).sessions,
      get(projects),
      get(settings).groupBy,
    );
    const target = order[slot - 1];
    e.preventDefault();
    if (target) setActiveSession(target.id);
    return;
  }
  ```

- The `preventDefault` fires whether or not a target exists, so a missing slot
  does not fall through to the command registry or the browser default.

### Overlay (session hints)

- **New store**: `src/lib/stores/ui.ts` exports
  `showSessionHints: Writable<boolean>`. No general UI store exists today, so
  this is a new file; additional UI-wide flags can live here later.
- **Trigger**:
  - On `keydown` where `e.key === "Meta"` and the store is currently `false`,
    start a 200ms `setTimeout`. When it fires, set `showSessionHints = true`.
  - On `keyup` where `e.key === "Meta"`, clear the pending timeout and set
    `showSessionHints = false`.
  - On `window.blur`, clear the pending timeout and set the store to `false`.
    This prevents a "stuck hint" when the user Cmd-Tabs away.
  - Do not reset the overlay when the user actually fires `Cmd+N`; they may
    immediately press `Cmd+M` next and expect the overlay to still be up.

- **Badge placement on the card**:
  - `SessionTabs.svelte` already renders a `SessionCard` for each session. As
    it walks the grouped structure it maintains a running counter and passes
    `slotNumber?: number` as a prop to each card. Cards in slot 11+ receive
    `undefined`.
  - `SessionCard.svelte` renders an overlay element when
    `$showSessionHints && slotNumber != null`:
    - Absolute positioning, filling the card.
    - A semi-transparent dimming layer covering the card content (matches the
      macOS app switcher feel).
    - A large centered digit (`slotNumber === 10 ? "0" : String(slotNumber)`).
    - `pointer-events: none` so clicks on the card still work normally.
    - Fades in/out over ~120ms (`transition: opacity`).

## Edge cases

- **More than 10 sessions**: slots 11+ get no overlay badge and no shortcut.
  This is intentional — the binding set is fixed at the 10 digit keys.
- **Command palette is open**: the existing handler already early-returns when
  `showPalette` is true. The digit branch is placed *after* the palette guard
  so the same behavior applies.
- **Quick shortcut without hold**: the 200ms delay means a user who presses
  `Cmd+K`, `Cmd+N`, `Cmd+W`, or `Cmd+1..0` and releases immediately never sees
  the overlay at all.
- **`Cmd` tapped on its own**: the overlay appears after 200ms and disappears
  on keyup. Cheap to render, no harm done.
- **Window blurred while Cmd held**: the blur handler hides the overlay and
  cancels any pending show, so the overlay cannot get stuck on.
- **`groupBy` change or project edit while overlay is up**: both the overlay
  slot assignment and the keyboard handler read the order from the same helper
  on demand, so they stay consistent.

## Testing

- **Unit tests** (`src/lib/sessions/__tests__/order.test.ts`):
  - `getGroupedSessions` matches the behavior previously inlined in
    `SessionTabs.svelte`, including "Untagged" pinned to the bottom for the
    project grouping.
  - `getVisualSessionOrder` flattens groups in the expected order.
  - Empty sessions returns an empty array.
- **Store test** for the session hints store using vitest fake timers:
  - Pressing Meta arms the overlay; releasing before 200ms leaves it hidden.
  - Holding past 200ms shows it; releasing hides it.
  - `window.blur` cancels pending show and hides an active overlay.
- **Component test** for `SessionCard.svelte`:
  - Badge only renders when the store is true and `slotNumber` is set.
  - Slot 10 renders as "0".
- **Manual verification** in the running app:
  - `Cmd+1..9` and `Cmd+0` switch to the expected card given the current
    grouping.
  - Switching `groupBy` re-orders the slots and the overlay follows.
  - Collapsing a group does not renumber remaining cards.
  - Creating a new session shifts numbering as expected (new card joins at the
    appropriate position per the existing grouping rules).

## Files touched

- `src/lib/sessions/order.ts` — new.
- `src/lib/sessions/__tests__/order.test.ts` — new.
- `src/lib/stores/ui.ts` — new, exports `showSessionHints` store.
- `src/lib/components/SessionTabs.svelte` — use extracted helper, pass
  `slotNumber` prop to each rendered card.
- `src/lib/components/SessionCard.svelte` — render overlay badge and dim layer.
- `src/App.svelte` — keydown branch for `Cmd+digit`, Meta keydown/keyup
  handlers that drive `showSessionHints` with a 200ms delay, blur handler.
- `docs/keyboard-shortcuts.md` — document the new shortcuts.
