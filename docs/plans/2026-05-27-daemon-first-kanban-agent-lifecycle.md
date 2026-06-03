# Daemon-First Kanban Agent Lifecycle

## Summary

Make Kanban card planning, starting, implementation, review, decisions, and
run history daemon-owned. The frontend should render and control this lifecycle,
but it should not infer lifecycle transitions from local UI state or optimistically
move cards after starting a session.

The core product contract is:

- A card can start as a rough idea.
- Project/repo and agent profile can be optional for draft/imported cards.
- Cards missing required config are visible but not startable.
- Planning is an optional first-class card action with its own daemon run/session.
- Starting implementation creates or reuses a dedicated card worktree, creates a
  session, dispatches a daemon-generated prompt, and only then moves the card to
  **In Progress**.
- If prompt dispatch fails, the card stays in **Todo** or **Ready**, records a
  visible error, and keeps any created session/worktree for debugging.
- One card can have many historical runs, but only one active implementation run.
- Agent completion moves the card to **In Review**. A human accepts the work and
  moves it to **Done**.

## Lifecycle

Use these primary card states:

- `todo`: rough or unstarted work.
- `ready`: planned/configured work that can be started.
- `doing`: implementation run is active or owns an in-progress worktree/session.
- `review`: agent has claimed completion and the card needs human or automated
  review.
- `done`: accepted work.

Use side-state badges derived from daemon state rather than extra columns:

- `not_startable`: missing project/repo or autonomous agent profile.
- `planning`: a planning run is active.
- `blocked`: a run is waiting on one or more unresolved decisions.
- `start_failed`: start attempted but failed before prompt dispatch completed.
- `stopped`: latest implementation run was explicitly stopped.

## Daemon Ownership

The daemon owns all durable/runtime behavior for Kanban:

- Card readiness and lifecycle transitions.
- Planning and implementation run records.
- Worktree creation, reuse, and binding to cards/runs.
- Session/PTY creation and binding to runs.
- Prompt generation and PTY prompt dispatch.
- Run events and audit history.
- Structured decision prompts and timeout/default handling.
- Completion/review transitions.
- Cleanup policy for cards, runs, sessions, PTYs, and worktrees.

The frontend owns:

- Board, card, and detail rendering.
- Opening terminals for daemon sessions.
- Presenting configuration, decision, delete, restart, and cleanup prompts.
- Local pane/window layout.

Tauri commands stay thin and route to daemon capabilities when connected. Local
fallback should not create split-brain ownership for cards, runs, sessions, or
worktrees.

## Card Configuration

Cards can be created without complete execution config. This supports imported
issues, rough ideas, and planning-first workflows.

Recommended fields:

- `project_id`: optional at creation; required before implementation start.
- `repo_path`: derived from project where possible; required before start.
- `agent_profile`: optional at creation; required before start and must be an
  autonomous agent-capable profile.
- `base_branch`: optional; defaults to the project's default branch.
- `worktree_id` or `worktree_path`: absent until implementation start creates or
  attaches a dedicated worktree.
- `latest_session_id`: display/open-terminal state only, not lifecycle truth.
- `readiness`: daemon-derived summary explaining whether the card can be started.

Plain shell and type-only profiles are not valid for implementation Start. They
may still be valid for manual sessions outside the autonomous Kanban workflow.

## Planning Runs

Planning is a first-class run kind, separate from implementation.

`work-item-plan` should:

1. Validate whatever context is available.
2. Create or reuse one active planning run for the card.
3. Create a planning session.
4. Generate a planning prompt in the daemon.
5. Dispatch the prompt to the session.
6. Record run events for session creation and prompt dispatch.

Planning should help convert a rough card into a startable card by producing:

- Problem statement.
- Acceptance criteria.
- Implementation notes.
- Likely files/areas.
- Risks.
- Test plan.
- Suggested project/repo, agent profile, and base branch.

Planning does not move the card to **In Progress**. When accepted, the card should
become `ready` if required configuration is present.

## Implementation Start

`work-item-start` is the transactional daemon command for autonomous work.

It should:

1. Load the card and verify there is no active implementation run.
2. Validate the card has a project/repo and autonomous agent profile.
3. Resolve the base branch, defaulting to the project default branch.
4. Create or reuse the card's dedicated worktree.
5. Create a new implementation `WorkItemRun`.
6. Create a daemon session/PTY for the selected agent profile in the worktree.
7. Generate the structured implementation prompt in the daemon.
8. Dispatch the prompt to the PTY.
9. Append audit events for each successful step.
10. Move the card to `doing` only after prompt dispatch succeeds.
11. Return the updated card, run, session binding, and any derived readiness data.

If any step before prompt dispatch fails, the daemon should:

- Leave the card in `todo` or `ready`.
- Mark the run as `failed` if a run record exists.
- Record a visible `start_failed` reason.
- Keep any created session/worktree for inspection.
- Return enough information for the frontend to show retry and open-terminal
  actions when applicable.

## Worktree Policy

Each card should get a dedicated implementation worktree by default.

Defaults:

- Branch name is generated from the card, for example
  `roux/card-<short-id>-<slug>`.
- Base branch defaults to the project's default branch.
- Retry uses the same worktree.
- Restart uses the same worktree unless the user explicitly chooses
  **Restart Fresh**.
- **Restart Fresh** creates a new worktree/branch and preserves old run history.

The daemon should be the only component that mutates the card-to-worktree binding.

## Active Run Rules

Each card can have:

