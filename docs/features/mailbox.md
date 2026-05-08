# Mailbox

Roux gives every agent a stable, addressable inbox so humans and other agents
can send durable messages to a session without typing into its terminal at the
wrong moment. The same store also powers a topic-based bus for ambient
broadcasts (build done, tests went red, deploy started).

!!! info "Why not just `roux send`?"
    `roux send` types raw bytes into a target session's PTY. Useful, but it
    jams whatever the receiver was doing — there's no queueing, no acking,
    no addressing by role, no audit trail. The mailbox keeps the same
    "across-pane communication" intent but adds: durable queueing,
    per-recipient read/ack state, alias-based addressing that survives
    session restart, and a backend the UI and MCP can plug into.

## The three layers

| Layer | What it is | Used for |
|---|---|---|
| **Aliases** | Stable, restart-durable names bound to panes (`reviewer`, `builder`, `me`) | Addressing |
| **Mailbox** | Queue of events addressed `to=<alias>` with per-recipient read/ack state | Direct mail |
| **Bus** | Same store, addressed by `topic` (e.g. `repo-a.build.completed`) | Broadcasts |

All three live in one append-only event log. "Mailbox" and "bus" are usage
patterns over the same store, not separate systems.

## Aliases

An **alias** is a human-meaningful name (lowercase letters, digits, hyphens;
1–64 chars) that resolves to whichever pane currently holds it. Aliases
survive session restart; pane IDs do not.

### How aliases get bound

1. **Auto-claim from pane name** *(easiest)*. Rename a pane to something that
   matches the alias format and Roux auto-claims that name as the pane's
   alias. Look for the `@<alias>` chip in the pane's title bar.
2. **`roux alias claim <name>`** *(manual)*. Run from inside the pane.
   Defaults to `$ROUX_PANE_ID`.
3. **`roux alias set <name> --session <id>` or `--pane <id>`** *(third-party)*.
   Bind from the outside, useful for scripts/MCP.

If the pane's name doesn't match the alias format (capitals, spaces,
parens) Roux skips auto-claim — the user can still bind manually. Reserved
names that can't be claimed: `me`, `human` (alias of `me`), `system`,
`audit`, `roux`.

### Inspecting aliases

```sh
roux alias whoami                       # what aliases is my pane holding?
roux alias list                         # all aliases (JSON)
roux alias list --only-unbound          # aliases with no current pane
roux alias get reviewer                 # resolve one alias
roux alias get reviewer --project foo   # project-scoped lookup
```

### Releasing

Auto-claimed aliases release when:

- the pane is renamed to a non-conformant name
- the pane is closed
- the user runs `roux alias unset <name>`

Manual claims persist across pane close — queued mail keeps for whoever
re-claims the alias next.

### The `me` alias

`me` (and its synonym `human`) is reserved for **you, the human at the
keyboard**. Agents post to `me` when they need your attention; mail
addressed to `me` shows up in the Mailbox panel's main inbox and fires a
Roux notification. Agents can't claim `me`.

### Project scoping

The same alias name can exist independently in different projects:

- `reviewer@frontend-app` and `reviewer@mobile-app` are different aliases.
- Bare `roux alias get reviewer` looks up in your current project's scope
  first (via `$ROUX_PROJECT_ID`), falling back to the global scope.
- Cross-project addressing uses `<alias>@<project>` syntax.

## Mailbox

Direct addressed mail with per-recipient read/ack state.

### Posting

```sh
roux mailbox post --to reviewer "PR ready: https://github.com/..."
roux mailbox post --to me "I need a decision on X" --kind question
roux mailbox post --to reviewer --subject "Hot fix" --kind task "<long body>"
```

Kinds: `task` (default), `result`, `question`, `fyi`, `signal`. The Mailbox
panel surfaces `me`-addressed mail and questions in the main inbox; agent-
to-agent `fyi` traffic shows in the Firehose tab only.

### Reading

