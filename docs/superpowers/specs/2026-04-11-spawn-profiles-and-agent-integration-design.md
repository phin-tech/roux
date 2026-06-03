# Spawn Profiles And Agent Integration

## Summary

Roux supports multiple concurrent AI agents per session by treating _every_ terminal pane as a shell pane and separating three concepts that earlier designs conflated into a dedicated "AI pane type":

1. **Pane kind** — the pane-tree primitive (`shell`, `markdown`, `command`). Unchanged, except that the legacy `claude` pane type is removed.
2. **Spawn profile** — optional persisted metadata describing _how_ a shell pane was launched. A reusable, named record of "spawn a shell, inject these env vars, optionally run these commands in it". Built-in profiles from provider modules cover Claude and Codex; user profiles let the same primitive handle dev servers, test watchers, REPL environments, remote SSH sessions, and anything else worth one-clicking.
3. **Observed agent state** — runtime-only state populated by incoming hook / OSC / `roux notify` events. Source of truth for session-card status, notifications, and provider-specific UI (Claude Allow/Deny, resume picker). Lights up on any shell pane where an agent is observed running, regardless of whether Roux spawned it via a profile or the user launched it by hand.

Correlation stays deterministic because every shell PTY gets `ROUX_SESSION_ID` and `ROUX_PANE_ID` injected into its env at spawn — unconditional, not profile-dependent. Any agent the user launches inherits them. Hook bridges route events by those ids first, so Roux knows exactly which pane a given event belongs to without cwd heuristics.

This spec supersedes `docs/superpowers/specs/2026-04-10-multi-ai-panes-per-session-design.md` and extends `docs/plans/2026-04-10-codex-cli-support-via-services-commands.md` (the in-flight parent Codex plan). The parent plan's install / normalization / notification work survives as-is; the parent plan's provider-aware PTY spawn path and `Session.provider` field disappear.

## Scope

**In scope**

- A session holds any number of shell panes. Each shell pane may optionally carry a `spawnProfileRef` describing how it was launched.
- `SpawnProfile` as a first-class persisted primitive: built-in profiles contributed by provider modules, user profiles in `RouxSettings.spawnProfiles`, inline profiles from the "Custom…" picker option.
- User profile commands (`setupCommand`, `startupCommand`) are arbitrary shell, trusted at the user's own config level.
- Every shell pane's PTY env is unconditionally populated with `ROUX_SESSION_ID` and `ROUX_PANE_ID`.
- Runtime `agentState` lights up on any shell pane from hook events, independent of whether a profile is attached.
- Session card status is a derived aggregate over each session's shell panes' `agentState` values.
- Notifications fire per-pane on `agentState.status` transitions.
- The `<sessionId>-main` pane invariant is removed. Any pane can be closed; sessions can hold zero panes briefly.
- `Session.status` is removed from the persisted session model; the frontend computes aggregate status from pane-level `agentState`.
- `status_watcher.rs` routes events by `ROUX_PANE_ID` first, with legacy-install fallbacks restricted to notification-only updates.
- Claude-specific UI (Allow/Deny, resume picker) is gated on observed `agentState.provider === "claude"`, not on pane type.
- The project-profile trust model is defined in this spec and the relevant schema fields (`RouxSettings.trustedWorkspaces`, `SpawnProfile.source = "project"`) are reserved now, even though project-profile loading is deferred to a later pass.

**Out of scope**

- Permission-request parity for Codex (deferred in the parent plan).
- Detach-and-reattach semantics for agents. Agents live in the shell PTY; killing the shell kills the agent.
- Project-level profile loading (`.roux/profiles.json`). The schema field, settings field, and trust model are reserved but the loader is not built.
- A GUI profile editor. User profiles are edited as JSON in settings.
- Profile inheritance, composition, conditional logic, or templating. Profiles are flat records.
- Automatic migration of existing persisted pane state from the old `claude` pane type. No backward compatibility required; old pane state is dropped on upgrade.
- "Adopt an unmanaged shell as a spawn profile" affordance.
- Persistent `agentState` across restarts. Observed state is runtime only.

## Architecture

### The three layers

