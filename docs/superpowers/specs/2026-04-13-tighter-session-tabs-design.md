# Tighter Session Sidebar Tabs

## Problem

Each session card in the sidebar crams redundant information into two rows. With grouped headers, the repo/project is shown up to three times (group header, folder path on row 2, project tag). Row 2 is a kitchen sink of branch + folder + project + cost + watch dots. The name is often the worktree folder, which duplicates what row 2 also shows.

## Goals

- Remove duplication between the group header, row 1, and row 2.
- Make the card scannable at a glance: status, identity, one or two pieces of metadata.
- Preserve the information elsewhere (tooltip, status bar) rather than deleting it.

## Non-Goals

- Redesigning group headers or the task panel.
- Changing the sidebar's outer chrome (header, footer buttons).
- Re-surfacing watch outcomes (those move to the status bar in separate work).

## Design

### Layout

```
┌───┬────────────────────────────────────┐
│ ● │ feature/better-tabs     [3]     × │  ← row 1
│   │ ⎇ worktree   roux           $2.41 │  ← row 2
└───┴────────────────────────────────────┘
 ↑
 left gutter: persistent status dot
```

- **Left gutter (20px)**: status dot, always visible, colored by effective session status. Replaces the in-row dot and the conditional vertical rail.
- **Row 1**: name, unread badge, reconnect button (when disconnected), close.
- **Row 2**: optional worktree marker, optional project tag, cost (right-aligned). Row 2 is hidden entirely when all three are absent.
- **Tooltip on the card**: `<branch> · <full worktree path>`.

### Name resolution

Precedence (first match wins):

1. User-renamed override (explicit rename from the UI or command palette).
2. Branch name, if `isGitRepo`.
3. Worktree folder name (last path segment).

The override is sticky: once a user renames, the override persists regardless of branch changes.

**Implementation note** — `Session.name` today is always populated at creation and is also the rename target. To distinguish "user override" from "auto-default", add a `nameOverride: string | null` field on `Session` (backend). Rename sets it; a new session leaves it null. The frontend computes the display name via the precedence above, reading `nameOverride` when set.

Alternative considered: frontend-only heuristic (compare `name` to the would-be auto-default). Rejected — brittle across branch renames and backend migrations.

### Status behavior

The gutter dot is **solid** for every status except `attention`, which **pulses**. `attention` is the one case the user needs to notice from across the screen (agent awaiting prompt); `thinking` and `generating` are expected background states and no longer animate.

Color map stays as today:

- idle: green · thinking: amber (solid) · generating: blue (solid) · error: red · disconnected: gray · attention: amber (pulsing)

The current `railClasses` vertical bar is removed. The active-tab indicator becomes the row background only (existing `bg-bg-active`).

### Row 2 contents

Shown conditionally:

- `⎇ worktree` marker: only when `session.isWorktree` is true. Distinguishes worktree sessions from regular clones.
- Project tag: only when `projectId` is set **and** grouping is not already by project (grouping makes the tag redundant with the group header).
- Cost: only when `session.cost != null`. Right-aligned.

If none apply, row 2 is not rendered — card compacts to one row plus gutter.

### Removed

- In-row status dot + ping animation.
- Conditional left vertical rail (`railClasses`).
- Branch text on row 2 (branch is now the name).
- Worktree folder path on row 2 (now in tooltip).
- Watch outcome dots (moving to status bar in separate work).
- Pulse animation for `thinking` / `generating`.

### Kept

- Unread notification badge.
- Reconnect button when `status === "disconnected"`.
- Close button.
- Slot-hint overlay (Cmd+1..9,0 shortcut affordance).
- `watch-flash` background animation (ambient signal, not a persistent dot).
- Context menu (unchanged).
- Inline rename (double-click the name).

## Data-model changes

Backend (`src-tauri`):

- Add `name_override: Option<String>` to the persisted session struct and TS binding `Session.nameOverride`.
- Migration: existing sessions get `name_override = Some(existing_name)` only if the stored name differs from what the new derivation would produce (branch for git, folder for non-git). For a fresh migration pass, simpler and safer: set `name_override = Some(existing_name)` for every session, preserving today's labels exactly. Users can clear the override via a "Reset name" context-menu action (out of scope for v1 — they can just rename to the branch manually if they want the new behavior).
- Rename command writes `name_override`, not `name`. `name` becomes an internal fallback only.

Frontend:

- `SessionCard.svelte`: swap layout to gutter + body, implement name precedence, conditional row 2.
- `SessionTabs.svelte`: pass `groupBy` to `SessionCard` so it can hide the project tag when already grouped by project.
- Tooltip switches from `session.worktreePath` to `${session.branch} · ${session.worktreePath}` (when git).

## Testing

- Unit: name precedence (override > branch > folder) across `isGitRepo` true/false and override null/set.
- Unit: row 2 visibility across the combinations of `isWorktree`, `projectId`, `cost`, and `groupBy`.
- Visual: side-by-side the new card against today's to confirm the reduction.
- Migration: existing persisted sessions render identically after the `name_override` backfill.

## Open Questions

- Should the worktree marker show the branch's relationship to the base (e.g., "worktree · ahead 3")? Out of scope for this pass — current scope is _reduce_, not add.
- Cache-age indicator per agent: explored during brainstorming, deferred — Claude Code doesn't expose cache-age over the hooks interface today. Estimate via last-activity timestamp would be a proxy, not ground truth.