```sh
roux mailbox count                    # how many unread? (JSON)
roux mailbox peek --unread            # see them without consuming
roux mailbox read --ack               # drain + mark processed (recommended)
```

`read` returns each event as JSON. `--ack` also flips `acked_at` on each so
the sender's `mailbox sent` view shows the work was processed.

### Acking with results

```sh
roux mailbox ack <event_id> --result "PR merged"
```

The result string is visible to the sender. Use it for short status updates
the original poster cares about.

### Threaded replies

Reply to an event by id; the new event copies the original's `correlation_id`
(or seeds one from the original event id if absent), so the conversation
threads:

```sh
roux mailbox reply <event_id> "looking now, will get back in 10"
```

### Sender's view

```sh
roux mailbox sent                     # everything I've sent
roux mailbox sent --to reviewer       # only stuff I sent to reviewer
```

Each row pairs the event with the recipient's read/ack state, so you can
tell whether the reviewer saw your mail and what they did with it.

## Bus

When you want to publish "this happened" without addressing a specific
recipient:

```sh
roux bus publish repo-a.build.completed "main is green"
roux bus tail --topic repo-a.build.completed   # filter
roux bus tail                                   # firehose, newest first
```

Topics live in the same store as mailbox events. An event can have *both*
`--to` and `--topic` — it lands in the recipient's inbox AND fires for
anyone tailing that topic.

Default `kind` for `bus publish` is `signal`.

## The Mailbox panel (UI)

Click the inbox icon in the activity rail (or use the keybinding for
"mailbox"). The panel has two views:

- **Inbox** — alias selector strip + the selected alias's events, oldest
  first. Mark-read / ack / clear-read buttons per event. Compose form at
  the top sends to any alias (with autocomplete).
- **All / Firehose** — every event newest-first, no per-recipient state.
  Read-only; for awareness across recipients.

The pane title bar shows an `@<alias>` chip when an alias is bound. Auto-
claimed bindings get a lighter outline; manually-claimed bindings are
filled. An unread badge appears next to the chip when mail is waiting.

### The Deliver button

In the Inbox view, each event has a `deliver →` button **iff the
recipient alias is bound to a live pane**. Clicking it types the message
body (plus a CR) into the recipient's pane via the existing PTY write
path, then auto-acks. This is the "send to my friend" UX — combines
mailbox addressing with PTY delivery for the last mile, useful when the
recipient agent isn't running a hook to drain mail automatically.

## Environment variables

When a pane is hosting an agent, Roux injects these:

| Var | Meaning |
|---|---|
| `ROUX_AGENT_ALIAS` | Alias bound to this pane at PTY spawn time. Snapshot — does not update mid-session. Use `roux alias whoami` for live state. |
| `ROUX_PANE_ID` | The pane's id; pass to `roux alias claim`. |
| `ROUX_SESSION_ID` | The session's id. |
| `ROUX_PROJECT_ID` | Project scope, if any. Used for current-project alias resolution. |

## How agents discover the mailbox

Roux ships a Claude Code [skill](../sessions.md#skills) (auto-installed at
`~/.claude/skills/roux/SKILL.md`) that teaches agents:

