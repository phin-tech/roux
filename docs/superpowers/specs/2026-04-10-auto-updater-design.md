# Roux auto-updater — design

Status: approved 2026-04-10
Owner: Sam

## Goal

Ship a Tauri v2 updater integration for Roux so that users on macOS get signed,
verified in-place updates from GitHub releases without having to download a
`.dmg` and drag it into `/Applications` themselves.

## Non-goals

- Windows or Linux updater support. Roux is macOS-only today.
- Multi-arch support beyond the host arch of the signing Mac. Only
  `darwin-aarch64` is produced initially; adding `darwin-x86_64` or
  `darwin-universal` later is a small, purely additive change.
- Delta/patch updates. Full `.app.tar.gz` downloads only.
- Background download while the user is unaware. Downloads only start after
  explicit user confirmation.
- Staged rollouts, A/B updates, channels beyond "latest".

## Scope

End-to-end: keypair + secrets management, build-time signing, manifest
generation and publication, in-app check/download/install flow, and the
Settings UI surface to drive it.

## 1. Keys and secrets

A dedicated Tauri updater keypair is generated once on the signing Mac with:

```
npm run tauri -- signer generate -w ~/.tauri/roux.key
```

This produces `~/.tauri/roux.key` (password-encrypted Ed25519 private key) and
`~/.tauri/roux.key.pub` (public key).

The **public key** is committed to `src-tauri/tauri.conf.json` under
`plugins.updater.pubkey` as its literal contents.

The **private key and its password** are stored in 1Password in vault
`owkafiordkt5yio7nt6rpjvzma`, item `Roux Updater`, with fields:

- `private_key` — the entire contents of `~/.tauri/roux.key`, not a path
- `password` — the password entered at generation time

`.env.signing.example` and `.env.signing` gain two new entries:

```
TAURI_SIGNING_PRIVATE_KEY=op://owkafiordkt5yio7nt6rpjvzma/Roux Updater/private_key
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=op://owkafiordkt5yio7nt6rpjvzma/Roux Updater/password
```

`op run --env-file=.env.signing -- task sign` resolves these at build time
exactly like the existing Apple Developer ID secrets. `task dev` never touches
them, so local development is unaffected.

### Key rotation risk

If the private key is lost, the only remediation is to generate a new keypair,
ship the new public key in a new release, and have every existing user install
that release manually — because their installed clients will reject updates
signed with a different key. This is the same blast radius as losing the Apple
Developer ID signing identity and is the primary reason for keeping the key in
1Password next to the other signing secrets.

## 2. Tauri configuration

### `src-tauri/Cargo.toml`

Add `tauri-plugin-updater` as a desktop-only dependency:

```toml
[target.'cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))'.dependencies]
tauri-plugin-updater = "2"
```

(Matching the Tauri 2 major version already in the workspace.)

### `src-tauri/src/main.rs` (or lib)

Register the plugin in the builder under `#[cfg(desktop)]`:

```rust
#[cfg(desktop)]
let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
```

Applied alongside the existing plugin registrations in `main.rs`, not in a new
module.

### `src-tauri/tauri.conf.json`

Two additions:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true,
    ...
  },
  "plugins": {
    "updater": {
      "pubkey": "<contents of ~/.tauri/roux.key.pub>",
      "endpoints": [
        "https://github.com/phin-tech/roux/releases/latest/download/latest.json"
      ]
    }
  }
}
```

Setting `createUpdaterArtifacts: true` causes `tauri build` to emit a
`.app.tar.gz` and matching `.app.tar.gz.sig` alongside the existing `.app` and
`.dmg`. The exact filename pattern (likely
`Roux_${VERSION}_aarch64.app.tar.gz` or simply `Roux.app.tar.gz`) is verified
during implementation step 2 by inspecting the bundle output directory; the
publish task then pattern-matches on that name the same way it already
pattern-matches DMGs.

### Capabilities

The app's default capability file (`src-tauri/capabilities/default.json` or the
equivalent) gains `updater:default` in the `permissions` array.

## 3. Build-time signing

`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are the
only required build-time env vars. They are already resolved by
`op run --env-file=.env.signing`. No changes to `scripts/run-macos-signing.sh`
or to the existing `task sign` pipeline beyond making sure the new vars are
present in `.env.signing.example` so a fresh checkout documents what is needed.

A preflight check in `task sign` (or `task sign:op`) verifies both env vars are
set and fails fast with a helpful message if they are not.

## 4. Manifest generation and publication

### Endpoint strategy

The updater points at a single static URL:

```
https://github.com/phin-tech/roux/releases/latest/download/latest.json
```

