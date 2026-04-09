# Windows build

Roux supports native Windows x64 local builds with an unsigned per-user NSIS installer.

## Prerequisites

- Windows x64.
- Native Claude Code CLI installed for Windows and available as `claude` or `claude.exe`.
- Git for Windows.
- Node.js/npm.
- Rust with the `x86_64-pc-windows-msvc` toolchain.
- Go Task.
- Optional: PowerShell 7 (`pwsh`). Roux falls back to Windows PowerShell and then `cmd.exe`.

WSL session mode, ARM64 Windows, auto-update, code signing, and Windows `nono` profile support are not part of the first Windows milestone.

## Development

Install dependencies and run the app:

```powershell
npm install
task dev
```

Run the full local test gate:

```powershell
task test
```

The Taskfile stages the internal `roux-cli.exe` sidecar before Tauri dev/build runs.

## Installer

Build the unsigned NSIS installer:

```powershell
task windows:build
```

The installer is written under:

```text
src-tauri\target\release\bundle\nsis\
```

The generated artifact is named with the app version and target architecture:

```text
src-tauri\target\release\bundle\nsis\Roux_<version>_x64-setup.exe
```

The NSIS installer uses Tauri's `currentUser` install mode, so it installs per user and does not require machine-wide installation by default.

## Runtime Notes

- `roux-cli.exe` is bundled internally and is not added to the user's `PATH`.
- Claude hooks are installed by the app using an absolute quoted path to the bundled `roux-cli.exe`.
- Uninstall leaves the user's Claude settings alone in v1.
- Native Windows Claude is required. Roux does not fall back to WSL Claude in v1.
- `nono` is checked opportunistically, but Windows `nono` profile support is deferred until a supported `nono.exe` flow is validated.
