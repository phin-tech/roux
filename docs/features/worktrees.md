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

In **Settings → Sessions**, the **New Worktree default** control picks which of the three starting points is used when you click **New Worktree** directly (as opposed to hovering to expose the flyout):

- **Current** (default) — the session's current branch. Matches the original click behavior before the base picker existed, so no muscle-memory breakage.
- **main** — always branch from local `main`. Good if your personal workflow always spins new branches off `main`.
- **origin/main** — always fetch origin and branch from `origin/main`. Good for teams that expect every new feature branch to start from an up-to-date remote.

The flyout and command palette always expose all three options regardless of this setting — it only controls the click-without-hover default.

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

## Caveats

- Worktrees share hooks and git config with the main repo. Be aware of any `post-checkout` or `post-commit` hooks that assume a single working copy.
- Do not `git worktree remove` the directory a running session is using — stop the session first.
