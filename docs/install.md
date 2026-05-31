# Install

Roux ships signed and notarized macOS desktop builds. Native Windows x64 desktop builds are also supported from source with an unsigned NSIS installer. The Linux desktop app is not yet supported, but standalone Linux CLI tarballs are published with releases.

## macOS

Install the desktop app with Homebrew:

```sh
brew install --cask phin-tech/tap/roux
```

Or install manually:

1. Download the latest `.dmg` from the [GitHub releases page](https://github.com/phin-tech/roux/releases).
2. Open the `.dmg` and drag **Roux** into `/Applications`.
3. Launch Roux from Launchpad or Spotlight.

The app is code-signed and notarized, so Gatekeeper should not block the first launch.

To install the latest prerelease desktop app:

```sh
brew install --cask phin-tech/tap/roux-pre
```

The Homebrew desktop casks currently require Apple Silicon.

## Standalone CLI

Release builds publish standalone `roux` CLI tarballs for macOS and Linux. To install the latest CLI into `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/phin-tech/roux/main/scripts/install-cli.sh | sh
```

Or install with Homebrew:

```sh
brew install phin-tech/tap/roux
```

To install the latest prerelease CLI with Homebrew:

```sh
brew install phin-tech/tap/roux-pre
```

To install a specific release, pass the tag:

```sh
curl -fsSL https://raw.githubusercontent.com/phin-tech/roux/main/scripts/install-cli.sh | ROUX_VERSION=v0.5.3 sh
```

Published CLI assets are named by target triple, for example `roux-aarch64-apple-darwin.tar.gz`, `roux-x86_64-apple-darwin.tar.gz`, and `roux-x86_64-unknown-linux-gnu.tar.gz`, each with a matching `.sha256` file.

## Windows

Roux also supports native Windows x64 local builds. For now, Windows is a source-first install path rather than a signed public installer flow:

1. Set up a Windows x64 machine with Claude Code, Git, Node.js, Rust MSVC, and Go Task.
2. Follow [Windows build](windows-build.md).
3. Run `task windows:build` to produce `src-tauri\target\release\bundle\nsis\Roux_<version>_x64-setup.exe`.

Current Windows limitations are documented on [Windows build](windows-build.md). In particular, the first Windows milestone does not include auto-update.

## Updating

Roux has a built-in auto-updater. When a new version is published, Roux checks for it silently on launch and shows a small banner offering to install it. You can also check manually at any time:

- **Settings** (++cmd+","++) → **Advanced** → **Check for updates**
- **Command palette** (++cmd+k++) → **Check for Updates**
- Native app menu (**Roux/File** → **Check for Updates…**, depending on platform)

You can also switch between **Stable** and **Pre-release (Alpha)** in **Settings** → **Advanced**. The prerelease channel follows the newest published prerelease build; switching back to Stable takes effect on the next stable release at or above your current version.

Docs for the current prerelease channel are published at [phin-tech.github.io/roux/pre/](https://phin-tech.github.io/roux/pre/).

Updates are signed and verified on device before they're installed.

After you click **Install and restart**, Roux downloads the new version, verifies the signature, and replaces the app bundle in place. In most cases it will then relaunch itself into the new version automatically. Occasionally — on macOS, after the bundle has just been swapped on disk — the automatic relaunch fails. When that happens the banner changes to **"Update installed. Quit and reopen Roux to finish."** with a **Quit Roux** button. Click it (or quit the app yourself) and reopen Roux from Launchpad or Spotlight — you'll be on the new version.

!!! note "First update from 0.2.x"
    Roux 0.2.x builds were shipped before the auto-updater existed. If you are on 0.2.x, the first updater-enabled release (0.3.0) still requires a one-time manual install — download the latest `.dmg` from the [releases page](https://github.com/phin-tech/roux/releases) and drag the new `Roux.app` into `/Applications`. Every update after that will install itself.

## Uninstalling

Drag `Roux.app` from `/Applications` to the Trash. Per-user settings and session state live under `~/Library/Application Support/roux/` and can be removed separately.