**Pane kind** is the pane-tree primitive. `PaneType = "shell" | "markdown" | "command"` — no `"ai"`, no `"claude"`. Every terminal pane becomes a shell pane. Split, stack, drag, restore identity, layout invariants — all unchanged.

**Spawn profile** is optional metadata attached to a shell pane at creation time. It describes how Roux launched the shell: a name, optional setup and startup commands, optional env additions, optional cwd override, an optional `provider` hint for UX, and a `source` field distinguishing built-in / user / inline / project origin. Profiles are looked up at new-pane time from a registry populated at app start.

**Observed agent state** is runtime-only. Any shell pane can have an `agentState` populated by incoming hook events, OSC sequences, or `roux notify` calls. The pane's UI reads from `agentState` — session-card status dot, pane header label, provider-specific affordances. The pane is agnostic about whether Roux spawned the agent: hook events get routed to the right pane via `ROUX_PANE_ID` and all downstream logic works the same way regardless of launch path.

### Why this over dedicated AI panes

Earlier spec revisions proposed `PaneType = "ai"` with a baked-in `provider` field. Two problems forced the pivot:

1. **Wrong abstraction level.** Roux is fundamentally a terminal multiplexer. Agents are things that run in terminals. Making "AI" a pane type threads provider branching through pane creation, restore, reconnect, status UI, setup, and failure handling — cost that grows with each new agent and with each new AI-adjacent feature.
2. **Provider-specific spawn logic everywhere.** With a shell-only model, `pty.rs` stays generic, the Tauri command surface stays generic, and provider-specific behavior narrows to exactly three places: hook install (`providers/{claude,codex}.rs`), event normalization (`services/status_integrations.rs`), and provider-specific UI components gated on observed agent state.

The cost of this design is one new primitive (`SpawnProfile`) and one new runtime store (`AgentState`). In exchange, any agent that speaks the hook protocol works without pane-type code, and non-AI use cases (dev servers, test watchers, REPLs) get the same one-click UX for free.

## Data Model

### Frontend

**`SpawnProfile`** (new, `src/lib/panes/profiles.ts`):

```ts
export type Provider = "claude" | "codex";

export type ProfileSource = "builtin" | "user" | "project" | "inline";

export interface SpawnProfile {
  id: string;
  name: string;
  setupCommand?: string;
  startupCommand?: string;
  startupBehavior?: "autoRun" | "typeOnly";
  env?: Record<string, string>;
  cwdOverride?: string | null;
  icon?: string;
  provider?: Provider;
  source: ProfileSource;
}

export type SpawnProfileRef =
  | { kind: "registered"; id: string }
  | { kind: "inline"; profile: SpawnProfile };
```

- Both `setupCommand` and `startupCommand` are optional shell strings. When both are set, `setupCommand` runs first. The split exists purely for display grouping so setup output can be distinguished from main command output in pane history. Semantically, `setupCommand; startupCommand` is equivalent to a single chained command.
- `startupBehavior` defaults to `"autoRun"`. `"typeOnly"` types the command into the shell without pressing Enter, letting the user review before submitting.
- `provider` is an optional UX hint. A profile with `provider: "claude"` tells the UI "panes launched from this are expected to produce Claude hook events" — used for pane-header labeling and for showing the Claude resume picker. It does not gate status routing or notifications, which come entirely from `agentState`.
- `source` tracks where the profile was defined. The loader sets `"builtin"` for profiles from provider modules, `"user"` for profiles from settings, `"inline"` for ad-hoc profiles from the "Custom…" picker. `"project"` is reserved for `.roux/profiles.json` and never produced in v1.
- `SpawnProfileRef` is what a pane persists. `kind: "registered"` is a stable id pointer; restore re-resolves it from the current registry. `kind: "inline"` captures the entire profile so ad-hoc panes survive restore without depending on registry state.

**`AgentState`** (new, `src/lib/panes/agentState.ts`):

```ts
export type AgentStatus = "idle" | "generating";

export interface AgentState {
  provider: Provider;
  status: AgentStatus;
  permissionInfo?: PermissionInfo;
  providerSessionId?: string;
  source: "hook" | "osc" | "notify";
  updatedAt: number;
}

export const agentStates: Writable<Map<string, AgentState>>;
```

