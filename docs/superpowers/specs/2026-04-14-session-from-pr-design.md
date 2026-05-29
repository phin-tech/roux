# Session from PR URL — Design

## Goal

Let the user paste a GitHub pull request URL into the New Session dialog and have
Roux create a session whose worktree is checked out at the PR's head branch. The
feature should work for same-repo PRs and fork PRs, fail gracefully when `gh` is
not installed, and never mutate the main repository's HEAD.

## Non-goals

- No GitHub API / token management in Roux itself. We defer to the `gh` CLI for
  auth.
- No PR review UI, no comment surfacing, no CI status. Just branch checkout.
- No support for GitLab, Bitbucket, or other forges in this iteration.

## UX

A new optional text input titled **"PR URL (optional)"** is added at the top of
`NewSessionDialog`, above the existing repo/branch section. The input is
rendered only when the `gh` CLI is detected at dialog-open time, with no
blocking modal and no first-run wizard.

The input accepts:

- Full URLs: `https://github.com/owner/repo/pull/142`
- Shortforms: `owner/repo#142`

On paste or blur, the dialog performs a PR lookup. A status line below the
input reflects progress:

- `idle` → no status line.
- `loading` → `Fetching PR…` with a spinner glyph.
- `ok` → `PR #142: "Fix login redirect"` with a small badge: `same-repo` or
  `fork: octocat:feature-branch`.
- `error` → inline red error message (see Error Handling).

When lookup succeeds, the dialog pre-fills:

- `sessionName` with `pr-<NNN>-<slug>` where `<slug>` is the PR title
  lowercased, non-alphanum characters replaced with `-`, runs of `-` collapsed,
  trimmed, truncated to ~30 characters. Final session name capped at ~40.
