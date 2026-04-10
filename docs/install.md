# Install

Roux ships signed and notarized macOS builds. Windows and Linux are not yet supported.

## macOS

1. Download the latest `.dmg` from the [GitHub releases page](https://github.com/phin-tech/roux/releases).
2. Open the `.dmg` and drag **Roux** into `/Applications`.
3. Launch Roux from Launchpad or Spotlight.

The app is code-signed and notarized, so Gatekeeper should not block the first launch.

## Updating

Roux does not ship an auto-updater yet. To update, download the newest `.dmg` from the releases page and replace the existing `Roux.app` in `/Applications`.

## Uninstalling

Drag `Roux.app` from `/Applications` to the Trash. Per-user settings and session state live under `~/Library/Application Support/roux/` and can be removed separately.