- Runtime only; not persisted.
- A pane has no `agentState` entry until the first event arrives. Until then the pane is a plain shell to the UI.
- Entries clear on pane disposal and on session close. Entries do _not_ clear on agent idle — `idle` is a valid persistent state until superseded by a later event or the pane closes.
- There is no `"disconnected"` status. PTY death is observable at the pane level (dead-pane view). Agent process death without shell death manifests as `agentState` sitting at its last value — acceptable for v1. A later pass can add OSC 133 prompt-marker detection to auto-clear stale agent state on the next shell prompt.

**`PaneInstance` and `PaneDescriptor`** (modified, `src/lib/panes/instances.ts`, `persistence.ts`):

```ts
export type PaneType = "shell" | "markdown" | "command";

export interface PaneInstance {
  id: string;
  type: PaneType;
  ptyId: string;
  spawnProfileRef?: SpawnProfileRef;
  // ...existing fields unchanged
}

export interface PaneDescriptor {
  id: string;
  type: PaneType;
  ptyId: string;
  spawnProfileRef?: SpawnProfileRef;
  // ...existing fields unchanged
}
```

- Every `type === "claude"` check in the existing frontend is rewritten. Most become `type === "shell"` (or are deleted if the check was vacuous after the rename).
- Claude-specific UI components (permission UI, resume picker) are gated on `agentState?.provider === "claude"`, not on pane type.
- The frontend adds a `schemaVersion: 3` marker to the pane-state payload. Lower versions are rejected on load and the session restores empty. Acceptable per the no-backcompat scope rule.
- The pane-state envelope on disk (Rust side, `pane_state.rs`) stays at version `1` because Rust treats the blob opaquely.

### Backend

`src-tauri/src/services/sessions.rs`, `src-tauri/src/services/providers/`, `src-tauri/src/pty.rs`:

- `Session` does **not** gain a `provider` field. Supersedes the parent Codex plan.
- `Session.status` is removed from the persisted session model. Downstream reads in `crates/roux-core/src/models/session.rs`, `src-tauri/src/session_service.rs`, `src/lib/stores/sessions.ts`, `src/App.svelte`, `src/lib/sessions/close.ts` all move to reading the frontend's derived aggregate store.
- `pty.rs` stays a generic PTY primitive. There is no `spawn_ai_pane` command. The only change to the shell spawn path is unconditionally injecting `ROUX_SESSION_ID` and `ROUX_PANE_ID` into the PTY env, alongside any `env` additions from the spawn profile.
- The `providers/` module shrinks to three jobs: (1) install hooks into the target agent's config files, (2) write provider-side feature flags where needed (e.g., Codex `[features].codex_hooks = true`), (3) contribute one or more built-in `SpawnProfile`s to the registry. Each provider module is a few hundred lines of focused Rust; no shared trait, no spawn config plumbing.
- A new Tauri command `get_builtin_profiles()` returns `Vec<SpawnProfile>` assembled from each provider module's `default_profiles()` function. Called once at frontend startup to populate the built-in segment of the profile registry.
- `services/status_integrations.rs` (from parent plan) stays. Parses provider-specific hook payloads, normalizes to the generic event shape that flows into `agentStates` on the frontend.
- `hooks.rs` / `cli.rs` hook bridge gains the `ROUX_PANE_ID`-aware invocation form from the earlier spec revision. Unchanged from that decision.
- `status_watcher.rs` generalization is unchanged: drop Claude-specific field names, emit provider-aware events, route by `roux_pane_id` first with legacy fallbacks restricted to notification-only updates.

### Hook protocol and routing

Unchanged from the earlier spec revision:

- `ROUX_PANE_ID` is unconditionally set in every shell PTY's env.
- Hook bridge writes `~/.config/roux/status/<uuid>.json` with `provider`, `roux_session_id`, `roux_pane_id`, `provider_session_id`, `cwd`, `status`, and optional tool/message fields.
- `status_watcher.rs` matches in order:
  1. Exact `roux_pane_id` → updates that pane's `agentState`.
  2. `roux_session_id` only (legacy) → routed to the session's notification service only; updates a pane's `agentState` only if that session has exactly one shell pane at the event's cwd, otherwise logged and dropped.
  3. `cwd` alone → same as tier 2.

