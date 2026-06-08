# Kanban Board

Roux's Kanban board is a project-scoped work queue for agent tasks. Cards are
durable work items stored by the daemon; terminal panes only render and attach
to the daemon-owned run state.

## Cards and Runs

Each card can have many runs. Cards can be created as drafts without a repo or
agent profile. Cards without a repo/project show **Configure** until they have
enough daemon-owned start config; cards without an explicit agent use the
Kanban default agent profile from settings.

Starting a configured card is daemon-owned. The daemon creates or reuses the
card's dedicated worktree, creates a session/PTY for the selected autonomous
agent profile, launches the agent with the generated task prompt as its initial
prompt, and only then moves the card to **In Progress**. The card's
`session_id` is latest-session display state only.

Planning is daemon-owned too. **Plan** creates a planning session in the repo
workspace and launches the agent with the planning prompt without moving the
card or binding its implementation session. If the planning run is stale,
**Retry planning** stops the active planning run and starts a fresh one.

Settings -> Kanban controls the default autonomous agent profile, workflow
stage labels, stage action button labels, optional phase/stage instructions,
and the sidebar opened at launch.

Settings -> Kanban can also point at a workflow JSON file. Use the settings
panel to browse to a JSON file, validate the current path, reveal Roux's config
directory, copy a starter `kanban-workflow.json` there, or save the currently
authored workflow back to the selected JSON path. Relative paths resolve from
Roux's config directory, next to `settings.json`.

The board always uses the fixed high-level columns `todo`, `planning`,
`doing`, `review`, and `done`. Workflows group concrete stages inside those
columns. The bundled workflow includes `todo`, `planning`, `implementation`,
`fix_ci`, `local_review`, `pr_review`, and `done`. Stages can define labels,
short button labels, phase/stage instructions, agent runners, command runners,
manual gates, command gates, environment, and explicit transitions.

See [example Kanban workflow JSON](../examples/kanban-workflow.json).

Cards show their current workflow stage and a stage action button. Agent stages
start the configured agent run. Manual stages and gates complete immediately
from the UI. Command stages and command gates run in the daemon, record stdout
and stderr previews in run history, and block loudly on failure so the workflow
JSON can be fixed.

After a card has an active or previous run, the card shows **Open terminal**
instead of **Start**. Opening the terminal attaches to the latest linked session;
it does not create another run by itself. Starting again creates a separate run
history entry rather than overwriting the prior attempt.

If start fails before prompt dispatch completes, the card stays in **Todo** or
**Planning**, records a visible `startError`, and preserves any created
session/worktree for inspection or retry.

Run history is persisted under the card and survives closing and reopening Roux.
PTY exit updates the run lifecycle: exit code `0` on an implementation run moves
the run and card to **Review**; non-zero or unknown exits mark the run `failed`.
Explicitly stopping a run marks it `stopped` and does not get overwritten by a
later PTY exit. Accepting review is a daemon command that moves both the
reviewed run and card to **Done**. Requesting changes attaches the human
feedback to the card, marks the reviewed run `changesRequested`, clears the
card's active session binding, and moves the card back to **In Progress** by
default. The next implementation prompt includes the latest review feedback.

## Decisions

Agents should ask for human choices through the Roux decision command:

```bash
roux work-item decision create <run-id> "Which path should I take?" \
  --option existing="Use existing code" \
  --option new="Create a new path" \
  --default-value existing \
  --timeout-seconds 86400
```

Roux persists those prompts as `WorkItemDecision` rows under the run, marks the
run blocked, and shows the decision on both the card and detail view.

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
roux work-item plan <card-id>
roux work-item move <card-id> planning
roux work-item start <card-id>
roux work-item review request <run-id> --summary "Implemented retry coverage" --test "npm run test" --changed-file src/lib/retry.ts
roux work-item review request-changes <run-id-or-card-id> --note "Add the missing retry coverage"
roux work-item review accept <card-id>
roux work-item accept <card-id>
roux work-item runs --work-item <card-id>
roux work-item events <run-id>
roux work-item decision create <run-id> "Question?" --option yes=Yes --option no=No
roux work-item decision list --work-item <card-id>
roux work-item decision resolve <decision-id> <value>
```

`roux kanban ...` is a visible alias for `roux work-item ...`.

Plans and handoffs that should stay attached to a card can be stored as Roux
documents:

```bash
roux document attach --work-item <card-id> --title "Plan" --file ./plan.md --mime-type text/markdown
roux document list --work-item <card-id>
roux document get <document-id>
```

The returned `documentId` is stable and can be retrieved from another session
or MCP client. Use card documents for reusable plan snapshots and references;
use run events for chronological execution history.

The MCP server exposes the same daemon-backed board surface with tools such as
`roux_list_work_items`, `roux_create_work_item`, `roux_plan_work_item`,
`roux_start_work_item`, `roux_run_work_item_stage`,
`roux_accept_work_item_review`,
`roux_request_work_item_review`, `roux_request_work_item_review_changes`,
`roux_list_work_item_runs`, and `roux_resolve_work_item_decision`. It also
exposes document tools:
`roux_attach_document`, `roux_list_documents`, and `roux_get_document`.

## Related Protocol

The daemon protocol surface is documented in
[Daemon Protocol: Work Items](../v2/daemon-protocol.md#work-items).
