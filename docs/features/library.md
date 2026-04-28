# Library

The Library is a scoped collection of reusable **prompts** and **skills**. Prompts can be rendered with variables and sent directly into the focused terminal. Skills are reusable markdown context blocks you can send to an agent or copy to the clipboard.

!!! warning "Experimental — subject to change"
    Library prompts and skills require Roux `v0.5.3-pre.1` or greater (skill sync requires `v0.5.3-pre.3` or greater) and are in active development. Vault layout, source layering, frontmatter schema, variable syntax, Git source handling, sync manifest format, and command shapes may change in future releases before stabilizing. If you keep important prompts or skills in your Roux vault or shared Library repos, keep your own backups and expect breaking migrations until this banner is removed.

## Where Library items live

Roux reads Library markdown from layered sources:

- **Global Library** — `library/prompts` and `library/skills` inside your Roux vault.
- **Active Repo Library** — `.roux/library/prompts` and `.roux/library/skills` inside the active session repo.
- **Local repo sources** — pinned repositories that expose `.roux/library`.
- **Git sources** — managed checkouts that Roux can clone, status-check, and sync.

Sources are ordered. Later sources override earlier sources when they define the same item id, so a repo-specific prompt can replace a global default without deleting it.

## Item format

Library items are markdown files with YAML frontmatter. Prompt files live under `prompts`; skill files live under `skills`.

Prompts:

```markdown
---
id: review.diff
type: prompt
title: Review Diff
description: Review a change with a specific concern in mind
tags: [review]
variables:
  - name: goal
    label: Review focus
    default: correctness
---
Review this diff for {{ goal }}.
```

Skills (also valid as Claude `SKILL.md` — see [Skill sync](#skill-sync)):

```markdown
---
name: rust.errors
id: rust.errors
type: skill
title: Rust Errors
description: Prefer typed errors over Result<_, String>
tags: [rust]
---
Body...
```

The `id`, `type`, and `title` fields identify the item in Roux. **Prompts** support variables (`{{ variableName }}` placeholders, strings/integers/floats/selects); Roux prompts for values before sending. **Skills do not support variables** — they are pure context blocks. The `name` field on skills must equal the `id` and is required so the file is loadable as a Claude `SKILL.md` when [skill sync](#skill-sync) is enabled.

## Using the Library

Open the Library from the activity rail, the command palette, or the leader Library tree. From there you can:

- filter prompts and skills
- preview rendered markdown
- create or edit Library items
- send an item to the focused terminal
- drag a prompt onto a terminal pane
- copy prompts or skills to the clipboard from the command palette
- add, remove, reorder, enable, disable, clone, or sync Library sources

Prompt sends append a carriage return, so the target terminal receives the rendered prompt as submitted input. Skills send their markdown body as context.

## Keyboard and command palette

Default Library shortcuts:

- ++cmd+alt+p++ — search Library prompts and send to the active pane
- ++cmd+alt+s++ — search Library skills and send to the active pane
- ++cmd+alt+shift+p++ — copy a Library prompt to the clipboard
- ++cmd+alt+shift+s++ — copy a Library skill to the clipboard
- ++cmd+; l++ — open the leader Library tree

The same actions are available from ++cmd+k++ under the **Library** category, including:

- **Sync Library Skills** — run a sync pass (see below)
- **Unsync All Library Skills** — remove all synced files (hash-checked)

## Skill sync

Library skills can be mirrored into Claude-readable `.claude/skills/<name>/SKILL.md` directories so Claude loads them automatically. Sync is **off by default**.

**Modes** (set globally in the Library panel under "Skill sync", or per-source via `librarySources[].skillSync` in `settings.json`):

- `off` — Roux does not write any skill files outside the Library.
- `copy` — Roux writes a copy of each skill on sync. A manifest at `<vault>/library/.skill-sync.json` records SHA-256 hashes so Roux can detect when a synced file has been edited locally and skip overwriting it.
- `symlink` — Roux symlinks each `.claude/skills/<name>/SKILL.md` back to the source skill file. Edits in either place are the same edit. On Windows without Developer Mode the OS rejects symlink creation; Roux auto-degrades to `copy` for that sync run.

**Destinations** are determined by the source layer:

- Global vault and Git-repo sources sync to user-level `~/.claude/skills/`.
- Local-repo and active-repo sources sync to that repo's own `.claude/skills/`.

**Conflict policy.** Sync is conservative: it never overwrites a file at the destination that Roux did not write. Untracked files and files whose content has drifted from the manifest hash are skipped and reported, never modified. For copy-mode files inside a git repo, treat git as the safety net — the synced file is part of your tree and `git diff` will show any drift.

**Removal.** Disabling a source or removing a skill from the Library marks the corresponding manifest entry as **stale** in the next sync report. To delete the synced files, run **Unsync All Library Skills** from the command palette. Unsync hash-checks every entry first and refuses to delete files that have been edited locally.

**One-time format migration.** On first launch after upgrading to skill sync, Roux silently rewrites global-vault skills that lack a `name:` field and strips any legacy `variables:` blocks. Repo and Git-source skills are migrated implicitly the next time they are saved through the Library editor. The migration leaves `{{ variable }}` placeholders in skill bodies untouched — review and clean them up by hand if any existed.
