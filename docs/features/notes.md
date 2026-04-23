# Notes

Roux keeps a scoped, Obsidian-compatible markdown vault for your sessions, repos, projects, and yourself. Notes are plain text on disk, scriptable from the CLI, and exposed to agents through environment variables.

!!! warning "Experimental — subject to change"
    Multi-scoped notes are in active development. Vault layout, CLI flag shapes, frontmatter schema, and environment variable names may change in future releases before stabilizing. If you point Obsidian at your Roux vault, keep your own backups and expect breaking migrations until this banner is removed.

## Scopes

Every session can read or write notes in four scopes:

- **Global** — your personal catch-all. Shared across every session, every repo, every project.
- **Project** — shared by every session assigned to the same Roux project. Useful when a piece of work spans multiple repos.
- **Repo** — shared by every session working in the same repo, regardless of project or worktree.
- **Session** — scoped to one session. The session folder survives forever, even after the session is deleted in Roux.

## Opening the panel

Press ++cmd+b++ to toggle the notes sidebar. The header shows a row of pills:

```
[Global] [Project] [Repo] [Session]
```

Click a pill to switch scope. The Project pill is greyed out when the current session has no project assigned. Your last-selected scope is remembered per session.

The panel is deliberately a plain-text editor. For rich viewing — wikilinks, backlinks, graph view, search — open the vault in Obsidian (see below).

## Notes panes

Roux also supports notes as a real pane type, not just the sidebar panel.

Open one from the command palette:

- **Open Notes Pane (Horizontal)**
- **Open Notes Pane (Vertical)**

A notes pane can live beside terminals, markdown docs, and other notes panes inside the normal split tree.

This is useful when you want notes visible next to a live shell, command pane, or agent session without replacing the sidebar.

### Pane-local scope and mode

Each notes pane keeps its own:

- scope (`session` / `repo` / `project` / `global`)
- view mode (`edit` / `read`)

That means two notes panes in the same session can show different scopes at the same time.

The available pane commands are:

- **Notes: Session Scope**
- **Notes: Repo Scope**
- **Notes: Project Scope**
- **Notes: Global Scope**
- **Notes: Toggle Edit/Read**

The **Project** scope is only available when the current session has a project assigned.

### Sidebar notes vs notes panes

The sidebar notes panel and notes panes use the same underlying notes files, but they behave differently:

- the sidebar is a global side panel for the active session
- a notes pane is part of the split layout and can remain visible beside terminals
- sidebar scope selection is remembered per session
- notes panes remember scope and mode per pane

If you just need a quick scratchpad, use ++cmd+b++. If you want notes to stay onscreen as part of the layout, open a notes pane.

## Where notes live

Notes are stored as regular markdown files in an Obsidian-compatible vault:

```
~/Documents/Roux/                     # configurable in Settings
├── global/notes.md
├── projects/<project-slug>/notes.md
├── repos/<repo-slug>/notes.md
└── sessions/<branch-slug>--<short-id>/
    └── notes.md
```

The vault is a clean Obsidian vault: just markdown files with YAML frontmatter. Point Obsidian at `~/Documents/Roux` (or your configured location) and every note is readable, searchable, and link-able with `[[wikilinks]]`. You can also add your own `.md` files anywhere in the vault — Roux doesn't surface them in the panel (yet), but they're full citizens of the vault.

## Environment variables

Every PTY Roux spawns gets the following variables so you and your agents can find the right file without guessing:

| Variable | Points to |
|---|---|
| `ROUX_NOTES_ROOT` | Vault root |
| `ROUX_GLOBAL_NOTES_FILE` | `<root>/global/notes.md` |
| `ROUX_GLOBAL_NOTES_DIR` | `<root>/global` |
| `ROUX_REPO_SLUG` | Slug for the current session's repo |
| `ROUX_REPO_NOTES_FILE` / `_DIR` | Repo scope file and directory |
| `ROUX_SESSION_PROJECT` | Project slug (unset if no project) |
| `ROUX_SESSION_PROJECT_NOTES_FILE` / `_DIR` | Project scope file and directory (unset if no project) |
| `ROUX_SESSION_DIR` | Session scope directory |
| `ROUX_SESSION_NOTES_FILE` | `<session-dir>/notes.md` |

These are snapshots at PTY spawn time; restart the shell if you reassign the session to a different project.

## Command-line usage

Every scope has the same verbs:

```sh
roux notes session show
roux notes session append "just shipped the TLS fix"
roux notes session append --timestamp "retried, still failing"
roux notes repo path
roux notes global show

# Target a specific topic file instead of the scope's notes.md anchor:
roux notes repo append --topic api-gotchas "handshake fails when SNI is missing"
roux notes repo append --topic api-gotchas --tag api --tag tls

# Find notes by tag (hierarchical prefix match by default):
roux notes search --tag api
roux notes search --tag api --scope repo
```

See `roux notes --help` for the full command list.

## Topic files and tags

The scope anchors (`notes.md`) are deliberately simple. For longer-lived, topic-organized knowledge, create **topic files** — any `.md` file inside a scope's directory. Write them by hand in Obsidian, or via the CLI with `--topic <name>`.

Tags live in frontmatter and in inline `#tag` text, same as in Obsidian. `roux notes search --tag` returns the union of both. Every Roux-written file defaults to the hierarchical tag `roux/<scope>` — so searching `#roux` in Obsidian surfaces every Roux-authored note, and `#roux/session` narrows to one scope.

## Timestamped entries

Add `--timestamp` to `append` to turn it into the "add a log entry" primitive:

```markdown
<a id="entry-a1b2c3d4"></a>

## 2026-04-18 14:30

retried after clearing token cache, still 401

^entry-a1b2c3d4
```

Each entry gets a stable 8-char id embedded two ways:

- `^entry-<id>` is an **Obsidian block reference**. Link to it from any other note with `[[file#^entry-<id>]]`.
- `<a id="entry-<id>">` is a plain HTML anchor. If you ever publish the vault as a static site (Quartz, Hugo, Zola, MkDocs, etc.), `#entry-<id>` is a deep link that Just Works.

Disable the HTML anchor in Settings (`Include web anchors for entries`) if you prefer cleaner raw markdown and don't plan to publish.

## Opening the vault in Obsidian

Roux's vault is a plain Obsidian vault. Open the folder at `ROUX_NOTES_ROOT`:

```sh
open -a Obsidian "$ROUX_NOTES_ROOT"
```

No plugins or configuration required. Block refs, wikilinks, frontmatter tags, and Dataview all work out of the box.

## Publishing (optional)

The vault is also compatible with static-site generators that read Obsidian vaults — Quartz is the most batteries-included choice, but Hugo, Zola, MkDocs, and Eleventy all work with minor config. If you publish, **configure a publish filter** on the generator side (e.g. require `publish: true` in frontmatter) so private notes stay private. Roux does not ship any publishing tooling in v1.

## When to use notes

- **Global** — principles, reusable prompts, opinions, commands you always forget.
- **Project** — cross-repo plans, stakeholders, decisions, a running log.
- **Repo** — repo-wide gotchas, architecture notes, onboarding tips for future you.
- **Session** — the in-flight scratchpad: what you were trying to do, what worked, what to pick up next time.

For long-form documentation that ships with code, commit a file to the repo itself. The vault is for your thinking.
