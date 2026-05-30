# Codex CLI Support Via Services/Commands Separation

> **Status:** Partially superseded by `docs/superpowers/specs/2026-04-11-spawn-profiles-and-agent-integration-design.md`. The provider-aware PTY spawn path and `Session.provider` field are gone; the install / status-normalization / notification work survives.

## Summary
Implement Codex support as a provider-aware extension of the new Rust layering, not as more Claude-specific conditionals. Tauri commands stay thin. All provider decisions, hook installation, hook payload normalization, status routing, and notification policy live in services or dedicated provider modules.

V1 includes first-class `codex` sessions, user-global Codex hook installation, normalized session status updates, and optional OS notifications for completed/error turns. V1 does not attempt Claude-style permission-request parity for Codex because the current Codex hook surface does not cleanly expose that state.

## Service Boundaries
- `commands/sessions.rs`
  - Accept `provider` on create/reconnect requests.
  - Forward to services only.
  - No binary selection, hook knowledge, or provider branching beyond request validation.
- `commands/setup.rs`
  - Return provider integration status for Claude and Codex.
  - Forward install actions to setup services only.
- `services/sessions.rs`
  - Own session creation/reconnect orchestration.
  - Resolve provider-specific spawn config through a provider adapter.
  - Persist `Session.provider`.
- `services/setup.rs`
  - Own install/check orchestration for provider integrations.
  - Expose “is Roux CLI installed”, “Claude hooks installed”, and “Codex hooks enabled/installed”.
- New provider module, e.g. `services/providers/` or `providers/`
  - `claude.rs`: current Claude command resolution and hook config generation.
  - `codex.rs`: Codex command resolution, hook config generation, and config feature enablement.
  - Shared trait or enum-backed adapter for spawn args/env and install behavior.
- New hook/status service, e.g. `services/status_integrations.rs`
  - Define the normalized on-disk hook payload Roux writes.
  - Parse provider-specific hook stdin and convert to one internal status event shape.
  - Keep backward compatibility for existing Claude hook installs.
- New notification service, e.g. `services/notifications.rs`
  - Own notification policy for normalized status transitions.
  - Suppress notifications while the Roux window is focused.

## Implementation Changes
- Shared models in `roux-core`
  - Add `Session.provider: "claude" | "codex"`.
  - Add settings fields:
    - `codexBinaryPath?: string | null`
    - `sessionNotificationsEnabled: boolean`
  - Keep `claudeBinaryPath`, `defaultModel`, and `additionalFlags` Claude-scoped in v1.
- PTY spawn path
  - Replace the Claude-only spawn contract in `pty.rs` with a provider spawn config input:
    - executable
    - args
    - env additions
    - display/provider metadata if needed
  - `pty.rs` remains a PTY/process primitive, not a product-policy layer.
- Hook installation
  - Claude installer stays equivalent to today, but moves behind provider setup code.
  - Codex installer:
    - ensure `~/.codex/config.toml` enables `[features].codex_hooks = true`
    - merge Roux entries into `~/.codex/hooks.json`
    - preserve unrelated user config and hooks
  - Auto-refresh on startup should call setup services, not `hooks.rs` directly.
- Roux CLI hook bridge
  - Keep legacy `roux-cli hook <status>` working for existing Claude installs.
  - Add provider-aware form for new installs so Codex hooks can invoke the same bridge safely.
  - Hook output written to `~/.config/roux/status/*.json` should include:
    - `provider`
    - `provider_session_id`
    - `roux_session_id` if inherited from env
    - `cwd`
    - normalized `status`
    - optional `message/tool_name/tool_input`
- Status ingestion
  - Generalize `status_watcher.rs` and frontend event payload names so they are not Claude-specific.
  - Match updates to Roux sessions by:
    1. `roux_session_id`
    2. provider + cwd fallback
  - This removes the current ambiguity for same-cwd sessions.
- Codex status mapping for v1
  - `SessionStart` -> `idle`
  - `UserPromptSubmit` -> `generating`
  - `Stop` -> `idle`
  - PTY exit -> `disconnected`
  - `PreToolUse` / `PostToolUse` are ignored for session-card permission UI in v1
- Frontend
  - New-session dialog adds a provider picker and passes `provider` to backend commands.
  - Claude-only controls remain visible only for Claude.
  - Setup UI becomes “provider integrations” rather than “Claude hooks”.
  - Session card can optionally display provider branding, but behavior stays otherwise shared.
  - Notifications are driven from normalized status events, not provider-specific UI code.

## Public Interfaces
- `Session` gains `provider`.
- `RouxSettings` gains `codexBinaryPath` and `sessionNotificationsEnabled`.
- `create_session` and `reconnect_session` gain `provider`.
- Setup status payload becomes provider-aware rather than one `cli_installed`/Claude-oriented flag.
- Frontend status event payload removes Claude-specific field names such as `claudeSessionId`.

## Test Plan
- Service-level Rust tests
  - provider spawn config generation for Claude and Codex
  - setup status/install orchestration
  - Claude settings merge preserves unrelated hooks
  - Codex `config.toml` feature enablement preserves unrelated TOML
  - Codex `hooks.json` merge preserves unrelated hooks
  - provider-aware and legacy `roux-cli hook` parsing/normalization
- Model/persistence tests
  - `Session.provider` round-trips through persisted sessions
  - new settings fields deserialize safely from older settings files
- Frontend/store tests
  - new-session dialog provider switch shows correct controls
  - status update matching prefers `roux_session_id`
  - Codex status updates do not populate Claude permission UI
  - notification gating only fires on `idle` completion and `error`
- Manual verification
  - Claude session still launches and updates status
  - Codex session launches from Roux and updates generating/idle/disconnected
  - Codex setup writes `~/.codex/hooks.json` and enables the feature flag
  - notifications only appear when enabled and Roux is unfocused

## Assumptions And Defaults
- Use the services/commands separation layer everywhere in this feature.
- New provider-specific code should live in service/provider modules, not in Tauri command handlers.
- V1 manages user-global Codex config only, not repo-local `.codex/hooks.json`.
- V1 is macOS/Linux only for Codex hooks because the current Codex docs say hooks are disabled on Windows.
- V1 targets status + notifications parity, not permission-request parity.
