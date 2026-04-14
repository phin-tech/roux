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
