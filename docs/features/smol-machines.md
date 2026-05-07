# Smol Machines

[smolmachines](https://smolmachines.com/) is a CLI-only local VM sandbox: sub-second cold start, hardware-isolated, configured with a TOML `Smolfile`. On macOS it uses Hypervisor.framework via libkrun; on Linux it uses KVM. Roux integrates with smolvm so a Roux session can declare "I run inside smol machine X" — every PTY spawned for that session executes inside the guest VM instead of on the host.

This is real OS-level isolation: the session's process tree, network namespace, and rootfs are all the VM's. Only the PTY straddles host ↔ guest.

## When you'd use it

- **Untrusted code** — review or test code without exposing your host filesystem.
- **Disposable environments** — spin up a fresh Linux box per session, throw it away when you're done.
- **OS-specific tooling** — run Linux-only binaries from your macOS host.
- **Per-project isolation** — keep each project's tooling installs separate without containers.

## Prerequisites

- Install smolvm and confirm `smolvm --version` works on your `$PATH`. Roux discovers the binary automatically; you can override the path in **Settings → Integrations → Smol Machines**.
- macOS 13+ for libkrun, or a Linux kernel with KVM.
- The integration is invisible when smolvm isn't installed — no sidebar icon, no menu items, no settings noise. Install smolvm later and Roux's detection picks it up.

## The Smol Machines panel

The panel lives in the activity rail (the icon only appears when smolvm is installed). It mirrors `smolvm machine ls` and exposes lifecycle controls.

Each row shows:

- machine name, state (`running` / `stopped` / `starting`), image
- start / stop / delete buttons
- "Open shell" — drops you into a pane attached to that machine via `smolvm machine exec`
- "Assign to active session" — binds the currently active session to this machine
- agent install dropdown (Claude / Codex)

Header controls:

- **+** — open the inline create form
- **refresh** — reload the machine list
- **sliders** — open the smolvm install-script bootstrap config in your editor
- **shield** — start/stop the configured managed HTTP proxy (only shown when configured)

## Creating a machine

Click **+** in the panel header. Fields:

- **Name** (required) — the machine name, used everywhere.
- **Smolfile** (optional) — pick a `Smolfile.toml` from disk. When set, the Smolfile is authoritative; Roux hides the per-field overrides below.
- **Image** — e.g. `alpine:3.19`, `ubuntu:24.04`. Hidden when a Smolfile is provided.
- **Network** — give the guest outbound network access. Hidden when a Smolfile is provided.
- **Forward SSH agent** — forward your host's running SSH agent into the guest so `git clone git@…` works for private repos. Private keys never leave the host — the hypervisor enforces it. Requires `ssh-add -l` to list keys on the host.
- **Host HTTP proxy URL** — route guest HTTP(S) through a host-side proxy. Useful when private registries IP-allowlist your host (AWS CodeArtifact, corporate Artifactory). Roux generates a managed Smolfile that exports `HTTP_PROXY` / `HTTPS_PROXY` in the guest via `/etc/profile.d/roux-proxy.sh` so every login shell picks it up.
- **Mount paths** — multi-row input. Each row is a `host:guest[:ro]` mount. Same-path mounts (host == guest) make `--workdir <host_worktree>` "just work" inside the guest — sessions land in their actual project directory instead of `$HOME`. Read-only is opt-in via the `ro` checkbox.

When you submit with only a proxy URL (no Smolfile), Roux generates a managed Smolfile at `~/.config/roux/smolmachines/<name>.toml` and links it. This is what lets the recreate flow preserve your settings.

## Binding a session to a machine

In the panel, click **Assign to active session** on a machine row. From that point on:

- Every shell or Claude Code spawn for that session runs inside the VM via `smolvm machine exec --name <name> -it -- /bin/sh`.
- A subset of `ROUX_*` env vars (session ID, pane ID, project ID, worktree path, notes paths) is forwarded into the guest as `-e KEY=VAL` flags.
- If the session has a worktree path, Roux passes `--workdir <worktree_path>` so the shell lands in the project directory — provided the path is mounted in the linked Smolfile.

When you bind to a machine whose linked Smolfile doesn't mount the session's worktree, Roux surfaces a yellow banner offering to append a same-path mount. Smolvm bakes volumes at machine create time, so you'll be told to recreate the machine to apply.

To unbind, run `cmd_set_session_smol_machine(id, null)` from the command palette, or use the session card's bound-machine badge.

## Agent install (Claude / Codex)

Fresh smolvm guests are minimal — `claude` and `codex` aren't preinstalled. Roux detects this and gives you two paths from each machine row's install dropdown:

- **Install Claude (Run in VM)** — runs the install script once inside the running guest. Quick and ephemeral; the install survives only as long as the machine isn't recreated.
- **Install Claude (Persist via Smolfile)** — appends the install line to the linked Smolfile's `[dev].init` so it runs on every machine boot. Idempotent. If the machine has no linked Smolfile, Roux offers a confirm modal that generates one and recreates the machine from it.

When a session bound to a VM tries to spawn Claude and the binary isn't on the guest's `$PATH`, Roux short-circuits the profile-replay and writes a multi-line `# claude is not installed…` comment block into the shell instead of letting `/bin/sh: claude: not found` fire. The hint includes the exact install command.

### Distro detection

Roux picks the right install script by prefix-matching the machine's image string:

- `alpine*` → alpine script (uses `apk`)
- `ubuntu*` → ubuntu script (uses `apt-get`)
- everything else → `default` (assumes node + npm already on `$PATH`)

Override any of these in **panel header → sliders icon**, which opens `~/.config/roux/smolvm-bootstraps.toml`. The file contains one bash line per `(agent, distro)` pair.

### Library-sourced install scripts

Install scripts can also live as Library items, layered the same way prompts and skills are. In the [Library](library.md) panel, click **+ Smolvm** to author a new script with `agent` and `distro` frontmatter. The resolver checks library items first, then the bootstrap config TOML, then falls back to the built-in default.

This is how teams share install scripts via a Git-backed library source.

## Managed HTTP proxy

When private registries IP-allowlist your host (AWS CodeArtifact, corporate Artifactory), the VM can't reach them directly because the registry trusts the *host's* IP. Roux solves this by orchestrating a host-side HTTP proxy and injecting the URL into the guest as env.

Roux ships no proxy code — you install whatever you want (tinyproxy, mitmproxy, squid, anything) and tell Roux how to invoke it. Configure in **Settings → Integrations → Smol Machines → Managed HTTP proxy**:

- **Command** — `tinyproxy -d -c ~/.config/roux/tinyproxy.conf` or `mitmdump --mode regular --listen-port 8888` or your own.
- **Port** — what port the proxy listens on. Roux polls this to verify the proxy started.
- **Bind address** — defaults to `127.0.0.1`.

Once configured, the panel header shows a Shield icon. Click to start the proxy; click again to stop. Roux:

- Spawns the command via `sh -lc` so PATH and aliases work.
- Polls the listen socket up to 5s to verify it bound; bubbles up the proxy's stderr on failure.
- SIGTERMs (then SIGKILLs after 2s) on stop and on app quit so processes don't leak.

When the proxy is running, the create form's **Host HTTP proxy URL** field auto-fills with `http://<bind>:<port>`. You can override.

## Worktree mounts

By default, sessions inside a smol machine land in the guest's `$HOME` because the host worktree path doesn't exist inside the guest. Mount the worktree (host == guest path) to fix this — Roux will pass `--workdir <worktree>` so sessions land in their project directory.

Two ways to mount:

- **At create time** — fill in the create form's "Mount paths" rows.
- **After binding** — when you assign a session to a machine and Roux detects the worktree isn't mounted, the panel surfaces a "Worktree not mounted" banner. Click **Add mount** to append a same-path entry to the linked Smolfile's `[dev].volumes`. Recreate the machine to apply (smolvm bakes volumes at create time, not start time).

## How Roux talks to smolvm

Everything routes through the `smolvm` CLI: `machine ls --json`, `machine create`, `machine start`, `machine stop`, `machine delete`, `machine exec --name <n> -it [...] --`. There's no parallel registry — Roux is a thin UI over the CLI. If smolvm changes the CLI, Roux follows.

The Smolfile registry at `~/.config/roux/smolmachines.json` records which machines have a Roux-managed Smolfile so the persist and recreate flows know whether to append in place or generate fresh.

## Limitations and known gaps

- **Mid-session VM stop** — if you stop the VM while a pane is attached, the pane's exit-code banner fires but doesn't distinguish "VM stopped" from "shell exited normally." Reattach to recover.
- **Mount changes need recreate** — smolvm bakes volumes at create time. Adding a mount via the auto-mount banner appends to the Smolfile, but the running machine doesn't pick it up until you recreate it.
- **Smolfile editor** — Roux doesn't bundle a Smolfile editor. The bootstrap config and managed Smolfiles open in your `$EDITOR`.
- **Multi-machine sessions** — a session can bind to one machine. Splitting a session across two machines isn't supported.
- **Cloud / remote smolvm** — smolvm is local-only by design. Roux doesn't try to extend it.
