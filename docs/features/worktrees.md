# Worktrees

Roux has first-class support for [git worktrees](https://git-scm.com/docs/git-worktree) so you can run multiple Claude Code sessions against the same repository without them stepping on each other.

## Why worktrees?

Claude Code edits files in place. Running two sessions in the same working copy means they can clobber each other's changes. A worktree gives each session its own checkout, backed by the same underlying git repo and history.

## Creating a worktree

When creating a new session you can choose:

- **Main working copy** — the session runs directly in the repository's primary checkout.
- **Existing worktree** — attach the session to a worktree you already have on disk.
- **New worktree** — Roux creates a new worktree for you at a path you choose, branching from a ref you pick.

The New Session dialog also supports pasting a GitHub PR URL. Roux can resolve the PR refs and create (or reuse) an appropriate local worktree for review.

## Spawning a worktree from an existing session

Any git-backed session can spawn a sibling worktree that branches from a chosen starting point. This is the fastest way to start a new feature branch without leaving the app.

### From the session context menu

Right-click a session in the sidebar and hover **New Worktree**. A flyout appears with three starting points:

- **Current branch** — branch from the session's current `HEAD`.
- **main** — branch from the local `main` branch.
- **origin/main** — Roux runs `git fetch origin` first, then branches from the remote `origin/main` (useful when your local `main` may be stale).

Pick one, type the new branch name, and hit ++enter++. Roux creates the worktree, spins up a Claude Code session in it, and attaches the sidebar.

Clicking **New Worktree** directly (without hovering) uses your configured default — see [Default starting point](#default-starting-point) below.

### From the command palette

Open the palette with ++cmd+k++ and run **New Worktree**. The palette drills into the same three starting points, then prompts for a new branch name.

### Starting-point resolution

- Roux passes the starting point literally to `git worktree add -b <branch> <path> <start_point>`. That means **main**-named repos work out of the box; repos where the default is `master` (or anything else) will surface a clean `Invalid start point` error rather than a raw git stderr.
- If you type a branch name that already exists, Roux ignores the chosen starting point and checks out the existing branch into the worktree instead (git's own semantics — a branch can only point at one commit).
- **origin/main** fetches `origin` first. If the fetch fails (network, auth, etc.), the session is not created and the error is surfaced in the context menu.

### Default starting point

In **Settings → Sessions**, the **New Worktree default** control picks which starting point is used whenever Roux creates a worktree without an explicit per-invocation base. This covers both:

- Clicking **New Worktree** directly in the session context menu (no hover), and
- Creating a **new worktree session** from the **New Session** dialog (pasting a PR URL or typing a new branch name).

Options:

- **Current branch** (default) — the session's current branch. Matches the original click behavior before the base picker existed, so no muscle-memory breakage.
- **main** — always branch from local `main`. Good if your personal workflow always spins new branches off `main`.
- **origin/main** — always fetch origin and branch from `origin/main`. Good for teams that expect every new feature branch to start from an up-to-date remote.

The context-menu flyout and the command palette always expose all three options regardless of this setting — it only controls the default used when you don't pick explicitly.

> Existing branches (whether local or a PR head ref that's already been fetched) ignore this setting. Git only lets a branch point at one commit, so Roux checks out the existing branch and leaves its history alone.

## Worktree base path templates

In **Settings → Sessions**, `Worktree base path` supports template variables:

- `{project_dir}`
- `{git_root}`
- `{project_name}`
- `{home}`

Example:

```text
{project_dir}/.worktrees
```

Roux previews the resolved path in Settings as you edit the template.

## Cleanup

Use **Settings → Sessions → On session close** to choose worktree cleanup behavior:

- **Keep** — never remove worktrees automatically
- **Ask** — prompt each time a worktree-backed session closes
- **Remove** — always remove the worktree on close

Session closure and worktree cleanup are now separate concepts:

- Closing a session archives the session into **Sessions History**.
- If the worktree is kept on disk, that history row can be restored later.
- If you later choose **Clean worktree** from Sessions History, the history row stays but Restore becomes unavailable because the checkout is gone.

> When the [Worktrunk integration](#worktrunk-integration) is active and a worktree is locked (via `git worktree lock` or `wt`'s own lock semantics), Roux refuses to remove it even in `Remove` cleanup mode. You'll see a clear error banner instead of a silent force-delete. Unlock the worktree first, or use the command palette for the tri-state escape hatch.

## Worktrunk integration

Roux integrates optionally with [worktrunk](https://worktrunk.dev) — a dedicated CLI for managing git worktrees built for parallel AI-agent workflows. When `wt` is installed, Roux uses it as an orchestration layer on top of git's native worktree commands so you get:

- Richer listing metadata in the New Session picker and on session cards (dirty state, ahead/behind, locked/prunable, current/previous, dev-server URLs)
- Optional per-repo `.config/wt.toml` that drives worktree paths, post-start hooks (`npm ci`, `npm run dev`), dev-server URL templates, and more
- A dedicated sidebar panel for managing worktrees, browsing hook definitions, and inspecting hook / command logs

Users without `wt` installed see no UI changes — the integration is strictly additive. Every code path falls back to the native `git worktree` behavior described above.

### Installing worktrunk

```bash
cargo install worktrunk
```

Roux requires **worktrunk 0.44.0 or newer**. The minimum floor is checked at detection time; older versions are treated as "not detected" and Roux falls back to native git. See [worktrunk.dev](https://worktrunk.dev/) for install options and `.config/wt.toml` documentation.

### Detection

Roux probes for `wt` once at launch and again whenever you change the override path:

1. **Explicit override** — `Settings → Integrations → Worktrunk → Binary path` (mirrors the `gh` binary path setting; useful on macOS where GUI apps inherit a minimal `PATH` that may exclude `/opt/homebrew/bin`).
2. **Login-shell `PATH`** — Roux spawns the user's login shell so shell-managed prefixes (Homebrew, fish, etc.) are visible even when the GUI wasn't launched from a terminal.
3. **Process `PATH`** — last-resort fallback for CLI-dev scenarios.

When detection succeeds, a **Trees** icon appears in the activity rail (second from the bottom). If `wt` isn't installed, the icon is simply absent — no dead affordance.

### Worktree provider setting

In **Settings → Integrations → Worktrunk**, the **Worktree provider** segmented control decides which backend Roux uses when **creating** new worktrees:

- **Auto** (default) — use `wt` when detected; fall back to native `git worktree add` otherwise. Recommended for everyone; users without `wt` see no change.
- **Git** — always use native git, even when `wt` is available. An escape hatch if a worktrunk hook misbehaves.
- **Worktrunk** — prefer `wt`. If `wt` fails mid-create for any reason, Roux still falls back to native git so worktree creation never breaks entirely. The setting expresses a preference, not a veto.

When the effective provider resolves to `wt`, the New Session dialog shows a small **"using wt"** badge in the Worktree / Branch legend so you know hooks and templates will run on create.

### Enriched worktree listing

When `wt` is detected, `wt list --format=json` powers worktree listings throughout the app. Each entry is enriched with:

| Chip                  | Meaning                                                   |
| --------------------- | --------------------------------------------------------- |
| `●` (yellow dot)      | Dirty — uncommitted staged / modified / untracked changes |
| `↑N ↓M`               | Commits ahead / behind the default branch                 |
| `🔒` (red)            | Locked — hover for reason                                 |
| `prunable` (red pill) | Prunable — hover for reason                               |
| `current` (blue pill) | The worktree wt considers current (`wt -` target)         |
| `prev` (muted pill)   | The previously-current worktree                           |
| `url` (blue link)     | Dev-server URL from the project's `[list] url` template   |

These chips render in three places:

- The **New Session** worktree picker (one row per worktree)
- Each active **session card** in the sidebar (when `isWorktree: true`)
- The **Worktrunk** sidebar panel's Worktrees tab (see below)

Without `wt` installed, the underlying data is `null` and none of these chips render — you get the same picker and cards as before.

### Worktrunk sidebar panel

The Trees icon opens a dedicated panel scoped to the active session's repo, with five tabs:

#### Worktrees

Default tab. Lists every worktree in the current repo with the full chip set from above. Each row has:

- **Remove** — removes the worktree on disk, keeps the branch. Confirm dialog spells out exactly what will happen.
- **⋮ → Remove worktree + branch** — removes both, for when you're deliberate about dropping the branch.

Remove is automatically disabled (with an explanatory tooltip) for:

- The **main worktree** — git doesn't allow it
- Any worktree that has an **active Roux session** — close the session first

Locked worktrees raise a `WorktrunkLocked` error inline rather than being force-deleted.

#### Hooks

Reads `wt config show --format=json` and surfaces every defined hook — both user-level (`~/.config/worktrunk/config.toml`) and project-level (`.config/wt.toml`). Each row shows the source badge (user / project), hook name (`post-start`, `pre-merge`, …), and the raw command.

Pipeline-array or object values are JSON-encoded for display so the panel doesn't need to model worktrunk's full config schema.

When no hooks are defined, the empty state links to <https://worktrunk.dev/hook/> so you can learn how to configure them.

#### Command log / Hook output / Diagnostic

Surfaces `wt config state logs --format=json` output. Each entry shows filename, size, and relative timestamp. Clicking a row opens a side-by-side reader that `cat`s the file (capped at 256 KiB so a runaway log doesn't crash the UI).

Hook-output rows additionally show the source (`user` / `project` / `internal`), hook type (`post-start`, etc.), and branch — useful for spotting which hook of which branch wrote which log.

### Version and schema compatibility

- Minimum supported version: **0.44.0**. Below the floor, detection returns `None` and Roux falls back to native git.
- All JSON schema fields are parsed with `#[serde(default)]` and no `deny_unknown_fields`, so a newer `wt` that adds fields won't break Roux.
- On any `wt` failure (spawn, non-zero exit, parse error), the enriched path falls back to native git with a single warning logged.
- Lock errors are the one exception — they propagate to the user instead of silently forcing, because forcing discards data the hook/lock was protecting.

## Caveats

- Worktrees share hooks and git config with the main repo. Be aware of any `post-checkout` or `post-commit` hooks that assume a single working copy.
- Do not `git worktree remove` the directory a running session is using — stop the session first.
- Worktrunk hooks and Roux's own agent hooks are separate systems. Roux does not install, route, or interfere with `wt`'s hook machinery; likewise `wt` has no visibility into Roux sessions. If you configure a `wt` post-start hook that needs the Roux session's environment, pass it explicitly in your hook template.
- [Automation hooks](hooks.md) can wrap Roux's own worktree create/remove operations and receive provider context (`git` vs `worktrunk`) without replacing Worktrunk's hook machinery.