GitHub auto-redirects `releases/latest/download/<name>` to the asset with that
name on the newest non-prerelease release. Uploading a `latest.json` asset to
every non-prerelease release is all that is required.

### Manifest shape

```json
{
  "version": "0.3.0",
  "notes": "- fix: ...\n- feat: ...",
  "pub_date": "2026-04-10T18:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<contents of Roux.app.tar.gz.sig>",
      "url": "https://github.com/phin-tech/roux/releases/download/v0.3.0/Roux.app.tar.gz"
    }
  }
}
```

Only one platform entry today. Adding more is a pure data change.

### Folding into `task publish`

`Taskfile.yml`'s existing `publish` task is extended to, after creating or
updating the GitHub release:

1. Compute the version, tag, and changelog exactly as it does today.
2. Skip manifest generation entirely when the version is a prerelease (contains
   a `-`). Prereleases still upload `.app.zip` and `.dmg` as before.
3. For normal releases:
   - Locate the `.app.tar.gz` and matching `.sig` in the bundle directory by
     `find`-pattern (same approach as the existing DMG upload loop).
   - Read the contents of the `.sig` file.
   - Generate `latest.json` into the bundle directory using the same version,
     changelog, and a `pub_date` of `date -u +%Y-%m-%dT%H:%M:%SZ`. The `url`
     field in the manifest points to the exact asset name that will be uploaded
     to the release.
   - `gh release upload "$TAG" <tarball> <sig> latest.json --clobber`.

The generator is inline shell inside the task (consistent with the existing
`publish` body), not a separate script, because it is short and has no reuse
elsewhere.

### First-release caveat

Version `0.2.4` and earlier have no updater plugin. Users on those versions
cannot auto-update into the first updater-enabled release (call it `0.3.0`).
They must install that release manually the old way. This must be called out in:

- the `0.3.0` release notes
- `docs/install.md`

After `0.3.0`, every subsequent release is delivered in-app.

## 5. Runtime behavior

### Frontend module: `src/lib/updater.ts`

A single module wraps the `@tauri-apps/plugin-updater` API and is the only
place the rest of the frontend talks to the updater.

```ts
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "no-update" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; progress: number | null }
  | { kind: "ready" }
  | { kind: "error"; reason: UpdaterError };

export type UpdaterError =
  | "network"
  | "signature-invalid"
  | "unknown";

export async function checkForUpdate(opts: { silent: boolean }): Promise<UpdateStatus>;
export async function installUpdate(onProgress: (p: number | null) => void): Promise<void>;
```

- In dev (`import.meta.env.DEV`), `checkForUpdate` returns `{ kind: "no-update" }`
  without touching the plugin.
- Network failures in `silent: true` mode log and return `{ kind: "no-update" }`
  so the user is never interrupted by transient connectivity issues. In
  `silent: false` mode they return `{ kind: "error", reason: "network" }` so the
  Settings UI can surface the failure.
- Signature failures are never silent. They always surface as `signature-invalid`
  — this is a tamper signal and must be visible.
- `installUpdate` calls `downloadAndInstall` and then `relaunch` from
  `@tauri-apps/plugin-process`.

### Settings store: `updateCheckOnLaunch`

A new boolean preference, default `true`, persisted next to the other user
settings. Exposed through the existing settings store so the Settings UI can
bind a toggle to it.

### Startup check

In the top-level app component, after the window is shown and session restore
has begun:

1. Wait 5 seconds (debounce, avoids competing with session restore work).
2. If `updateCheckOnLaunch` is `false`, do nothing.
3. Otherwise call `checkForUpdate({ silent: true })`.
4. If the result is `{ kind: "available", ... }`, show a non-blocking toast or
   banner with two actions: **Install and restart** and **Later**.
5. **Later** dismisses the toast for this session only. The toast re-appears on
   the next launch if the user still has not updated.
6. **Install and restart** transitions into the download flow described below.

### Settings UI

A new **Updates** section in Settings containing:

- Current version (from `getVersion()` in `@tauri-apps/api/app`)
- **Check for updates** button
- Inline status, driven by an `UpdateStatus` value local to this section
- **Check for updates on launch** toggle, bound to `updateCheckOnLaunch`

When the user clicks **Check for updates**, the section calls
`checkForUpdate({ silent: false })` and renders the returned status inline:

- `checking` → spinner + "Checking…"
- `no-update` → "You're on the latest version."
- `available` → "Update available: 0.3.0" with an **Install and restart** button
  and a collapsible release notes section
- `downloading` → progress bar (determinate if `progress` is not null,
  indeterminate otherwise)
