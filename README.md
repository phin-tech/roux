# Roux

A multi-session Claude Code terminal manager built with Tauri and Svelte.

## Development

```bash
npm install
task dev
```

## Release

Version bump, tag, and push:

```bash
task version BUMP=patch
task version BUMP=minor
task version BUMP=major
```

Build, sign, notarize, and staple on a signing Mac:

```bash
op run --env-file=.env.signing -- task sign
```

Create or update the GitHub release for the current version and upload the signed artifacts:

```bash
task publish
```

Or do signing and publishing together:

```bash
task publish:op
```

See [docs/macos-signing.md](docs/macos-signing.md) for macOS signing and notarization setup.
