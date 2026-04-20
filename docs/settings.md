# Settings

Open settings with ++cmd+","++ or from the command palette.

## Overview

Settings are grouped into categories in a sidebar modal. Changes are persisted automatically.

## Sections

- **General** — theme, tab position, status bar position.
- **Sessions** — close/reconnect behavior, default project path, repo roots quick-pick sources, worktree base template, worktree cleanup mode, and the New Worktree default starting point.
- **Terminal** — font, scrollback, and cursor settings.
- **Claude** — binary path override, default model, additional flags.
- **Integrations** — GitHub CLI (`gh`) path override for PR/session integrations.
- **Notifications** — OS notification master switch and test notification trigger.
- **Keyboard** — toggles for Option-pane and Command-session hint overlays.
- **Notes** — experimental multi-scoped vault settings. See below.
- **Advanced** — app version and updater controls.

## Notes (experimental)

The ++cmd+b++ notes panel writes to an Obsidian-compatible vault on disk.

- **Vault root** (`notes.vaultRoot`) — absolute path to the vault folder. Defaults to `~/Documents/Roux` on first note write. Point this at an existing Obsidian vault if you'd rather co-locate Roux notes with your personal second brain. Changing this setting does **not** move existing content; copy the folder manually before pointing Roux at a new location.
- **Include web anchors for entries** (`notes.includeWebAnchors`, default on) — when enabled, `roux notes <scope> append --timestamp` adds an inline `<a id="entry-...">` HTML anchor in front of each timestamped entry so the entry stays deep-linkable if the vault is later published through a static-site generator (Quartz, Hugo, Zola, MkDocs, …). Disable for cleaner raw markdown if you only ever read the vault in Obsidian.

See [Notes](features/notes.md) for the panel UX, CLI surface, and env vars.

## Doctor panel

The **Advanced** section includes a **Doctor** panel that checks integration health for:

- Roux CLI install/version
- hook install state
- Claude Code skill install state
- GitHub CLI availability

Use **Install / Update / Reinstall** actions per item if anything is missing or stale.

## Updates

The Updates section shows the currently running Roux version and lets you manage the built-in auto-updater.

- **Check for updates** — runs a manual check against the release server. If a new version is available, release notes appear inline along with an **Install and restart** button.
- **Check for updates on launch** — when enabled (the default), Roux silently checks for a new version a few seconds after startup. If one is available, a small banner appears at the top of the window with **Install and restart** and **Later** buttons. Disabling this means you'll only see updates when you click **Check for updates** manually.

Updates are signed by Roux's release key and verified on your machine before they're installed. A signature failure always surfaces visibly — Roux will never silently ignore one.

After a successful install, Roux tries to relaunch itself into the new version. If the automatic relaunch fails (a known macOS quirk after the app bundle is swapped in place), the banner and this section will both show **"Update installed. Quit and reopen Roux to finish."** with a **Quit Roux** button — click it and reopen the app to complete the update.

See the [Troubleshooting](troubleshooting.md) page if a setting doesn't seem to take effect.
