# Kanban Board

Roux's Kanban board is a project-scoped work queue for agent tasks. Cards are
durable work items stored by the daemon; terminal panes only render and attach
to the daemon-owned run state.

## Cards and Runs

Each card can have many runs. Cards can be created as drafts without a repo or
agent profile; those cards show **Configure** until they have enough daemon-owned
start config.

Starting a configured card is daemon-owned. The daemon creates or reuses the
card's dedicated worktree, creates a session/PTY for the selected autonomous
agent profile, writes the generated task prompt into that PTY, and only then
moves the card to **In Progress**. The card's `session_id` is latest-session
display state only.

After a card has an active or previous run, the card shows **Open terminal**
instead of **Start**. Opening the terminal attaches to the latest linked session;
it does not create another run by itself. Starting again creates a separate run
history entry rather than overwriting the prior attempt.

If start fails before prompt dispatch completes, the card stays in **Todo** or
**Ready**, records a visible `startError`, and preserves any created
session/worktree for inspection or retry.

Run history is persisted under the card and survives closing and reopening Roux.
PTY exit updates the run lifecycle: exit code `0` marks it `done`; non-zero or
unknown exits mark it `failed`. Explicitly stopping a run marks it `stopped` and
does not get overwritten by a later PTY exit.

## Decisions

Agents can emit structured decision prompts as newline-delimited JSON. Roux
persists those prompts as `WorkItemDecision` rows under the run, marks the run
blocked, and shows the decision on both the card and detail view.

The card shows the blocked question plus numbered choices. The detail view shows
the same choices as buttons. Choosing an option resolves the decision, records
an audit event, and writes the selected value plus a newline back to the linked
session.

Decision prompts may include a default and timeout. If the timeout expires, the
daemon records the decision as timed out, writes the default value back to the
linked session, and unblocks the run when no other pending decisions remain.

## Deleting Cards

Deleting a card deletes the card's daemon-owned run history, run events, and
decision prompts from the board database. Session/PTY deletion is a separate
runtime-lifecycle decision surfaced by the app confirmation flow.

## CLI and MCP

The human CLI wraps the daemon work-item commands:

```bash
roux work-item list
roux work-item create "Fix login" --project <project-id> --agent-profile claude --repo-path /path/to/repo
roux work-item move <card-id> ready
roux work-item start <card-id>
roux work-item runs --work-item <card-id>
roux work-item events <run-id>
roux work-item decision list --work-item <card-id>
roux work-item decision resolve <decision-id> <value>
```

`roux kanban ...` is a visible alias for `roux work-item ...`.

The MCP server exposes the same daemon-backed board surface with tools such as
`roux_list_work_items`, `roux_create_work_item`, `roux_start_work_item`,
`roux_list_work_item_runs`, and `roux_resolve_work_item_decision`.

## Related Protocol

The daemon protocol surface is documented in
[Daemon Protocol: Work Items](../v2/daemon-protocol.md#work-items).