- `$ROUX_SESSION=1` ⇒ they're in Roux
- The `mailbox`, `alias`, and `bus` CLI surfaces and when to use them
- That "mail" / "inbox" / "messages" mean **Roux mailbox**, NOT Gmail (this
  prevents Claude from reaching for the Gmail MCP when you ask "any new
  mail?" inside a Roux pane)

For deterministic auto-drain (without relying on the model remembering),
add a `UserPromptSubmit` hook in your project's `.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "command": "roux mailbox read --ack"
    }]
  }
}
```

Output of `mailbox read` will be prepended to whatever you type, so the
agent picks up pending mail at the start of every turn.

## MCP

The Roux MCP server exposes the mailbox tools natively to MCP-aware
clients. To wire it up:

1. Open Roux → Settings → **Integrations** → **Model Context Protocol**.
2. Click **Add to Claude Code**, **Add to Claude Desktop**, or **Add to
   Codex** depending on which client you use. Roux writes the right config
   shape (JSON `mcpServers` for Claude clients, TOML `[mcp_servers.roux]`
   for Codex) and prints the diff in the preview pane.
3. Restart the client. The next session sees `roux_mailbox_*`,
   `roux_alias_*`, `roux_bus_*` tools natively — no shell-out cost, no
   permission prompts per call.

Behind the scenes the MCP tools just wrap the same socket commands the
CLI uses; you get parity with the CLI surface, just typed.

## Persistence

| File | Format | Notes |
|---|---|---|
| `aliases.json` | Versioned envelope (`{version: 1, data: [...]}`) | Full rewrite on mutation |
| `events.jsonl` | Append-only NDJSON, one event per line, with `schemaVersion: 1` | Audit log; never compacted |
| `read_state.json` | Versioned envelope | Full rewrite on mark-read / ack / clear-read |

All three live under `roux_config_dir()` (`~/.config/roux/` on
macOS/Linux). Future-version rows are preserved on disk but skipped at
load time, so a downgrade doesn't lose data.

## Retention

The in-memory event store caps at the most recent 5,000 events. Events
past the cap are evicted from RAM but stay in `events.jsonl` for audit.
Restarts rebuild the in-memory view from the on-disk log, applying the
cap.

## Discipline

- **Don't drain mail mid-tool-call.** Mail is durable — it's there at the
  start of your next turn. Draining mid-task fragments your context.
- **Don't flood.** A soft per-sender rate cap (~60 events/min) folds
  identical bodies posted to the same recipient within 5 seconds.
- **Treat `me` mail with priority.** It's the human asking — surface it,
  don't silently sit on it.

## Common workflows

### "Tell my reviewer I'm ready"

From the implementer's pane:

```sh
roux mailbox post --to reviewer "PR ready, /Users/me/branch-foo, please look"
```

The reviewer's pane shows the unread badge. The reviewer (a person or an
agent) drains via `roux mailbox read --ack` or by clicking through the
Mailbox panel.

### "Coordinate two agents on a worktree"

```sh
# Open a worktree session, name two panes "frontend" and "backend"
# (auto-claim picks them up)
roux session create --worktree-branch feature-x

# From the frontend pane:
roux mailbox post --to backend "API contract for /reviews: {fields...}"

# From the backend pane:
roux mailbox read --ack       # sees the contract
# ...does the work...
roux mailbox post --to frontend --kind result "/reviews implemented in commit abc123"
```

### "Broadcast that the build went green"

```sh
roux bus publish repo-a.build.completed "main is green at sha abc123"
```

Anyone tailing `repo-a.*` (or a wildcard subscriber, once subscriptions
land) sees it.

### "Send me a note from a script"

```sh
roux mailbox post --to me "deploy finished, took 4m12s" --kind fyi
```

Roux fires a notification on your screen.

## Troubleshooting

**`alias 'reviewer' is already bound to pane 'pane-XYZ'`**: another pane
holds the alias. Either rename the conflicting pane (auto-release), use
`roux alias claim reviewer --steal` to take it over, or pick a different
name.

**My pane name auto-claimed but other pane has the same name**: only the
first auto-claim wins; the second pane keeps its name but no alias.
Manually `roux alias claim <some-other-name>` for the second pane.

**Mail isn't arriving even though I posted**: check `roux alias list` to
confirm the recipient alias exists and has a `paneId` set. Posts to
unbound aliases queue durably — mail is there, just no live recipient.
Bind a pane (or use the Deliver button) to see it.

**Agent ignores mail**: install the `UserPromptSubmit` hook (see above)
or run `roux mailbox read` manually. The skill teaches agents about the
mailbox, but doesn't compel them to drain on every turn — hooks do.
