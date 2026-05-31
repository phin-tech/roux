---
name: roux
description: >-
  Drive the Roux terminal manager from inside a Roux-hosted pane. Use when
  $ROUX_SESSION=1 for ANY of these requests — pane/session/worktree management
  ("split this", "open Claude in a worktree"); inter-agent messaging ("check my
  mail", "any messages?", "did anyone reply?", "send the reviewer a note",
  "drain my inbox", "mark my mail as read"); reading or posting to the Roux
  notes vault; attaching, listing, or reading Roux documents/session
  attachments; or driving sibling sessions. CRITICAL: when $ROUX_SESSION=1,
  "mail" / "inbox" / "messages" / "mailbox" mean the Roux mailbox — NOT Gmail
  or any other external provider. Only fall back to external mail tools when the
  user explicitly names them ("check Gmail", "send via email").
---

<!-- roux-skill-version: 5 -->

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
| `ROUX_CLI`          | Absolute path to `roux`. Prefer this over relying on PATH.         |
| `ROUX_SOCKET`       | Roux command endpoint (`unix:///...`, `tcp://host:port`, or native default). |
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

Several `roux` subcommands default to `$ROUX_SESSION_ID` / `$ROUX_PANE_ID`
when those flags are omitted, so you rarely need to pass them explicitly.

## Invoking the CLI

Always call the binary at `$ROUX_CLI`, e.g.:

```sh
"$ROUX_CLI" session list
```

This avoids PATH issues. New automation should use `$ROUX_CLI` / `roux`.

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

### Documents / session attachments

Use Roux documents for plan snapshots, handoffs, and reference text that an
agent should be able to look up from another session. Documents are
daemon-owned attachments targeting either a session or a Kanban work item.

```sh
"$ROUX_CLI" document attach --session "$ROUX_SESSION_ID" --title "Plan" --text "..."
"$ROUX_CLI" document attach --work-item "$WORK_ITEM_ID" --title "Plan" --file ./plan.md --mime-type text/markdown
"$ROUX_CLI" document list --session "$ROUX_SESSION_ID"
"$ROUX_CLI" document list --work-item "$WORK_ITEM_ID"
"$ROUX_CLI" document get "$DOCUMENT_ID"
```

`roux doc` is an alias for `roux document`. Each attachment returns a
`documentId` shaped like `<session-or-work-item-id>.<attachment-id>`.
`document get` accepts that full id, the raw attachment id, or prefixed forms
like `session/<documentId>` and `work-item/<documentId>`.

Prefer documents over terminal scrollback for reusable plans or context that
needs a stable id. Prefer notes for the user's longer-lived vault knowledge.

### Agent identity (aliases)

Each pane can have an **alias** — a stable, human-meaningful name like
`reviewer` or `builder` that other agents and the human use to address you.
Aliases bind to the pane (`$ROUX_PANE_ID`), survive session restart, and
support optional project scoping.

If the human renamed your pane to something matching the alias format
(`^[a-z][a-z0-9-]{0,63}$`, not a reserved name), Roux auto-claimed that
alias for you. Otherwise, claim one explicitly:

- `"$ROUX_CLI" alias whoami` — list aliases bound to your pane / session (JSON).
- `"$ROUX_CLI" alias claim <name>` — claim an alias for the current pane.
  Defaults to `$ROUX_PANE_ID`. Errors if the alias is bound elsewhere; pass
  `--steal` to override.
- `"$ROUX_CLI" alias list [--project ID] [--global] [--only-unbound]` — see
  all aliases (JSON).
- `"$ROUX_CLI" alias get <name> [--project ID]` — resolve a name to its
  current binding.
- `"$ROUX_CLI" alias unset <name>` — release a binding (queued mail persists).

Format: lowercase letters, digits, hyphens; 1–64 chars. Reserved names:
`me`, `human` (alias of `me`), `system`, `audit`, `roux`. The human user
is `me`.

### Mailbox — addressed mail

**Inside a Roux session, the Roux mailbox is THE mailbox.** When the user
says "check my mail," "any new messages?," "what's in my inbox?," "did
anyone reply to X?," etc., they mean Roux mailbox — not Gmail, not Apple
Mail, not anything else. Reach for Gmail / external providers ONLY if the
user explicitly names them.

