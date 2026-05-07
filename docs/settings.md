# Settings

Open settings with ++cmd+","++ or from the command palette.

## Overview

Settings are grouped into categories in a sidebar modal. Changes are persisted automatically.

## Sections

- **General** — theme, tab position, status bar position.
- **Sessions** — close/reconnect behavior, default project path, repo roots quick-pick sources, worktree base template, worktree cleanup mode, and the New Worktree default starting point.
- **Terminal** — independent terminal theme selection, user-imported `.itermcolors` themes, font, scrollback, cursor, and GPU acceleration settings.
- **Claude** — binary path override, default model, additional flags.
- **Integrations** — GitHub CLI (`gh`) path override, Smol Machines binary path, and managed-proxy lifecycle config.
- **Agent Integrations** — Roux MCP enablement and supported MCP host setup.
- **Notifications** — OS notification master switch, test notification trigger, and Claude/Codex agent notification setup.
- **Keyboard** — toggles for Option-pane and Command-session hint overlays.
- **Notes** — experimental multi-scoped vault settings. See below.
- **Advanced** — app version, updater controls, update channel, logging, and the Doctor panel.

## Terminal themes

The **Terminal** section controls xterm's palette separately from the rest of the app.

- **Terminal theme** (`terminalTheme`) — choose between auto-follow, built-in app-matched palettes, built-in editor-style palettes, or user themes.
- **User themes** — drop iTerm2 `.itermcolors` files into `~/.config/roux/themes/`, then use **Reload** to rescan them or **Reveal** to open the folder in your file manager.
- Missing user themes are preserved as setting values until you restore the file or pick a different theme, so Roux does not silently overwrite a temporarily unavailable palette.

## GPU acceleration

Terminal panes use a WebGL renderer by default for smooth rendering of large output buffers. The **Terminal** section exposes a **GPU acceleration** dropdown for the rare cases where the default isn't right.

- **Auto** (default) — try WebGL; if construction fails or the WebGL context is lost (GPU process crash, suspended tab, too many WebGL contexts on the page), fall back silently to xterm's built-in DOM renderer. Recommended for almost everyone.
- **On (WebGL)** — same fallback behavior as Auto today; reserved as a distinct option in case future Roux versions add a Canvas tier or stricter "WebGL only" semantics.
- **Off (DOM)** — skip WebGL entirely and use the DOM renderer. Useful if you suspect a driver/GPU issue, are running over a remote desktop with poor GL passthrough, or just want lower power draw at the cost of slower scrollback rendering.

The setting (`gpuAcceleration`) applies to terminal panes opened **after** you change it. Existing panes keep their current renderer until they're closed and reopened (or the session is restarted).

## Notes (experimental)

The ++cmd+b++ notes panel writes to an Obsidian-compatible vault on disk.

- **Vault root** (`notes.vaultRoot`) — absolute path to the vault folder. Defaults to `~/Documents/Roux` on first note write. Point this at an existing Obsidian vault if you'd rather co-locate Roux notes with your personal second brain. Changing this setting does **not** move existing content; copy the folder manually before pointing Roux at a new location.
- **Include web anchors for entries** (`notes.includeWebAnchors`, default on) — when enabled, `roux notes <scope> append --timestamp` adds an inline `<a id="entry-...">` HTML anchor in front of each timestamped entry so the entry stays deep-linkable if the vault is later published through a static-site generator (Quartz, Hugo, Zola, MkDocs, …). Disable for cleaner raw markdown if you only ever read the vault in Obsidian.

See [Notes](features/notes.md) for the panel UX, CLI surface, and env vars.

## Agent Integrations

The **Agent Integrations** section configures Roux for MCP hosts such as Claude Desktop.

- **Enable Roux MCP** (`mcpEnabled`) — allows MCP hosts to launch `roux-cli mcp` and use Roux through the running desktop app. Turning this off makes the MCP server return a disabled error even if a host can start the process.
- **CLI status** — shows whether Roux's installed CLI is present and current, plus the path that host configs will use.
- **Host status** — shows supported MCP hosts, whether their config file exists, whether Roux is configured, and any config parse/read error.
- **Preview** — shows the Roux MCP server entry that would be written for the host before changing the host config.
- **Configure** — adds or updates only Roux's MCP server entry for that host.
- **Last configured** — records the last successful host setup metadata when possible.

