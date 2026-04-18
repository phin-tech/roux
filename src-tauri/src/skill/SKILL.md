---
name: roux
description: Drive the Roux terminal manager from inside a Roux-hosted pane. Use when $ROUX_SESSION=1 and the user asks to spawn panes, send input to other sessions, split layouts, focus panes, post notifications, list/create sessions, or otherwise orchestrate Roux.
---

<!-- roux-skill-version: 2 -->

# Roux

Roux is a desktop terminal manager (panes, tabs, worktrees, sessions) that
hosts Claude Code and shell panes. When this skill loads, you are very likely
running *inside* one of those panes.

## Detect you are in Roux

Check `$ROUX_SESSION`. If it equals `1`, you are in a Roux-hosted PTY and the
rest of this skill applies. If unset, do nothing — fall back to your normal
shell behavior.

## Environment Roux sets for you

| Var                 | Meaning                                                            |
|---------------------|--------------------------------------------------------------------|
| `ROUX_SESSION=1`    | Marker. You are inside a Roux pane.                                |
| `ROUX_CLI`          | Absolute path to `roux-cli`. Prefer this over `roux-cli` on PATH.  |
| `ROUX_SOCKET`       | Unix socket / Windows named endpoint Roux listens on.              |
| `ROUX_SESSION_ID`   | Id of the session this pane belongs to.                            |
| `ROUX_PANE_ID`      | Id of this pane.                                                   |
| `ROUX_PROJECT_ID`   | Id of the project this session belongs to, if any.                 |
| `ROUX_WORKTREE_PATH`| Absolute path to the worktree root, if the session is worktree-backed. |
| `ROUX_NOTES_ROOT`   | Root of the Obsidian-compatible notes vault (experimental).        |
| `ROUX_SESSION_DIR`  | This session's vault folder (experimental).                         |
| `ROUX_SESSION_NOTES_FILE` | This session's anchor `notes.md` (experimental).              |
| `ROUX_REPO_SLUG`    | Resolved slug for this session's repo (experimental).              |
| `ROUX_REPO_NOTES_FILE` / `_DIR` | This session's repo-scope notes file and dir (experimental). |
| `ROUX_GLOBAL_NOTES_FILE` / `_DIR` | User-level global notes file and dir (experimental).    |
| `ROUX_SESSION_PROJECT` | Project slug when the session has a project; unset otherwise.   |
| `ROUX_SESSION_PROJECT_NOTES_FILE` / `_DIR` | Project-scope notes file and dir, when a project is assigned. |

Several `roux-cli` subcommands default to `$ROUX_SESSION_ID` / `$ROUX_PANE_ID`
when those flags are omitted, so you rarely need to pass them explicitly.

## Invoking the CLI

Always call the binary at `$ROUX_CLI`, e.g.:

```sh
"$ROUX_CLI" session list
```

This avoids PATH issues and the ambiguity of a `roux` shell alias.

## CLI surface

All output that ends in "(JSON)" below is machine-parseable JSON.

### Sessions

- `"$ROUX_CLI" session list` — list all sessions (JSON).
- `"$ROUX_CLI" session create [--name N] [--working-dir DIR] [--worktree-branch BRANCH] [--profile P] [--flag F]...`
  Create a new session. `--worktree-branch` creates a git worktree from the
  given branch, rooted at `--working-dir` (or cwd).
- `"$ROUX_CLI" session poll [--session ID]` — dump session state (JSON).
  Defaults to `$ROUX_SESSION_ID`.
- `"$ROUX_CLI" session send "text" [--session ID] [--pane ID] [--no-enter]`
  Type into a session's PTY. Enter is appended by default.

### Panes

- `"$ROUX_CLI" session panes list [--session ID]` — list panes (JSON).
- `"$ROUX_CLI" session panes create [--session ID] [--profile P] [--direction horizontal|vertical] [--working-dir DIR]`
  Open a new pane split off the active one. Default profile: `plain-shell`.
- `"$ROUX_CLI" split [--direction horizontal|vertical]` — split the current pane.
- `"$ROUX_CLI" shell [--working-dir DIR]` — open a shell pane.
- `"$ROUX_CLI" run "<command>" [--working-dir DIR]` — run a command in a new pane.
- `"$ROUX_CLI" focus [--pane ID] [--session ID]` — focus a pane or session.

