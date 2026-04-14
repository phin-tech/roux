# Claude Code Skill Injection

**Status:** Design
**Date:** 2026-04-13

## Problem

Claude Code sessions running inside Roux panes have no built-in knowledge of
Roux's CLI bridge, pane/session context, or how to drive the host app. Users
who want Claude to spawn panes, read terminal output, manage worktrees, or
orchestrate sibling sessions have to teach it by hand every time.

jmux solves the equivalent problem by shipping an "agent skill" that Claude
Code auto-discovers, combined with env-var markers (`$TMUX`, `$TMUX_PANE`)
and a `jmux ctl` CLI. Roux already has the CLI (`roux-cli`) and most of the
env markers; the missing piece is the skill document and two extra env vars.

## Goals

- Any Claude Code session launched inside a Roux PTY can immediately drive
  Roux via the existing socket/CLI bridge, with zero per-project setup.
- Skill install is automatic, idempotent, and versioned.
- Add only the env vars the skill actually needs; reuse what's already set.

## Non-Goals (v1)

- Codex, Gemini CLI, Cursor, or other agent integrations.
- Project-level or worktree-level skill files.
- A full MCP server. (The existing socket/CLI surface is what the skill
  teaches Claude to use.)

## Design

### Components

**1. Bundled skill document**
A single `SKILL.md` shipped inside the Roux binary/resources. Contents:
- Frontmatter: `name: roux`, description mentioning Roux + trigger conditions.
- Trigger guidance: "use when `$ROUX_SESSION=1` and the user asks about
  spawning panes, reading pane output, switching worktrees, orchestrating
  sibling sessions, or anything referencing Roux."
- Documented env vars (see below).
- Documented `$ROUX_CLI` subcommand surface, pulled from `src-tauri/src/cli.rs`.
- Version header (e.g., `# roux-skill-version: N`) so Roux can detect stale
  installs and overwrite.

**2. Install-on-startup**
On Roux app startup, a Rust routine:
- Resolves `~/.claude/skills/roux/SKILL.md` (XDG-respecting on non-mac where
  relevant; Claude Code's canonical path is `~/.claude/skills/<name>/`).
- If the file is missing, or its version header is older than the bundled
  version, writes the bundled content atomically.
- Never touches unrelated files in `~/.claude/skills/`.
- Logs (at info level) what it did.

**3. Extended PTY env**
`src-tauri/src/pty.rs` currently sets `ROUX_SESSION`, `ROUX_SOCKET`,
`ROUX_CLI`, `ROUX_SESSION_ID`, `ROUX_PANE_ID`. Add:
- `ROUX_PROJECT_ID` — id of the project the session belongs to (when known).
- `ROUX_WORKTREE_PATH` — absolute path to the worktree root when the session
  is worktree-backed; unset otherwise.

Both are set only when their value is available; the skill treats "unset" as
a meaningful signal.

### Data flow

```
Roux startup
  └─ install_skill()  →  ~/.claude/skills/roux/SKILL.md

Pane spawn (pty.rs)
  └─ cmd.env(ROUX_*)  →  PTY  →  shell/Claude inherit env

Claude inside pane
  └─ reads SKILL.md on session start
  └─ sees $ROUX_SESSION=1, knows skill applies
  └─ invokes $ROUX_CLI <subcommand>  →  unix socket  →  Roux backend
```

### Error handling

- Skill write failures (permissions, missing home dir) are logged and
  non-fatal. App continues to start.
- Env vars that can't be resolved (no project id, no worktree) are simply
  omitted from the PTY env rather than set to empty strings.

### Versioning / update

- Bundled skill carries an integer version in a dedicated header line.
- Install routine parses the existing file's version; overwrites iff
  bundled version > on-disk version, or file is missing/unparseable.
- Uninstall is manual: documented path in the skill file itself.

## Testing

- Rust unit test for the version-compare / overwrite logic (pure function
  over file contents).
- Rust test confirming the PTY env builder includes the two new vars when
  the relevant inputs are present, and omits them otherwise.
- Manual verification: launch Roux, confirm `~/.claude/skills/roux/SKILL.md`
  exists; spawn a Claude pane; confirm Claude can run `$ROUX_CLI list` (or
  equivalent) and acts on pane context without being told the details.

## Open questions

None blocking. Codex/Gemini analogs and project-level overrides are
deliberately deferred.
