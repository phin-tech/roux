# Auto PR Watches In GitHub Settings

## Summary

- Add a GitHub settings section that can automatically create and maintain PR watches for the PR attached to each session's current branch.
- Use a hybrid approach: `gh` for branch-to-PR discovery, then keep Roux's existing Rust watch pipeline and octocrab-based PR detail polling for checks, reviews, and outcome.
- Treat auto-discovered PRs as persisted, auto-managed watches. They appear in the existing Watches UI and drive the existing session watch indicators.
- Tagging rule for v1: auto PR watches belong to the session that owns the `worktreePath`. If one agent is working across multiple folders or repos, that should be modeled as multiple sessions.

## Interface Changes

- Add `github` settings to `RouxSettings`:
  - `autoWatchCurrentPrs: boolean` default `false`
  - `autoClosePrWatches: boolean` default `true`
  - `currentPrPollIntervalSecs: number` default `60`
- Extend `SetupStatus` with `ghAuthenticated: boolean` so the GitHub settings section can show installed and authenticated state separately.
- Extend `Watch` with `managedBy: "user" | "autoCurrentPr"`.
- Keep `WatchKind::GithubPr` unchanged. Auto PR watches are regular PR watches with different ownership and lifecycle metadata, not a new watch kind.
- Keep `CreateWatchConfig` user-only. Auto-managed PR watches are created internally by the backend reconciler, not via the existing manual watch command.

## Implementation Changes

- Add a backend GitHub auto-watch reconciler that runs on app startup, session restore or create or close, GitHub settings changes, and on a fixed interval.
- For each git-backed session, the reconciler runs `gh pr view --json number,title,url,state,isDraft,headRefName,headRepository,headRepositoryOwner` in `session.worktreePath` with a hard timeout.
- If `gh pr view` reports no PR for the current branch, remove any existing auto-managed PR watch for that session.
- If a PR exists, upsert one persisted session-scoped auto-managed `githubPr` watch keyed by session ownership. Do not dedupe across sessions in v1.
- Reuse the existing `execute_github_pr_check` path to populate reviews, checks, and outcome after discovery. `gh` is only the discovery layer.
- When an auto-managed PR watch reaches `merged` or `closed`, emit the normal notification, then remove it from the watch store if `autoClosePrWatches` is enabled. Manual PR watches keep the current stop-only behavior.
- In the Settings panel, add a GitHub section with:
  - installed and authenticated status
  - `Auto-watch current PRs`
  - `Auto-close PR watches when merged/closed`
  - `Refresh interval`
- In the Watches pane, label auto-managed PR watches as `auto` and hide or remove per-watch pause or remove controls for them; the settings section owns their lifecycle.
- In session UI, keep using the existing session watch indicators. Because auto PR watches are session-scoped, they will show up through the current session watch filtering without adding a second session-status data path.
- Do not add `roux register pr` or a new `roux watch X` CLI or socket command in v1. Manual PR watching already exists; the new value is the automatic GitHub setting.

## Test Plan

- Rust unit tests for `gh` discovery parsing:
  - PR found
  - no PR on branch
  - `gh` missing or unauthenticated
  - timeout or malformed JSON
- Rust reconciler tests:
  - creates auto-managed PR watch on discovery
  - updates the same watch when PR metadata changes
  - removes the watch when the branch no longer has a PR
  - cleans up on session close
  - auto-closes and removes on merged or closed PR when enabled
- Watch store tests:
  - `managedBy` persists through `watches.json`
  - persisted auto-managed watches reload cleanly and resync on startup
- Frontend tests:
  - new GitHub settings defaults and rendering
  - Watches pane labeling and controls for auto-managed PR watches
  - session watch indicators reflect auto PR watches
- Verification commands:
  - `npm run test`
  - `npm run check`
  - targeted `cargo test` for watch store, reconciler, and GitHub discovery modules

## Assumptions

- "Open watches for any PR" means: for every session whose current branch has a PR, Roux should automatically create and maintain a PR watch.
- Multi-folder work is not inferred from arbitrary directory changes inside one session in v1; session or worktree ownership is the authoritative tag.
- `gh` is the right discovery mechanism because its current-branch behavior matches the need directly; octocrab remains the right detail-fetch mechanism once repo and PR number are known.
- Prior art and references:
  - CMUX PR 2453: <https://github.com/manaflow-ai/cmux/pull/2453>
  - GitHub CLI `gh pr view`: <https://cli.github.com/manual/gh_pr_view>
  - GitHub REST pull requests docs: <https://docs.github.com/en/rest/pulls/pulls>