### App

- `"$ROUX_CLI" app [PATH]` — open or focus a Roux session for a directory and
  bring the app to front. Defaults to `.`.
- `"$ROUX_CLI" notify -t "Title" [-b "Body"] [--subtitle S] [-l info|success|attention|warning|error] [-s SESSION] [--cwd DIR] [--source TAG]`
  Post a notification. Routes to `$ROUX_SESSION_ID` by default.

### Notes (experimental)

Roux hosts an Obsidian-compatible markdown vault at `$ROUX_NOTES_ROOT`. The
CLI exposes four scopes: `global`, `project`, `repo`, `session`. The
session/scope context is read from `$ROUX_SESSION_ID` automatically.

Verbs (per scope):
- `"$ROUX_CLI" notes <scope> show [--topic NAME]` — print the file.
- `"$ROUX_CLI" notes <scope> append [--content TEXT] [--timestamp] [--topic NAME] [--tag TAG]...`
  — append. Reads stdin if `--content` is omitted. `--timestamp` wraps the
  addition in a timestamped heading + Obsidian block ref + optional HTML
  anchor (for deep-linking if the vault is ever published).
- `"$ROUX_CLI" notes <scope> write [--content TEXT] [--topic NAME] [--tag TAG]...`
  — replace the file body. Reads stdin if `--content` is omitted.
- `"$ROUX_CLI" notes <scope> path [--topic NAME] [--dir]` — print the
  absolute file (or directory) path. Useful for piping into other tools.

Cross-scope:
- `"$ROUX_CLI" notes search --tag TAG [--tag TAG]... [--scope SCOPE] [--tag-exact]`
  — find all notes whose tags match all supplied filters. Matches
  frontmatter tags OR inline `#tag` body occurrences (same semantics as
  Obsidian's tag pane). Hierarchical prefix matching by default
  (`--tag api` finds `api/tls`).
- `"$ROUX_CLI" notes root` — print `$ROUX_NOTES_ROOT`.

Topic files (`--topic NAME`) are arbitrary `.md` files inside a scope's
directory. Use them for the "one topic, many entries" pattern — e.g.
`"$ROUX_CLI" notes repo append --topic api-gotchas --timestamp --tag api "TLS fix"`
appends a dated entry to `repos/<repo-slug>/api-gotchas.md`, creating the
file with frontmatter on first write.

The vault is a clean Obsidian vault (frontmatter YAML + markdown, no
sidecar state). Users can open `$ROUX_NOTES_ROOT` in Obsidian directly.
Agents should prefer the CLI — Obsidian requires a running GUI, the CLI
does not.

## When to use this skill

Trigger any time the user asks to:

- spawn, split, or close panes ("open a shell here", "split vertically", "run
  `foo` in a new pane")
- switch focus between panes or sessions
- list or create sessions, especially with worktrees ("make a worktree for
  branch X and start Claude there")
- send keystrokes or text to a sibling session
- post a Roux notification (status pings, task completion, etc.)
- read the current session/pane state (`poll`, `list`)
- read, append to, write, or search notes in the vault ("log this to my
  session notes", "add #api/tls to that gotcha", "what have I written about
  TLS?")

Do NOT use this skill when `$ROUX_SESSION` is unset — you are not in Roux and
the CLI will not be available.

## Orchestrating sibling agents

Because every Roux pane carries its own `ROUX_SESSION_ID` / `ROUX_PANE_ID`,
you can drive sibling Claude Code sessions:

1. `"$ROUX_CLI" session list` to find the target session id.
2. `"$ROUX_CLI" session send "prompt text" --session <id>` to type into it.
3. `"$ROUX_CLI" session poll --session <id>` to read its state back.

Treat this carefully — sending input mid-turn can interrupt the other agent.

## Failure modes

- If `$ROUX_CLI` is unset but `$ROUX_SESSION=1`, something is wrong with the
  install. Tell the user to open Roux → Settings → Doctor and reinstall the
  CLI.
- If the CLI reports "Roux is not running", the host app has exited; nothing
  in this skill will work until it is relaunched.
