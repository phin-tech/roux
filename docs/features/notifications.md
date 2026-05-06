# Notifications

Roux has one notification flow for agent events, watches, tasks, CLI pushes, and terminal notification escape sequences. Notifications appear in-app first, with optional operating-system fan-out when the event should interrupt you.

## Surfaces

- **Notifications pane** — the in-app inbox for recent notifications.
- **Activity rail badge** — shows unread notification count.
- **Session badges** — show unread notifications associated with a session.
- **OS notifications** — desktop notifications controlled by **Settings -> Notifications -> Enable OS notifications**.

Opening or focusing a notification marks it read where appropriate. The in-app pane remains available even when OS notifications are disabled.

## Agent Setup

Open **Settings -> Notifications** and use **Agent notifications** to check supported agent providers.

### Claude Code

Claude Code notifications are delivered through Roux's Claude hooks.

- **configured** means Roux sees a current CLI and installed hooks.
- **needs update** means the installed Roux CLI is stale.
- **not configured** means the CLI or hooks are missing.

Use **Configure** or **Reinstall** to run Roux's hook installer. The installer also refreshes the local `roux-cli` used by the hooks.

### Codex

Codex notifications are controlled by Codex's TUI config at:

```toml
[tui]
notification_condition = "always"
```

Roux checks `~/.codex/config.toml`.

- **Preview** shows the full TOML content Roux would write.
- **Configure** creates `~/.codex/config.toml` if needed, adds a `[tui]` section when missing, or updates an existing `notification_condition` value.

Roux preserves unrelated Codex config where possible and only targets `[tui].notification_condition`.

## Testing Desktop Notifications

Use **Settings -> Notifications -> Send test** to send a sample notification through the same service used by agents and automation.

On macOS development builds, Roux may appear under Terminal in System Settings because unsigned dev binaries cannot own the final bundle notification identity. Release builds use Roux's app bundle id.