Tiers 2 and 3 exist only for legacy hook installs that predate `ROUX_PANE_ID`. Roux-spawned panes always hit tier 1.

## Spawn Profile Registry

### Sources and precedence

The registry is populated at app start from four sources, in order of precedence (later sources override earlier ones on id collision):

1. **Built-in** — provider modules contribute profiles via `default_profiles()`. Claude contributes `{ id: "claude", name: "Claude", startupCommand: <settings-derived>, provider: "claude", source: "builtin" }`. Codex contributes the equivalent. A provider module may return multiple profiles if it has meaningful variants (e.g., Codex could return `codex` and `codex-exec`).
2. **User** — profiles from `RouxSettings.spawnProfiles: SpawnProfile[]`. Edited as raw JSON in the settings file. No GUI editor in v1. The settings loader force-sets `source: "user"` on each loaded profile regardless of what's in the JSON.
3. **Project** — reserved in the schema but not loaded in v1. See "Trust model" below.
4. **Inline** — created by the user from the "Custom…" picker option. Not registered; captured directly on the pane's `spawnProfileRef` as `{ kind: "inline", profile }`.

Sources 1–3 produce `SpawnProfileRef` values of `kind: "registered"`. Source 4 produces `kind: "inline"`.

### Scripts in user profiles

User profiles may put arbitrary shell commands in `setupCommand` and `startupCommand`. The shell interprets them — multiline scripts, piped commands, conditional chains, subprocess invocations, whatever the user writes. Examples:

```json
{
  "id": "claude-with-mcp",
  "name": "Claude + MCP servers",
  "setupCommand": "set -euo pipefail; ./scripts/start-mcp-servers.sh",
  "startupCommand": "claude --mcp-config ~/.config/mcp.json",
  "provider": "claude"
}
```

```json
{
  "id": "dev-server",
  "name": "Dev server",
  "startupCommand": "bun run dev",
  "env": { "NODE_ENV": "development" }
}
```

User profile scripts run at the user's own trust level. They live in the user's own settings file, the same trust level as `~/.zshrc` or `.envrc`. No extra sandboxing.

### What a user profile cannot do

A user profile cannot contribute a new value to the `Provider` enum. `agentState.provider` is `"claude" | "codex"` at the type level (plus whatever a future provider module ships). A user profile wrapping a custom agent has two choices:

1. **Piggyback on an existing provider.** If the custom tool speaks a Claude-compatible protocol, set `provider: "claude"`. Provider-specific UI lights up. Correct if the protocol actually matches; misleading if not.
2. **Omit `provider` entirely.** The profile runs, the shell hosts the tool, the user can call `roux notify` from their own script for notification-level integration, but session-card status and inline permission UI stay dark.

For full first-class agent UI, the user needs a provider module written in Rust. That's a deliberate line: **everything data-shaped lives in user config; everything code-shaped lives in the app.** User-defined profiles cannot define provider modules because provider modules do things (write into third-party config files with merge-preserving logic, parse provider-specific hook payloads) that require code and privileged I/O.

### Trust model for project profiles

Project profiles (`.roux/profiles.json` committed in a repo) are deferred to a later pass, but the trust model is locked in now so the schema does not need to change when they land.

When project profiles ship, they follow the VS Code workspace-trust pattern:

- On first open of a session whose root contains `.roux/profiles.json`, Roux prompts: **"Do you trust the authors of `<repo path>`? This workspace defines spawn profiles that can run shell commands on your machine."** Options: `Trust workspace`, `Don't trust`, `View profiles`.
- Untrusted workspaces: `.roux/profiles.json` is not loaded. Roux continues to work normally; only the project-profile portion of the registry is empty.
- Trusted workspaces: `.roux/profiles.json` is loaded and merged into the registry with `source: "project"`.
- Trust decisions persist in `RouxSettings.trustedWorkspaces: string[]` (absolute paths). Can be revoked from settings at any time.
- Gating rule: v1 will treat _any_ project profile as requiring trust. A later pass may relax this so profiles that only set `env` / `cwdOverride` (no commands) can load without a prompt, but that's a follow-up refinement.

