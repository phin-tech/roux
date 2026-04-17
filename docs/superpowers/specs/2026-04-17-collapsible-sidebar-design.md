# Collapsible Sidebar (VSCode-style Icon Rail)

**Date:** 2026-04-17
**Status:** Approved

## Problem

Today `settings.sidebarCollapsed` fully hides the left sidebar. That makes the session list unreachable until the user toggles it back. A middle state — a narrow icon rail — keeps session switching available while reclaiming horizontal space, matching VSCode's collapsed sidebar behavior.

## Goals

- Collapse the sidebar to a narrow rail of session dots instead of fully hiding it.
- Clicking a dot activates the session. Hovering shows the session name as a tooltip.
- Add a `<` / `>` toggle arrow in the sidebar header to swap between expanded and rail states.
- Preserve the existing `ui.toggle-sidebar` command and its keybinding.

## Non-Goals

- Keyboard navigation within the rail.
- Drag-to-reorder sessions from the rail.
- Animated transitions between modes.
- Remembering per-group collapse state across expand/collapse cycles.
- Placing action buttons (New, Watches, Settings, Notifications) or the task panel in the rail.
- A third fully-hidden state — this replaces the existing hide behavior.

## Design

### Behavior

- `settings.sidebarCollapsed: boolean` is reused. `false` = expanded, `true` = rail.
- The rail has a fixed width of **44px**. `settings.tabWidth` applies only in expanded mode.
- The divider/resize handle between sidebar and main pane is rendered only in expanded mode.
- Clicking a session dot calls `setActiveSession(session.id)`.
- Native `title` attribute on each dot provides the hover tooltip (session name).
- All sessions appear in the rail in their current grouped order. Group headers are dropped; groups are separated by a thin 1px divider. Collapsed-group state is ignored in rail mode.
- Rail scrolls vertically using the existing `app-scrollbar` class when sessions overflow.

### Components

**New — `src/lib/components/CollapsedSidebar.svelte`**
- Renders the rail container (44px fixed width, full height, matching `SessionTabs` background).
- Top: `>` toggle button that calls `updateSetting("sidebarCollapsed", false)`.
- Below: vertically stacked `SessionDot` per session, iterating `getGroupedSessions(...)` in the same order used by `SessionTabs`.
- A 1px divider between groups.

**New — `src/lib/components/SessionDot.svelte`**
- ~28px circle, first letter of the session name as content.
- Props: `session`, `active: boolean`, `onselect: () => void`.
- Active: filled background + accent ring.
- Inactive: subtle background, hover state.
- Uses project/repo color signal consistent with `SessionCard` where cheap to share. If extracting color logic is non-trivial, v1 may use a neutral tint and revisit.
- `title` attribute = session name.

**Modified — `src/lib/components/Layout.svelte`**
- Replace the current `{#if !$settings.sidebarCollapsed}` hide branch with a conditional that renders either `SessionTabs` (expanded) or `CollapsedSidebar` (rail).
- Render the drag handle only in expanded mode.
- Apply `sidebarWidth` only in expanded mode; use fixed 44px in rail mode.

**Modified — `src/lib/components/SessionTabs.svelte`**
- Add a `<` toggle button in the header row next to "Sessions" that calls `updateSetting("sidebarCollapsed", true)`.
- No other behavior changes.

**Modified — `src/lib/commands/ui.ts`**
- No change needed. Existing `ui.toggle-sidebar` flips the same boolean, which now semantically swaps between expanded and rail.

### State

- No new settings. No persistence schema change. Default remains `sidebarCollapsed: false`.

### Error Handling

- None. Pure UI state on one boolean. No I/O, no external calls.

## Testing

- **Unit — `SessionDot.svelte`:** renders initial letter, applies active styling when `active` is true, fires `onselect` on click.
- **Unit — `CollapsedSidebar.svelte`:** renders one dot per session in the order returned by `getGroupedSessions`; toggle button updates the `sidebarCollapsed` setting.
- **Manual:**
  - Toggle from header button and via `ui.toggle-sidebar` command — both round-trip cleanly.
  - Clicking a dot activates the correct session.
  - Hover shows the session name.
  - Divider/resize handle disappears in rail mode, returns in expanded mode.
  - Sidebar does not flicker or leak stale widths between modes.

## Risks & Mitigations

- **Color logic duplication:** `SessionCard` has existing color rules per project. If porting is awkward, v1 uses a neutral tint — acceptable tradeoff for a first pass.
- **Feature-flag drift:** The old "fully hidden" behavior is removed. Anyone relying on it (e.g., muscle memory) will see the rail instead. This is the intended UX change.

## Out of Scope / Future

- Keyboard focus/navigation within the rail.
- Animations on collapse/expand.
- Drag-reorder or drag-to-project from rail dots.
- Showing notification / watch failure badges on relevant dots.
