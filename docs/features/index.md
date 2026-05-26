# Features

Roux is a terminal-first workspace for running agent sessions, shells, notes, watches, and reusable context side by side. These are the major feature areas.

## Workspace

- [Panes](panes.md) — split, stack, focus, attach, detach, and run commands.
- [Multiline Editor](editor.md) — docked terminal input editor with selection seeding, command corrections, and context chips.
- [Sessions](sessions.md) — create, reconnect, archive, restore, and group agent workflows.
- [Projects](projects.md) — group sessions across repos, save session blueprints, and inject project context.
- [Kanban Board](kanban.md) — plan card-based agent work, start daemon-owned runs, and resolve blocked decisions.
- [Project Prompt Templates](project-prompt-templates.md) — Minijinja variables for branch, worktree, model, and sibling-session context.
- [Layouts](layouts.md) — start sessions from KDL templates and spawn profiles.
- [Worktrees](worktrees.md) — manage isolated git checkouts for session work.
- [Smol Machines](smol-machines.md) — run sessions inside local libkrun / KVM VMs for OS-level isolation.

## Context

- [Library](library.md) — reuse scoped prompts and skills from global, repo, local-source, and Git-backed libraries.
- [Notes](notes.md) — keep scoped markdown notes beside your sessions.
- [Automation hooks](hooks.md) — integrate Roux with external events and scripts.

## Automation

- [Notifications](notifications.md) — in-app inbox, unread badges, OS notification fan-out, and agent setup.
- [Mailbox & Bus](mailbox.md) — addressable mail between agents and the human, plus topic-based broadcasts. Aliases bind to panes; auto-claim from pane name.
- [Watches](watches.md) — track long-running checks, HTTP endpoints, and GitHub PRs.
- [CLI](cli.md) — drive sessions, panes, notes, notifications, and automation from `roux`; includes the standalone CLI crate layout and experimental `roux daemon`.
- [Keymap](keymap.md) — customize direct shortcuts, leader chords, and command bindings.
