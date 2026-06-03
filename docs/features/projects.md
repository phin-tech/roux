# Projects

A **project** groups related sessions across repositories, worktrees, and spawn profiles. Use projects when a piece of work is larger than one checkout: a product area, client engagement, release train, or feature that spans several repos.

Projects are metadata on top of sessions. Deleting a project does not delete sessions or files; sessions that belonged to it are simply untagged.

## Creating a Project

Open **New Project** from the sidebar or command palette.

A project can include:

- **Name** — the label shown in the sidebar, command palette, notes, and session rows.
- **Repos** — one or more repository roots that belong to the project.
- **Session blueprints** — saved session templates that can be spawned later.
- **Project prompt** — extra instructions appended when supported agent profiles start.
- **Context paths** — files or directories exposed to spawned PTYs through an environment variable.

If you configured repository roots in Settings, the repo picker searches discovered git repos under those roots. You can also type or browse to a path manually.

## Session Blueprints

A **session blueprint** is a saved project session. It records the repo, optional worktree branch, base ref, fetch behavior, and spawn profile.

Use the **Defaults** section in the project dialog to generate one blueprint per repo. The name template supports:

| Token         | Meaning                   |
| ------------- | ------------------------- |
| `{{project}}` | Project name              |
| `{{repo}}`    | Repository folder name    |
| `{{branch}}`  | Worktree branch, when set |

Generated rows can be edited before saving. You can also add rows manually.

## Spawning Project Sessions

Project sessions can be started from:

- The sidebar, when sessions are grouped by **project**.
- The command palette command **Spawn Project Session**.

When a blueprint is spawned, Roux creates a session using the blueprint values and tags it with the project. If the blueprint includes a branch, Roux creates or uses the corresponding worktree according to the same worktree rules as New Session.

The sidebar suppresses the blueprint row while its live session is active, so the project group shows the running session instead of a duplicate launch row.

## Project Prompts

The project prompt is free-form text injected when supported agent profiles start. It can include Minijinja variables for the model family, worktree path, branch, same-project sessions, and other spawn-time context.

Supported providers:

| Provider | Startup injection        |
| -------- | ------------------------ |
| Claude   | `--append-system-prompt` |
| Codex    | `-c instructions=...`    |

Other profiles are left unchanged. The prompt is still exposed as `ROUX_PROJECT_PROMPT`, so custom profiles can opt into it manually.

Prompt injection happens at spawn time. Changing a project prompt does not rewrite already-running agent commands; spawn a new session to pick up the new prompt.

See [Project Prompt Templates](project-prompt-templates.md) for the variable reference, examples, preview behavior, and template error handling.

## Context Paths

Context paths are project-level files or directories that agents may need to inspect. Roux exposes them to spawned PTYs as `ROUX_PROJECT_CONTEXT_PATHS`, encoded as the platform path list for the current OS.

For example, a project can point agents at:

- a product brief
- a design doc directory
- a shared API schema
- a repo-local runbook

Roux does not automatically read or paste these files. It only gives the spawned process a stable pointer so agents, shell scripts, and custom profiles can decide how to use them.

## Environment Variables

Project-tagged PTYs include project context when Roux can resolve it:

| Variable                     | Description                              |
| ---------------------------- | ---------------------------------------- |
| `ROUX_PROJECT_ID`            | Internal project id for the session      |
| `ROUX_PROJECT_PROMPT`        | Project prompt text, when configured     |
| `ROUX_PROJECT_CONTEXT_PATHS` | Project context paths as an OS path list |

Notes also expose project-scoped note variables when the session has a project. See [Notes](notes.md#environment-variables).

Environment variables are snapshots taken when the PTY starts. If you retag a running session or edit project settings, restart the shell or spawn a fresh session to get updated values.

## Editing and Deleting

Use **Edit Project** from the sidebar project group or command palette to change repos, blueprints, prompts, and context paths.

Use **Delete Project** from the command palette to remove the project record. Sessions remain in Roux, but their project tag is removed and project-scoped notes/prompts no longer apply to those sessions.

## See Also

- [Sessions](sessions.md) — session lifecycle, grouping, reconnect, and history
- [Worktrees](worktrees.md) — branch and worktree behavior
- [Notes](notes.md) — project-scoped markdown notes
- [Layouts](layouts.md) — pane templates for session startup