- `worktreeFilterInput` with the resolved local branch name (see "Branch
  resolution" below).

The user can still edit either field before clicking Create. The existing
`resolveGitTarget()` flow takes over from there: if a worktree already exists
for the branch, it's selected; otherwise the "unknown branch" path creates a
new worktree on Create.

## Architecture

```
User pastes URL
      │
      ▼
Frontend: debounce 250ms
      │
      ▼
tauri: lookup_pr(repoPath, url)
      │
      ▼
Rust: shell out to `gh pr view <ref> --json ...`
      │
      ▼
Return PrInfo { number, title, head_ref, head_owner, is_cross_repository }
      │
      ├── same-repo? → pre-fill branch = head_ref
      │
      └── fork?      → tauri: fetch_pr_branch(repoPath, pr_number)
                         │
                         ▼
                    Rust: `git fetch <fork-url> <head_ref>:pr-<NNN>`
                         (no HEAD movement)
                         │
                         ▼
                    Pre-fill branch = `pr-<NNN>`
```

On Create, `create_session_shell` runs with the pre-filled branch. No changes
to the session-creation path itself.

## Components

### Frontend: `src/lib/components/NewSessionDialog.svelte`

New state:

```ts
let prUrl = $state("");
let prLookup = $state<"idle" | "loading" | "ok" | "error">("idle");
let prInfo = $state<PrInfo | null>(null);
let prError = $state<string>("");
let ghInstalled = $state(false);
```

- `$effect` on dialog visibility: call `checkGhInstalled()` once per open.
- `$effect` on `prUrl` (debounced ~250ms): if `ghInstalled` and `prUrl` parses
  as a PR ref, call `lookupPr(repoPath, prUrl)`.
- On successful lookup with `is_cross_repository === true`, immediately call
  `fetchPrBranch(repoPath, prInfo.number)` and use the returned branch name.
- On successful lookup + branch resolution, set `sessionName` and
  `worktreeFilterInput`. Do not overwrite if the user has already typed into
  those fields since the last lookup (track a "user has edited" flag per field).

### Frontend: `src/lib/tauri.ts`

New bindings:

```ts
export async function checkGhInstalled(): Promise<boolean>;
export async function lookupPr(repoPath: string, url: string): Promise<PrInfo>;
export async function fetchPrBranch(repoPath: string, prNumber: number): Promise<string>;
```

Where `PrInfo`:

```ts
export interface PrInfo {
  number: number;
  title: string;
  headRef: string;
  headOwner: string;
  isCrossRepository: boolean;
}
```

### Backend: new module `src-tauri/src/pr.rs`

Exposes:

- `parse_pr_ref(input: &str) -> Option<PrRef>` — pure parser, unit tested.
  Accepts both URL and shortform. Returns `PrRef { owner, repo, number }`.
- `check_gh_installed() -> bool` — uses `which::which("gh")`.
- `lookup_pr(repo_path: &str, input: &str) -> anyhow::Result<PrInfo>` — shells
  out to `gh pr view <ref> --json number,title,headRefName,headRepositoryOwner,isCrossRepository,headRepository`
  with `--repo owner/repo` when the URL embeds it. Runs in `repo_path` so gh's
  auth context matches the user's normal workflow.
- `fetch_pr_branch(repo_path: &str, pr_number: u32) -> anyhow::Result<String>` —
  runs `gh pr view <N> --json headRepository,headRefName` to discover the fork
  clone URL, then `git -C <repo_path> fetch <fork-url> <head_ref>:pr-<N>`.
  Returns the local branch name (`pr-<N>`). Does NOT move HEAD.

### Backend: `src-tauri/src/commands/pr.rs` (new)

Thin Tauri command adapters:

- `#[tauri::command] check_gh_installed() -> bool`
- `#[tauri::command] lookup_pr(repo_path: String, url: String) -> Result<PrInfo, String>`
- `#[tauri::command] fetch_pr_branch(repo_path: String, pr_number: u32) -> Result<String, String>`

Registered from `src-tauri/src/main.rs` alongside existing commands.

No changes to `services/sessions.rs` or `worktree.rs` — `create_session_shell`
already accepts an explicit branch via `SessionTarget::NewWorktree`, and the
branch exists locally by the time the user clicks Create.

## Data flow

1. Dialog opens → `checkGhInstalled()` → gates PR input visibility.
2. User pastes URL → debounce → `lookupPr()` → `PrInfo` returned.
3. Dialog displays PR title + cross-repo badge.
4. If `isCrossRepository`, `fetchPrBranch()` is called; the local
   `pr-<NNN>` branch now exists in the main repo's branch list.
5. Dialog pre-fills `worktreeFilterInput` with the resolved branch name and
   `sessionName` with the slug.
6. User clicks Create → existing path:
   - `resolveGitTarget()` finds either an existing worktree for the branch or
     returns `branchArg = <branch>`.
   - `createSessionShell` creates a new worktree off that branch in the
     configured worktree base dir.

## Error handling

All errors surface inline in the dialog under the PR input. The dialog remains
usable — the user can clear the URL and proceed without the PR feature.

| Condition                              | Error message                                                        |
| -------------------------------------- | -------------------------------------------------------------------- |
| `gh` missing at lookup time            | `gh CLI not found`                                                   |
| `gh auth` missing / expired            | `gh is not authenticated — run 'gh auth login' and retry`            |
| PR not found / 404                     | `PR not found`                                                       |
| URL parse failure                      | `Not a valid GitHub PR URL`                                          |
| Network failure                        | `Failed to fetch PR: <gh stderr truncated to 200 chars>`             |
| Fork fetch failure                     | `Failed to fetch fork branch: <git stderr truncated>`                |
| Branch `pr-<N>` already exists locally | `fetch_pr_branch` force-updates it (`+<head_ref>:pr-<N>` refspec)   |

## Testing

### Rust unit tests (`src-tauri/src/pr.rs`)

- `parse_pr_ref`:
  - full URL → `Some(PrRef)`
  - shortform `owner/repo#123` → `Some(PrRef)`
  - garbage input → `None`
  - trailing slash / query string variants
- `lookup_pr` / `fetch_pr_branch`: test via a thin `GhRunner` trait (or
  `Command` abstraction) so we can stub `gh` output without requiring `gh` on
  CI. Happy-path + auth-error + 404 cases.

### Frontend unit tests (`src/lib/components/__tests__/NewSessionDialog.test.ts`)

- PR input hidden when `checkGhInstalled()` returns `false`.
- On valid URL paste + mocked `lookupPr` → session name + branch filter
  pre-filled.
- On fork PR → `fetchPrBranch` called; returned branch name populates the
  filter.
- On lookup error → error string rendered inline, other dialog fields still
  usable.
- User-edit-wins: if the user types into `sessionName` after the lookup, a
  subsequent lookup does not overwrite it.

### Manual verification

- Paste same-repo PR URL → new worktree created at correct branch.
- Paste fork PR URL → local `pr-<N>` branch exists after fetch; worktree
  created off it; main repo HEAD unchanged.
- Dialog with `gh` uninstalled → no PR input rendered.

## Open considerations

- **Slug locale**: the title slug uses ASCII alphanum only; non-Latin titles
  will collapse to `pr-<N>-`. Acceptable for v1.
- **Debounce timing**: 250ms is a starting point; may need tuning if `gh`
  latency is consistently >1s.
- **Existing worktree for PR branch**: if the user creates two sessions from
  the same PR URL, the second one will find the existing worktree and
  select it. This is correct behavior; no special-case needed.

## Files touched

- `src/lib/components/NewSessionDialog.svelte` — UI + state
- `src/lib/components/__tests__/NewSessionDialog.test.ts` — new or extended
- `src/lib/tauri.ts` — three new bindings
- `src/lib/bindings.ts` — generated types for `PrInfo`
- `src-tauri/src/pr.rs` — new module
- `src-tauri/src/commands/pr.rs` — new Tauri commands
- `src-tauri/src/commands/mod.rs` — register new module
- `src-tauri/src/main.rs` — register new commands in handler list
- `src-tauri/Cargo.toml` — `which` likely already present; confirm during plan
