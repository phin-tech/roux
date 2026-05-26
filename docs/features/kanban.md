# Kanban Board

Roux's Kanban board is a project-scoped work queue for agent tasks. Cards are
durable work items stored by the daemon; terminal panes only render and attach
to the daemon-owned run state.

## Cards and Runs

Each card can have many runs. Starting a card creates a new `WorkItemRun`, links
it to a daemon session/PTy, moves the card to **In Progress**, and keeps the
card's `session_id` as latest-session display state only.

After a card has an active or previous run, the card shows **Open terminal**
instead of **Start**. Opening the terminal attaches to the latest linked session;
it does not create another run by itself. Starting again creates a separate run
history entry rather than overwriting the prior attempt.

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
decision prompts from the board database. Session/PTy deletion is a separate
runtime-lifecycle decision surfaced by the app confirmation flow.

## Related Protocol

The daemon protocol surface is documented in
[Daemon Protocol: Work Items](../v2/daemon-protocol.md#work-items).