**For v1 this means:** the loader never reads `.roux/profiles.json`, `source: "project"` never appears in the registry, the trust-prompt UI does not exist. The `RouxSettings.trustedWorkspaces` field _is_ defined and persisted so it's available when project profiles land. This costs ~5 lines of schema and zero runtime overhead today.

### Inline "Custom…" profiles

The "Custom…" option in the new-session and split pickers opens an inline editor with fields for name (optional), `setupCommand`, `startupCommand`, `startupBehavior`, and a collapsed-by-default env + cwd section. Submitting creates a `SpawnProfile` with `source: "inline"`, a generated `id` (e.g. `inline-<uuid>`), and attaches it to the new pane as `spawnProfileRef: { kind: "inline", profile }`. The profile is _not_ added to the registry.

A nice-to-have follow-up ("Save as user profile" button on inline-profile panes) can promote an inline profile into a user profile by appending it to `RouxSettings.spawnProfiles`. The inline JSON format round-trips trivially into user settings.

## Session Creation And Pane Lifecycle

### Creating a session

1. User opens the new-session dialog. Picks a spawn profile (built-in, user, `Plain shell`, or `Custom…`). Picks cwd and worktree options. There is no separate "provider" selector — provider is an attribute of the profile.
2. Frontend calls `create_session` with cwd and worktree config. Receives `session_id`.
3. Frontend calls `spawn_shell_pane { session_id, pane_id, cwd, env }`. The backend unconditionally adds `ROUX_SESSION_ID` and `ROUX_PANE_ID` to the env; the frontend's `env` additions (from the profile's `env` field) merge in. Receives `pty_id`.
4. Frontend creates the pane instance with `spawnProfileRef` attached, places it as a single-leaf layout, persists pane state.
5. Frontend waits for PTY readiness, then writes `setupCommand` (if any) followed by `startupCommand` (if any) into the PTY, respecting `startupBehavior`. Profiles with no commands produce a plain shell — no writes.

Session creation and profile command injection are sequential steps in the frontend. The backend has no knowledge of profiles.

### Adding another pane

- Pane menu and command palette gain `Split right → <profile>` / `Split down → <profile>` actions. The user picks a profile from the same picker as the new-session dialog. A submenu groups by source: **Built-in**, **User**, **Plain shell**, **Custom…**.
- Action: `spawn_shell_pane { session_id, pane_id: <new>, cwd: <from focused pane>, env }` → insert a new leaf next to the focused pane → run the profile's setup + startup commands.
- Keyboard shortcuts: `Split → Claude` and `Split → Codex` default to the respective built-in profile. Additional user profiles can be given keybinds from settings (nice-to-have, additive).

### Closing a pane

- `disposePane` kills the shell PTY, same as today for shell panes. The `provider === "claude"` exception in `disposePane` is removed entirely.
- Mid-turn guard: if `agentState?.status === "generating"` and `confirmOnClose` is set, prompt the user. Otherwise kill immediately.
- Closing the last pane leaves the session open with zero panes; aggregate session status becomes `null`.

### Re-running a profile

- The pane header shows a "Re-run profile" button when the pane has a `spawnProfileRef`. Clicking it writes the profile's setup + startup commands into the existing shell again — no PTY respawn, no new shell process.
- The button does not auto-interrupt a busy shell or mid-turn agent. If the shell is at a prompt, the commands execute. If the shell is busy, the commands are queued into the shell's input buffer and the user is responsible for whatever happens. (A nice-to-have: confirmation prompt when `agentState?.status === "generating"`, deferred.)
- Useful for: restarting Claude after an agent crash, re-running `bun run dev` after editing config, rerunning a test watcher after killing it.

### Startup restore