- `error: network` → "Check failed — couldn't reach the update server. Retry?"
- `error: signature-invalid` → "Update signature invalid. Download blocked.
  Please report this." with a link to the issue tracker.

### Command palette entry

A new command **Check for updates** registered in `src/lib/commands/index.ts`
that calls the same code path as the Settings button. Available regardless of
the auto-check toggle.

### Download flow

Regardless of trigger (toast, Settings button, command palette), the flow is:

1. Call `installUpdate(onProgress)`.
2. `onProgress` updates the visible status to show a progress indicator.
3. On success, `relaunch()` is called and the app exits + restarts into the new
   version. No explicit "done" state is needed since the current process is
   replaced.
4. On failure, the error is surfaced the same way as a failed manual check.

## 6. Documentation changes

- **`docs/install.md`** — replace the current "Updating" section with a
  description of the in-app updater: auto-check on launch, Settings button, and
  the one-time manual install for the first updater-enabled release.
- **`docs/settings.md`** — add a short "Updates" subsection documenting the
  manual check, the current version display, and the auto-check toggle.
- No entry in the published docs site for key rotation or release runbook; that
  lives in internal notes (see below).

### Internal release notes

A short note in `docs/plans/2026-04-10-auto-updater.md` (excluded from the
mkdocs build via `exclude_docs`) covering:

- How the manifest is generated inside `task publish`
- How to rotate the updater keypair if compromised
- What the first-release caveat looks like from a user's perspective

## 7. Testing strategy

### Frontend unit tests

`src/lib/__tests__/updater.test.ts`, using Vitest and a mock of
`@tauri-apps/plugin-updater`. Coverage:

- `checkForUpdate` returns `no-update` when the plugin reports no update.
- `checkForUpdate` returns `available` with version + notes on success.
- `checkForUpdate({ silent: true })` swallows network errors and returns
  `no-update`.
- `checkForUpdate({ silent: false })` surfaces network errors as
  `error: network`.
- Signature errors always surface as `error: signature-invalid`, even in silent
  mode.
- Dev-mode short-circuit: `checkForUpdate` never calls the plugin when
  `import.meta.env.DEV` is true.

### Manifest generator test

A small shell-level test inside `Taskfile.yml` or a separate `bats` / Python
unit test that, given a fake `.sig` file and a known version, asserts the
generated `latest.json` contains the expected fields. This is worth automating
because a typo in the manifest silently breaks updates for every user.

### No Rust tests

The plugin is first-party Tauri code. Wrapping it in Rust unit tests adds
maintenance burden without meaningful coverage.

### Manual verification

The following cannot be automated without publishing real releases and must be
done once by hand before announcing the updater:

1. Generate a throwaway pre-release build on a branch with a bumped version.
2. Install an older build locally, run it, and confirm the updater shows the
   new version, downloads it, verifies the signature, and restarts into the new
   app.
3. Tamper with a `.sig` asset on a test release and confirm the signature path
   fails loudly in the UI rather than silently.

## 8. Rollout order

This is the intended merge sequence. Each step is independently testable.

1. **Keypair + secrets.** Generate the keypair, stash in 1Password, update
   `.env.signing.example`. No code changes yet; verifies the 1Password side is
   wired correctly.
2. **Plugin registration + config.** Add `tauri-plugin-updater`, register it in
   `main.rs`, add `plugins.updater` and `createUpdaterArtifacts: true` to
   `tauri.conf.json`, add `updater:default` to capabilities. `task sign`
   should now produce `Roux.app.tar.gz` + `.sig`. No UI yet.
3. **Frontend module + unit tests.** Write `src/lib/updater.ts` and its tests.
   Nothing calls it yet.
4. **`task publish` manifest generation.** Fold `latest.json` generation and
   upload into `publish`. Test with `--dry-run` or by publishing to a
   throwaway prerelease tag.
5. **Settings UI + command palette entry + startup check.** Wire the runtime
   behavior. This is the point at which end users see anything new.
6. **Docs updates.** `docs/install.md` and `docs/settings.md`.
7. **Cut `0.3.0`.** This is the first updater-enabled release. Release notes
   call out the manual install caveat.
8. **Cut `0.3.1`.** Exercises the real update path end-to-end against the
   `0.3.0` clients.

## 9. Open questions

None blocking. Possible follow-ups after shipping:

- Add `darwin-x86_64` and/or `darwin-universal` builds and manifest entries.
- Optional "Install on next launch" mode (download now, install on relaunch).
- Update channels (beta / stable) if we ever want to ship prereleases to
  opted-in users.