Roux reads an existing host config before writing. It preserves unrelated config and unknown fields on Roux's own server entry, skips writes when the entry is already current, and writes updates atomically.

For v1, MCP exposes useful inspection and safe action tools by default. It does not expose arbitrary shell execution, PTY kill, worktree removal, permanent session deletion, or broad filesystem mutation.

## Smol Machines

Under **Integrations → Smol Machines**, two related controls bundle the [Smol Machines](features/smol-machines.md) integration's host-side config:

- **smolvm binary path** (`smolvmBinaryPath`) — overrides the auto-detected `smolvm` on `$PATH`. Leave blank to use whichever `smolvm` Roux finds first. The whole integration (sidebar icon, panel, session bindings) hides itself when no binary is detected.
- **Managed HTTP proxy** (`managedProxy`) — optional. Lets Roux orchestrate a host-side HTTP proxy that the guest VM routes through. Configure a **command**, **port**, and optional **bind address**. Examples (Roux ships no proxy code; pick whatever you've installed):
    - `tinyproxy -d -c ~/.config/roux/tinyproxy.conf` on port `8888`
    - `mitmdump --mode regular --listen-port 8888` on port `8888`
    - `squid -N -f /path/to/squid.conf` on port `3128`

When configured, the Smol Machines panel header shows a Shield toggle. Roux spawns the command via `sh -lc`, polls the listen socket up to 5 seconds to confirm it bound, and SIGTERM/SIGKILLs on stop and on app quit. The create form's "Host HTTP proxy URL" auto-fills with `http://<bind>:<port>` when the proxy is running.

## Notifications

The **Notifications** section controls both desktop notification fan-out and agent notification setup.

- **Enable OS notifications** (`notificationsEnabled`) — controls whether Roux forwards qualifying events to the operating system. The in-app notifications pane, unread badges, and session indicators still work when this is off.
- **Agent notifications** — checks whether supported agent CLIs are configured to report back to Roux.
- **Claude Code** — uses Roux's hook installer. **Configure** installs missing hooks and the Roux CLI if needed; **Reinstall** refreshes an already-configured hook setup.
- **Codex** — reads `~/.codex/config.toml` and checks `[tui].notification_condition`. **Preview** shows the full TOML content Roux would write. **Configure** creates or updates the file so Codex uses `notification_condition = "always"`, while preserving unrelated config where possible.
- **Test notification** — sends a sample notification through Roux's notification service so you can confirm OS permissions.

Codex config writes are intentionally previewable because `~/.codex/config.toml` may contain unrelated model, approval, sandbox, or provider settings. Roux only targets the `notification_condition` entry under `[tui]`.

## Doctor panel

The **Advanced** section includes a **Doctor** panel that checks integration health for:

- Roux CLI install/version
- hook install state
- Claude Code skill install state
- GitHub CLI availability

Use **Install / Update / Reinstall** actions per item if anything is missing or stale.

## Updates

The **Advanced** section shows the currently running Roux version and exposes the built-in auto-updater controls.

- **Check for updates** — runs a manual check against the release server. If a new version is available, release notes appear inline along with an **Install and restart** button.
- **Update channel** (`updateChannel`) — choose **Stable** or **Pre-release (Alpha)**. Stable follows `latest.json`; Pre-release follows the newest published prerelease manifest. Switching back to Stable only takes effect once a stable release exists at or above the version you're currently running.
- **Check for updates on launch** — when enabled (the default), Roux silently checks for a new version a few seconds after startup. If one is available, a small banner appears at the top of the window with **Install and restart** and **Later** buttons. Disabling this means you'll only see updates when you click **Check for updates** manually.

Updates are signed by Roux's release key and verified on your machine before they're installed. A signature failure always surfaces visibly — Roux will never silently ignore one.

After a successful install, Roux tries to relaunch itself into the new version. If the automatic relaunch fails (a known macOS quirk after the app bundle is swapped in place), the banner and this section will both show **"Update installed. Quit and reopen Roux to finish."** with a **Quit Roux** button — click it and reopen the app to complete the update.

See the [Troubleshooting](troubleshooting.md) page if a setting doesn't seem to take effect.