- Existing session-restore path loads pane state and rehydrates instances. Shell panes auto-respawn their PTY (matching today's shell behavior). `ROUX_SESSION_ID` and `ROUX_PANE_ID` inject on the fresh PTY.
- Profile setup and startup commands are **not** auto-run on restore. The restored pane is a live shell with no prior agent running. The "Re-run profile" button is visible and the user clicks it to re-execute if desired.
- Rationale: avoids surprising users by re-running heavy setup scripts at app start, avoids accidental double-spawn of a profile's sidecar processes, mirrors the earlier spec revision's decision to restore AI state as "click to re-run" rather than "auto re-run".
- Inline profiles survive restore with their captured content. Registered profiles re-resolve from the registry at restore time; if the profile is missing (user deleted it, provider module unloaded), the pane still restores as a live shell but the "Re-run profile" button is hidden.

### Main-pane invariant removal

Today's code hard-codes a non-closeable `<sessionId>-main` Claude pane per session. This spec removes that invariant. Concretely:

- The `mainPaneId(sessionId)` helper and every check of the form `` paneId === `${sessionId}-main` `` are deleted.
- `src/lib/panes/actions.ts` no longer refuses to close a pane because it is the session's main pane.
- `src/lib/queries/index.ts` lookups that assume a main pane exists per session are rewritten to "find the first shell pane in the session, or null".
- `src/lib/sessions/reconnect.ts` targets an explicit pane id instead of the session's main pane.
- UI surfaces that need "the session's primary shell" use "most-recently-focused shell, falling back to the first shell in layout order, falling back to any pane".

Sequenced early in the implementation plan so subsequent changes operate on the new model. This must happen _before_ the `claude → shell` pane-type unification so intermediate commits leave the app runnable.

## Observed Agent State And Notifications

### Session card aggregate

A new derived store `sessionAgentStatus: Derived<Map<sessionId, AggregateStatus | null>>` walks each session's shell panes and reads their `agentState`:

- `generating` when any pane has `agentState?.status === "generating"`.
- `idle` when at least one pane has `agentState?.status === "idle"` and none is generating.
- `null` when no shell pane in the session has an `agentState` entry.

The sidebar card subscribes to `sessionAgentStatus` instead of reading `session.status`. Dot color and text come from the aggregate. No provider badge on the card in v1.

There is no explicit `disconnected` aggregate state. A session whose agent crashed sits at whatever `agentState.status` the agent last reported, until the user closes or re-runs the profile on that pane. Acceptable tradeoff for v1; OSC 133 prompt-marker detection can auto-clear stale state in a later pass.

### Notifications (per-pane)

- Notification service receives `pane-status-update` events and fires per-pane on `generating → idle` transitions. Two panes finishing in the same session produces two notifications.
- Title and body include the pane name and `agentState.provider`. A profile-launched pane uses the profile's `name` as a label default; the user can rename. An unmanaged pane with agent state uses a generated default.
- Window-focus suppression is unchanged.
- Disconnect events from the backend `pane-disconnected` emitter fire per pane.

The aggregate and per-pane rules are deliberately different abstractions: the card answers "is this session busy?" at a glance; the notification answers "did the specific work I was waiting on just finish?"

### Provider-specific UI, gated on observed state

- **Claude Allow/Deny permission UI** — shown when `agentState?.provider === "claude" && agentState.permissionInfo != null`. The pane header renders the existing inline buttons. The concurrent notification-pane plan is retiring these in favor of the notification service; this spec does not block that evolution.
- **Claude resume picker** (`SessionPicker.svelte`) — shown from the pane header when either `spawnProfileRef` resolves to a profile with `provider === "claude"` or the pane has a live `agentState?.provider === "claude"`. Picking a past Claude session writes `claude --resume <id>` into the shell.
- **Codex-specific UI** — none in v1. Reserved for future provider-specific affordances.

Gating provider UI on observed `agentState` rather than profile or pane type has a concrete payoff: an unmanaged shell where the user types `claude` by hand gets the same first-class Allow/Deny UI as a profile-launched pane, because Claude's hooks are installed globally and fire regardless of who spawned the binary.

## Relationship To The Parent Codex Plan

The parent plan (`docs/plans/2026-04-10-codex-cli-support-via-services-commands.md`) splits roughly in half under this architecture:

**Survives unchanged**

- Hook install for Claude and Codex (including `~/.codex/config.toml` feature flag and non-destructive `~/.codex/hooks.json` merge).
- `services/status_integrations.rs` with provider-specific payload parsing.
- Codex status mapping: `SessionStart → idle`, `UserPromptSubmit → generating`, `Stop → idle`.
- `services/notifications.rs` policy layer.
- Setup UI rename to "provider integrations".
- `roux-cli hook` provider-aware form.
- `status_watcher.rs` generalization.
- Settings fields `codexBinaryPath`, `sessionNotificationsEnabled`.

**Shrinks**

- The `providers/` module. Drops "spawn config" and "shared trait for spawn args/env" entirely. Keeps hook install, feature flag writing. Gains "contribute built-in spawn profiles to the registry".

**Disappears**

- `Session.provider` field.
- `spawn_ai_pane` Tauri command and provider-aware PTY spawn path.
- `create_session` provider parameter.
- Pane type `"claude"`.
- `aiStatus: "disconnected"` as a persistent state (subsumed by "no `agentState` entry").
- `defaultModel` / `additionalFlags` as spawn inputs. They become inputs to the Claude built-in profile's default `startupCommand` string.

Net: Codex support gets _smaller_ under this architecture, not bigger.

## UI Changes

### New-session dialog

- Single profile picker dropdown with sections: **Built-in** (Claude, Codex, Plain shell), **User** (from settings, if any), **Custom…** (opens inline editor).
- Selecting Claude or Codex reveals a "Customize command" expander with the default command string prefilled from settings. Editing it converts the selection from registered to inline.
- No separate provider picker. No dedicated model dropdown. Both collapse into the profile string.

### Pane menu and command palette

- `Split right → <profile>` and `Split down → <profile>` split commands, grouped under a "Spawn profile" submenu next to the existing `Split → Shell` command.
- Command palette entries mirror the menu.
- Default-new-profile resolution for "Split → Claude" keyboard shortcut: most-recently-focused pane's profile if it had `provider: "claude"`, falling back to the `claude` built-in profile. Same pattern for Codex.

### Session card

- Subscribes to `sessionAgentStatus`. Dot color and text come from the aggregate. No provider badge in v1.

### Pane header

- Displays pane name (defaults to profile name if attached, otherwise a generated default), `agentState` dot if present, provider-specific affordances if applicable.
- "Re-run profile" button visible when `spawnProfileRef` is set.
- (Deferred) "Save as profile" button on inline-profile panes, to promote an inline profile into a user profile.

### Claude-only UX surfaces getting provider-scoped

- `src/lib/components/SessionPicker.svelte` — Claude-specific resume picker. Opens from a pane header when the pane's `agentState.provider === "claude"` or the pane's profile has `provider: "claude"`.
- `src/lib/components/PaneShell.svelte` — currently embeds Claude-specific UI branches. These are guarded behind `agentState?.provider === "claude"` and the pane-type check on the `"claude"` enum value is deleted.

### Settings

- `RouxSettings.spawnProfiles: SpawnProfile[]` — user profile list, edited as JSON.
- `RouxSettings.trustedWorkspaces: string[]` — reserved for the project-profile trust model, unused in v1.
- `claudeBinaryPath`, `codexBinaryPath`, `defaultModel`, `additionalFlags`, `sessionNotificationsEnabled` — all stay as settings fields. Their _consumers_ change: `claudeBinaryPath` etc. are consulted by the Claude provider module when constructing the default built-in profile's `startupCommand`, not by a spawn path.

### What is explicitly not changing

- Pane-tree invariants, split flattening, tab stacking, worktree UX, notes, watches, tasks.
- Shell, markdown, command panes at the structural level.
- Socket / CLI protocol wire shape. Internally the socket `session-create` path creates a shell with the default built-in Claude profile to match current UX; the protocol fields stay as they are.

## Test Plan

### Frontend unit (Vitest)

- `panes/profiles.ts`:
  - Registry loads built-in + user profiles, with user overriding built-in on id collision.
  - `SpawnProfileRef` of `kind: "inline"` round-trips through persistence with its captured profile intact.
  - Missing registered profile on restore resolves to a shell pane with no re-run button visible.
- `panes/agentState.ts`:
  - `updateAgentState(paneId, event)` creates and updates entries.
  - `disposeAgentState(paneId)` clears the entry on pane disposal.
  - `sessionAgentStatus` follows the aggregate rules: `generating` dominates, `idle` when any idle and none generating, `null` when empty.
- `panes/instances.ts`:
  - Creating a shell pane results in `ROUX_SESSION_ID` and `ROUX_PANE_ID` in its env (verified via mock PTY spawn).
  - `disposePane` unconditionally kills the PTY — the old `claude`-type exception is gone.
- `panes/persistence.ts`:
  - Round-trip a layout with plain shells, registered-profile shells, and inline-profile shells.
  - Load rejects payloads with `schemaVersion < 3`; the session restores empty.
- Pane actions:
  - `Split → Claude` picks the Claude built-in profile and writes its startup command into the new shell.
  - `Split → Custom…` collects an inline profile and creates the pane from it.
- Notifications:
  - `generating → idle` on one pane fires one notification.
  - Title includes the pane name and `agentState.provider`.
  - Window-focus suppression still applies.

### Backend unit (Rust)

- `providers/claude.rs::default_profiles()` returns a Claude profile with the settings-derived `startupCommand`.
- `providers/codex.rs::default_profiles()` returns a Codex profile.
- `pty.rs` shell spawn injects `ROUX_SESSION_ID` and `ROUX_PANE_ID` into env on every spawn, regardless of caller.
- `status_watcher.rs`:
  - Updates the correct pane's `agentState` when `roux_pane_id` is present.
  - Legacy events (only `roux_session_id`, or only `cwd`) route to the notification service only and update a pane's `agentState` only when exactly one pane in the target session matches at that cwd; otherwise log and drop.
- `hooks.rs` / `cli.rs` hook bridge reads `ROUX_PANE_ID` from env and emits it in the status JSON.
- `services/status_integrations.rs` Claude and Codex payload normalization into the generic event shape.

### Integration and manual

- Create a session with the Claude profile. Pane spawns, `claude` runs automatically, status lights up on first hook event, notification fires on idle.
- Split right with the Codex profile. Both panes run independently, session card aggregates, notifications distinguish panes by name and provider.
- Create a session with `Plain shell`. Pane is a live shell with no profile commands; `agentState` is absent.
- Create a session with `Custom…`. Inline profile's setup + startup commands run.
- Edit `spawnProfiles` in settings to add a dev-server profile with `startupCommand: "bun run dev"`. New-session dialog shows it. Creating a pane from it runs the command, and the pane inherits `ROUX_SESSION_ID` + `ROUX_PANE_ID` so `roux notify` from the dev server reports into the right pane.
- Kill a Codex process out-of-band. `agentState` sits at its last value; the shell remains alive. User re-runs the profile to restart Codex.
- Full quit plus relaunch: panes restore as live shells. Re-run buttons appear for profile-attached panes. Clicking re-runs the profile commands in the existing shell.
- Close a pane while `agentState?.status === "generating"`: confirm prompt appears.
- Close the last pane in a session: session stays open with zero panes.
- Install Claude hook config via setup UI. Without `ROUX_PANE_ID`-aware invocation (simulating a legacy install), events for a session with exactly one shell pane update that pane's `agentState`; events for a session with multiple shell panes are dropped with a log line.

### What is not tested here

- Codex permission-request parity (out of scope).
- Project-profile loading (deferred).
- GUI profile editor (deferred).
- OSC 133 prompt-marker-based `agentState` auto-clearing (deferred).

## Open Questions

None blocking. Items for the implementation plan to sequence:

1. Order of main-pane invariant removal vs. the `claude → shell` type unification. Removal must come first to keep intermediate commits runnable.
2. Whether the "Re-run profile" button confirms before writing into a busy shell. Default in the spec is "no confirmation"; may warrant a per-session preference.
3. Exact default `startupCommand` string for the built-in Claude profile when `claudeBinaryPath`, `defaultModel`, and `additionalFlags` combine. The provider module needs a small shell-quoting helper; the helper's exact rules are an implementation detail.
4. Whether "Save inline profile as user profile" ships in v1 or later. Additive either way.
