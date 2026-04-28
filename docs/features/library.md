# Library

The Library is a scoped collection of reusable **prompts** and **skills**. Prompts can be rendered with variables and sent directly into the focused terminal. Skills are reusable markdown context blocks you can send to an agent or copy to the clipboard.

!!! warning "Experimental — subject to change"
    Library prompts and skills require Roux `v0.5.3-pre.1` or greater and are in active development. Vault layout, source layering, frontmatter schema, variable syntax, Git source handling, and command shapes may change in future releases before stabilizing. If you keep important prompts or skills in your Roux vault or shared Library repos, keep your own backups and expect breaking migrations until this banner is removed.

## Where Library items live

Roux reads Library markdown from layered sources:

- **Global Library** — `library/prompts` and `library/skills` inside your Roux vault.
- **Active Repo Library** — `.roux/library/prompts` and `.roux/library/skills` inside the active session repo.
- **Local repo sources** — pinned repositories that expose `.roux/library`.
- **Git sources** — managed checkouts that Roux can clone, status-check, and sync.

Sources are ordered. Later sources override earlier sources when they define the same item id, so a repo-specific prompt can replace a global default without deleting it.

## Item format

Library items are markdown files with YAML frontmatter. Prompt files live under `prompts`; skill files live under `skills`.

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

The `id`, `type`, and `title` fields identify the item in Roux. Prompt variables use `{{ variableName }}` placeholders. Variables can be strings, integers, floats, or selects; Roux prompts for values before sending the rendered prompt.

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

The same actions are available from ++cmd+k++ under the **Library** category.
