# macOS signing, notarization, and stapling

This repo now uses Tauri's built-in macOS signing flow instead of re-signing the app bundle after `tauri build`.

## What you need from Apple

- A paid Apple Developer account. Free accounts can sign for local testing but cannot notarize.
- A `Developer ID Application` certificate if you want to distribute the app outside the Mac App Store.
- Either:
  - Apple ID notarization credentials: `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`
  - App Store Connect API credentials: `APPLE_API_KEY`, `APPLE_API_ISSUER`, and the private key contents

## What lives in 1Password

Store these as fields or file attachments in 1Password:

- `APPLE_CERTIFICATE`: base64-encoded exported `.p12`
- `APPLE_CERTIFICATE_PASSWORD`: password used when exporting the `.p12`
- `APPLE_SIGNING_IDENTITY`: optional, only if Tauri cannot infer it from the certificate
- `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` for Apple ID notarization
- or `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_CONTENT` for API-key notarization

`APPLE_API_KEY_CONTENT` should be the contents of the downloaded `AuthKey_*.p8` file. The signing wrapper writes it to a temporary file and exports `APPLE_API_KEY_PATH` for Tauri.

## Local setup

1. Copy `.env.signing.example` to `.env.signing`.
2. Replace each placeholder value with your own `op://...` secret reference.
3. Sign in to 1Password CLI: `op signin`

## Commands

Build, sign, notarize, and staple with `op run`:

```sh
op run --env-file=.env.signing -- task sign
```

Equivalent convenience task:

```sh
task sign:op
```

If you want to build a specific target triple, pass it through `TAURI_BUILD_ARGS` and `TAURI_TARGET`:

```sh
TAURI_TARGET=aarch64-apple-darwin \
TAURI_BUILD_ARGS="--target aarch64-apple-darwin" \
op run --env-file=.env.signing -- task sign
```

## What the task does

- Builds `roux-cli`
- Bundles it into `Roux.app/Contents/MacOS/roux-cli` before signing
- Runs `tauri build`, which signs the app and submits notarization when the Apple env vars are present
- Lets Tauri wait for notarization and staple the ticket unless you explicitly pass `--skip-stapling`
- Verifies the finished `.app` signature and validates the stapled `.dmg`