- Many historical planning runs.
- Many historical implementation runs.
- At most one active planning run.
- At most one active implementation run.
- No simultaneous planning and implementation run by default.

If a user tries to start while an active run/session exists, the frontend should
ask the user what to do instead of silently reusing state. Suggested options:

- Open existing terminal.
- Restart in same worktree.
- Restart fresh.
- Cancel.

## Prompt Contract

The daemon generates prompts so the autonomous-work contract is centralized and
testable.

Implementation prompts should include:

- Card title and description.
- Acceptance criteria.
- Project/repo/worktree path.
- Base branch and branch naming expectations.
- Whether to commit changes.
- Testing expectations.
- Decision-prompt protocol.
- Completion/reporting expectations.
- Instruction to request review instead of moving directly to done.

Planning prompts should focus on clarifying and refining the card, not editing
the repo unless explicitly allowed.

## Review Flow

Agent completion should request review, not final completion.

When the agent claims completion:

1. The daemon records a completion/review-request event.
2. The implementation run moves to a review-requested/completed state.
3. The card moves to `review`.
4. The session/PTY remains available.
5. The frontend shows **Open Terminal** as the primary action and review actions
   such as Accept, Request Changes, Restart, and Stop/Cleanup.

Future automated review can run as another daemon-owned run kind tied to the
same card and worktree.

## Decision Prompts

Structured decisions are first-class daemon state. They can block planning,
implementation, or review automation.

Decision prompts should support:

- Card/run association.
- Question text.
- Structured choices.
- Optional default choice.
- Optional timeout.
- Persisted resolution.
- Audit history.
- Resume behavior that writes the chosen value back to the linked session.

When unresolved required decisions exist, the card shows `blocked` and the active
run remains blocked until all required decisions are resolved or time out.

## Frontend Actions

The card primary action should be derived from daemon state:

- Missing config: **Configure**.
- Startable but no active session: **Start**.
- Planning active: **Open Planning Terminal**.
- Implementation active or latest session exists: **Open Terminal**.
- Review requested: **Review** or **Open Terminal**, depending on card layout.

Cards with active or historical sessions should expose a split action menu:

- Stop.
- Restart.
- Restart Fresh.
- Mark Ready.
- Request Review.
- Accept Done.
- Delete.

Delete should always ask what to do with associated runtime resources:

- Delete card only.
- Delete card and archive sessions.
- Delete card and cleanup sessions/PTYs/worktrees where safe.

## Public Interfaces

Expected daemon/socket additions or refinements:

- `work-item-plan`
- `work-item-start`
- `work-item-run-restart`
- `work-item-run-restart-fresh`
- `work-item-review-accept`
- `work-item-review-request-changes`
- `work-item-readiness`

Expected model additions or refinements:

- `WorkItem.status`: include `ready` and `review`.
- `WorkItem.readiness`: daemon-derived startability/config status.
- `WorkItemRun.kind`: `planning | implementation | review`.
- `WorkItemRun.status`: include `starting`, `running`, `blocked`, `failed`,
  `stopped`, `completed`, and `review_requested` as needed.
- `WorkItemRun.session_id`.
- `WorkItemRun.worktree_path` or stable worktree reference.
- `WorkItemRun.events`.
- `WorkItemDecision` tied to card and run.

When protocol payloads change, update the daemon protocol docs, CLI callers,
MCP tools, Tauri daemon client, frontend stores, and generated bindings together.

## Implementation Phases

1. Model readiness and statuses in the daemon without changing UI behavior.
2. Make `work-item-start` enforce the daemon-owned start transaction and stop
   frontend optimistic status moves.
3. Add dedicated worktree creation/reuse for implementation starts.
4. Add planning runs and planning prompt generation.
5. Add review state and completion-to-review behavior.
6. Expand frontend card actions around daemon-derived readiness/run state.
7. Add cleanup/delete prompts for sessions, PTYs, and worktrees.
8. Add automated review as a later daemon-owned run kind.

## Test Plan

Daemon/Rust tests:

- Card without project/repo is not startable.
- Card without autonomous agent profile is not startable.
- Plain shell/type-only profile cannot start implementation.
- Start creates a run/session/worktree and moves card to `doing` only after
  prompt dispatch succeeds.
- Prompt dispatch failure leaves card in `todo` or `ready`, records an error,
  and preserves created resources.
- Active implementation run prevents another silent start.
- Retry reuses the same worktree.
- Restart Fresh creates a new worktree and preserves previous run history.
- Agent completion moves card to `review`, not `done`.
- Decision resolution writes the selected value to the linked session and records
  an audit event.

Frontend/store tests:

- Missing config renders Configure/not-startable state.
- Start button is hidden or disabled for non-startable cards.
- Start success renders Open Terminal and `doing`.
- Start failure renders an error icon and keeps the card out of `doing`.
- Active run shows split actions.
- Review state renders review actions.
- Delete confirmation exposes card/session/PTY/worktree cleanup choices.

Manual verification:

- Start from a ready card kicks off the agent without extra typing.
- Closing and reopening Roux preserves card/run/session/worktree truth.
- Opening terminal for an active card attaches to the daemon-owned session.
- Planning session can refine a draft card without starting implementation.
- Completion leaves the terminal available and moves the card to review.

## Open Questions

- Exact persisted shape for readiness: stored field, derived response, or both.
- Whether `ready` should be a first-class board column immediately or a badge in
  `todo` for the first iteration.
- How automated review should report findings and decide whether to request
  changes.
- Cleanup policy for old dedicated worktrees after a card is accepted as done.