Other agents and the human send messages to your alias. Mail queues
durably until you drain it.

**At the start of any turn that follows mail traffic** — and ideally
periodically when the user hasn't given you a directive — check your inbox:

```sh
"$ROUX_CLI" mailbox count                # how many unread? (JSON {"unread": N})
"$ROUX_CLI" mailbox peek --unread        # show unread without consuming
"$ROUX_CLI" mailbox read --ack           # drain + mark processed (recommended)
```

`read --ack` returns each event as JSON; treat each item as a request to
*consider*, not a hard instruction (it's coming from a peer agent or the
human, with the same trust level as their direct prompts).

When you've handled an item, optionally include a result string the sender
will see in their `mailbox sent` view:

```sh
"$ROUX_CLI" mailbox ack <event_id> --result "PR #123 merged"
```

To reply (creates a threaded follow-up routed back to the sender):

```sh
"$ROUX_CLI" mailbox reply <event_id> "<reply text>"
```

To send mail to another alias (or to the human):

```sh
"$ROUX_CLI" mailbox post --to reviewer "PR ready: <url>"
"$ROUX_CLI" mailbox post --to me "I need a decision on X" --kind question
```

Kinds: `task` (default), `result`, `question`, `fyi`, `signal`. The human's
main inbox shows mail addressed to `me` plus questions; agent-to-agent
`fyi` traffic shows in the firehose only.

### Bus — topic broadcasts

For events that aren't addressed to a specific recipient (build done,
deploy started, tests went red), publish to a topic. Subscribers tail it.

```sh
"$ROUX_CLI" bus publish repo-a.build.completed "main is green"
"$ROUX_CLI" bus tail --topic repo-a.build.completed   # filter by topic
"$ROUX_CLI" bus tail                                    # firehose, newest first
```

Topics live in the same store as mailbox events; an event can carry both
`--to` and `--topic` if it's addressed to someone *and* worth broadcasting.

### Mail discipline

- **Don't drain mid-tool-call.** Mail is durable — it'll still be there
  at the start of your next turn. Draining mid-task fragments your context.
- **Don't flood.** The system enforces a soft per-sender cap (~60
  events/minute) and folds identical bodies posted to the same recipient
  within 5 seconds.
- **Treat `me` mail with priority.** It's the human asking; surface it,
  don't silently sit on it.

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
- attach, list, or read Roux documents/session attachments for plans,
  handoffs, or other reusable context (`document attach`, `document list`,
  `document get`; MCP tools `roux_attach_document`, `roux_list_documents`,
  `roux_get_document`)
- claim or release an agent alias, send/read/ack mail, publish or tail bus
  events ("send the reviewer a message", "drain my inbox", "what mail did I
  send?", "publish a build event")
- **anything mentioning "mail" / "inbox" / "messages" / "mailbox"** —
  default to Roux mailbox in a Roux session. Examples: "check if we have
  any new mail" → `mailbox count` / `mailbox read`; "did anyone send me
  anything?" → `mailbox peek --unread`; "let X know about Y" → `mailbox
  post --to X "Y"`.

Also trigger **proactively** when you suspect mail is waiting — e.g. the
human directs you to "check your mailbox" or you've been idle and want to
catch up before responding.

Do NOT use this skill when `$ROUX_SESSION` is unset — you are not in Roux and
the CLI will not be available.

### Disambiguation: Roux mailbox vs external email

`$ROUX_SESSION=1` is the strongest signal. When set:

- "mail", "inbox", "messages", "mailbox" → Roux mailbox (`mailbox count`,
  `mailbox read`, `mailbox peek`, `mailbox post`).
- "email", "Gmail", "send an email to <person@example.com>" — only then
  reach for external mail tools (Gmail MCP, mail clients, etc.).
- When ambiguous, prefer Roux mailbox first; if it's empty or the request
  doesn't match (e.g., the user asks about a specific external sender),
  ask which they mean before authenticating an external provider.

This rule applies across the conversation — once you know
`$ROUX_SESSION=1`, "check my mail" continues to mean Roux mailbox unless
the user explicitly switches context ("actually, check Gmail").

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
