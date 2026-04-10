# Install

Roux ships signed and notarized macOS builds. Windows and Linux are not yet supported.

## macOS

1. Download the latest `.dmg` from the [GitHub releases page](https://github.com/phin-tech/roux/releases).
2. Open the `.dmg` and drag **Roux** into `/Applications`.
3. Launch Roux from Launchpad or Spotlight.

The app is code-signed and notarized, so Gatekeeper should not block the first launch.

## Updating

Roux has a built-in auto-updater. When a new version is published, Roux checks for it silently on launch and shows a small banner offering to install and restart. You can also check manually at any time:

- **Settings** (++cmd+","++) → **Updates** → **Check for updates**
- Or **Command palette** (++cmd+k++) → **Check for Updates**

Updates are signed and verified on device before they're installed.

!!! note "First update from 0.2.x"
    Roux 0.2.x builds were shipped before the auto-updater existed. If you are on 0.2.x, the first updater-enabled release (0.3.0) still requires a one-time manual install — download the latest `.dmg` from the [releases page](https://github.com/phin-tech/roux/releases) and drag the new `Roux.app` into `/Applications`. Every update after that will install itself.

## Uninstalling

Drag `Roux.app` from `/Applications` to the Trash. Per-user settings and session state live under `~/Library/Application Support/roux/` and can be removed separately.
