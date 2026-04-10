# Settings

Open settings with ++cmd+","++ or from the command palette.

!!! note "Stub page"
    This page is a placeholder. Detailed documentation for each settings section is still being written.

## Overview

Settings are grouped into sections for appearance, terminal behavior, sessions, and integrations. Changes are persisted immediately.

## Sections

- **Appearance** — theme and color scheme
- **Terminal** — font, scrollback, cursor behavior
- **Sessions** — default shell, Claude Code command, startup behavior
- **Projects** — per-project defaults
- **Notifications** — OS notification preferences
- **Updates** — current version, manual update checks, auto-check toggle

## Updates

The Updates section shows the currently running Roux version and lets you manage the built-in auto-updater.

- **Check for updates** — runs a manual check against the release server. If a new version is available, release notes appear inline along with an **Install and restart** button.
- **Check for updates on launch** — when enabled (the default), Roux silently checks for a new version a few seconds after startup. If one is available, a small banner appears at the top of the window with **Install and restart** and **Later** buttons. Disabling this means you'll only see updates when you click **Check for updates** manually.

Updates are signed by Roux's release key and verified on your machine before they're installed. A signature failure always surfaces visibly — Roux will never silently ignore one.

See the [Troubleshooting](troubleshooting.md) page if a setting doesn't seem to take effect.
