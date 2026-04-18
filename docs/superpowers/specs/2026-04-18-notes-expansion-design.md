# Multi-Scoped Notes & Obsidian-Compatible Vault

**Date:** 2026-04-18
**Status:** Approved

## Problem

Today Roux has a single per-project plain-text scratchpad (`roux_config_dir()/notes/<project_id>.txt`, rendered by `NotesPanel.svelte`). It's tied to projects, sessions get nothing, repos get nothing, and there is no "roux-level" catch-all. Notes are invisible to agents running inside sessions, and the storage format is a dead end — it can't grow into a second-brain surface the user actually leans on.

## Goals

- Four note scopes available to every session: **global**, **project**, **repo**, **session**.
- Storage is a single on-disk directory shaped as a clean Obsidian vault (markdown + YAML frontmatter, no sidecar state files). A user can point Obsidian, Quartz, or any markdown tool at the folder and it Just Works.
- Notes are reachable from agents via the `roux` CLI and via environment variables injected into every PTY.
- External edits (Obsidian, `$EDITOR`) are first-class — the Roux panel reloads when the file changes on disk.
- Plain-text editing in the panel. No markdown preview, no rich-text editor.
- The existing project notes migrate automatically on first launch; no data loss.

## Non-Goals

- Multi-file browsing inside the Roux panel. The panel surfaces exactly four `notes.md` files (one per scope). Users manage additional `.md` files via Obsidian or the filesystem. (Future v2.)
- Markdown rendering / preview in the panel.
- In-panel search, graph view, or backlinks. Obsidian owns that surface.
- Conflict resolution for simultaneous unfocused edits. Last writer wins once both saves complete; no merge UI.
- Cross-session browsing from the panel (reading another session's notes). Use Obsidian or `roux notes` CLI.
- Integration with Obsidian CLI, Quartz, Hugo, Zola, or MkDocs. The vault is compatible with those by construction; we do not ship tooling or config for them.
- Daily-notes, templates, or note creation helpers beyond the four anchor files (v2 CLI: `roux notes <scope> new <name>`).
- Auto-archive or auto-delete of session notes. Files persist indefinitely.
- Per-entry edit/delete-by-id operations (v2, once entries have stable ids).
- Collaborative editing, sync, or network storage. The vault is a local folder.

## Design

### Scopes & Vault Layout

Four scopes, one `notes.md` anchor file per scope. Session scope is a *folder* from day one so future siblings (transcript backups, logs) drop in without a filesystem move.

```
$ROUX_NOTES_ROOT/                       # default: ~/Documents/Roux (configurable)
├── global/
│   └── notes.md
├── projects/
│   └── <project-slug>/
│       └── notes.md
├── repos/
│   └── <repo-slug>/
│       └── notes.md
├── sessions/
│   └── <branch-slug>--<short-id>/
│       └── notes.md
└── .roux/
    ├── repos.json                      # { repo_id → slug, plus metadata }
    └── projects.json                   # { project_id → slug, plus metadata }
```

- `.roux/` holds Roux's internal index mapping internal ids to vault slugs. Obsidian ignores dot-prefixed folders by default, so this does not pollute the vault experience. The two index files are the **only** non-markdown state in the vault.
- Users may create any additional `.md` files anywhere in the vault (e.g. `repos/roux/api-gotchas.md`, `repos/roux/INDEX.md`, `projects/<slug>/specs/2026-04-18-feature.md`). Roux does not surface them in the panel in v1 but never deletes or overwrites them.
- The `specs/` subfolder under each scope is a **convention**, not a code-enforced path. Nothing in Roux reads or writes `specs/` directly; it's there for users and agents to park design docs they want alongside their notes.

### Identity & Slugs

**Repo slug** is derived once per distinct `repo_root`, cached in `.roux/repos.json`:

1. If the directory is a git repo and has an `origin` remote, slugify the URL (strip protocol, `.git`, replace non-alphanumerics with `-`, lowercase). Example: `git@github.com:phin-tech/roux.git` → `phin-tech-roux`.
2. Otherwise slugify the canonicalized absolute path's basename. Example: `/Users/sam/src/playground` → `playground`.
3. If the slug collides with an existing entry pointing to a different `repo_root`, append `-2`, `-3`, etc.
4. Slug is **frozen** after first write. Moving the clone, adding a remote later, or renaming the directory does not change the slug. Users rename via `roux notes repo rename <old> <new>` (moves the folder on disk and rewrites `repos.json`).

**Project slug** is slugified from the project's user-chosen name at project creation, stored in `.roux/projects.json`, frozen on creation. Renaming the project inside Roux does **not** rename the vault folder. `roux notes project rename <old> <new>` is the escape hatch.

**Session slug** is `<branch-slug>--<short-id>`:
- `<branch-slug>` = current branch at session creation, slugified. Falls back to `detached` for detached-HEAD sessions and `no-git` for non-git workdirs.
- `<short-id>` = first 6 hex chars of the session id.
- Frozen at session creation; does not follow branch renames.
- Session folders persist forever — Roux never auto-deletes them. Deleting a Roux session leaves its folder on disk.

### Frontmatter Schema

Every `notes.md` (and every Roux-generated file) starts with YAML frontmatter. `updated` is rewritten only when Roux itself writes the file; external writers (Obsidian, user) may or may not update it.

**Global** (`global/notes.md`):

```yaml
---
type: global
tags: [roux, global]
created: 2026-04-18T10:30:00-05:00
updated: 2026-04-18T10:30:00-05:00
---
```

**Project** (`projects/<project-slug>/notes.md`):

```yaml
---
type: project
project: <project-slug>
project_name: "Marketing Site Revamp"
tags: [roux, project]
created: 2026-04-18T10:30:00-05:00
updated: 2026-04-18T10:30:00-05:00
---
```

**Repo** (`repos/<repo-slug>/notes.md`):

```yaml
---
type: repo
repo: <repo-slug>
repo_path: /Users/sam/src/github.com/phin-tech/roux
remote: git@github.com:phin-tech/roux.git        # omitted if none
tags: [roux, repo]
created: 2026-04-18T10:30:00-05:00
updated: 2026-04-18T10:30:00-05:00
---
```

**Session** (`sessions/<branch-slug>--<short-id>/notes.md`):

```yaml
---
type: session
session_id: a1b2c3d4-e5f6-7890-abcd-ef1234567890
repo: phin-tech-roux
project: null                                     # or <project-slug>
branch: feature/session-notes
worktree: /Users/sam/src/worktrees/session-notes
tags: [roux, session]
created: 2026-04-18T10:30:00-05:00
updated: 2026-04-18T10:30:00-05:00
---
```

The `tags:` list enables Dataview queries (`FROM "" WHERE type = "session"`) and Obsidian tag-search out of the box. `type` duplicates a tag so either query style works.

### Timestamped Entry Format (`append --timestamp`)

`roux notes <scope> append --timestamp` turns append into the "add a comment" primitive, giving each entry a stable id for grep and deep-linking.

With `notes.includeWebAnchors = true` (default):

```markdown
<a id="entry-a1b2c3d4"></a>

## 2026-04-18 14:30

<content>

^entry-a1b2c3d4
```

With `notes.includeWebAnchors = false`:

```markdown
## 2026-04-18 14:30

<content>

^entry-a1b2c3d4
```

- `<a id="entry-<id>"></a>` is an inline HTML anchor. Preserved verbatim by every mainstream static site generator that respects inline HTML (Hugo, Zola, MkDocs, Jekyll, Eleventy, Astro, Quartz). Enables `#entry-<id>` deep links on any published vault, zero generator-specific config.
- `^entry-<id>` is Obsidian's native block reference. Lets any other note in the vault deep-link this entry with `[[notes#^entry-<id>]]`.
- `<id>` is 8 hex chars from a random UUID v4 (~4B entry space; collision-resistant for a personal log; short enough to be readable).
- The `^entry-<id>` block ref is unconditional — it's cheap, invisible in Obsidian reading view, and idiomatic. Only the `<a id>` is toggled.
- Both ids are generated once per append and embedded twice with the same value. Grep for `entry-a1b2c3d4` finds either form.
- Plain `append` (without `--timestamp`) is unchanged: leading newline + content + trailing newline. No heading, no id.

### UI — `NotesPanel.svelte`

The panel keeps its current geometry (right-docked sidebar, `Cmd+B` toggle) and grows a **scope pill row** in the header.

- Pills: `[Global] [Project] [Repo] [Session]`, rendered left-to-right in that order.
- The active pill is highlighted.
- The **Project** pill is disabled (greyed, tooltip "No project assigned") when the session has no project.
- Click a pill to switch scope. The panel reads the corresponding file and replaces the textarea contents.
- **Sticky per session.** The last-selected scope is persisted per `session_id` in frontend settings. On next panel open for the same session, the previously selected scope is re-selected. New sessions default to **Session**.
- Textarea below the pill row, unchanged visually — plain text, monospace, 500ms debounced save.
- If the scope's file does not exist yet (first-ever write), creation is lazy: the textarea shows empty content, and the file is materialized (with frontmatter) on the first debounced save.
- If the session has no associated repo (`repo_root` is empty — rare but possible), the Repo pill is disabled, same tooltip pattern as Project.

### Filesystem Watcher (external edits)

External editing is first-class. The backend runs a filesystem watcher over `$ROUX_NOTES_ROOT` for the lifetime of the app.

- Implementation: Tauri backend uses the `notify` crate (already a direct dep at version 7 in `src-tauri/Cargo.toml`), scoped to the vault root.
- Events debounced to ~250ms to collapse save bursts.
- On any file change inside the vault, the backend emits a `notes-changed` Tauri event with the full path of the changed file.
- `NotesPanel` listens; if the changed file matches the currently displayed scope's path, the panel re-reads the file and diffs against the textarea's last-saved content.
  - If the textarea is unchanged from the last save, replace content silently.
  - If the textarea has unsaved in-memory edits that differ from disk, show a non-modal banner: "File changed externally. [Reload] [Keep my edits]." Keep-my-edits continues with the current content; next debounced save overwrites disk.
- The watcher is reused for: v2 multi-file support, `roux notes` CLI writes that should immediately appear in the panel, and any future tool that writes to the vault.

### Environment Variables

Every PTY gets these in addition to the existing `ROUX_SESSION_ID` / `ROUX_PANE_ID` / `ROUX_PROJECT_ID` / `ROUX_WORKTREE_PATH` / `ROUX_CLI` / `ROUX_SOCKET`:

| Variable | Value | Example |
|---|---|---|
| `ROUX_NOTES_ROOT` | Vault root | `/Users/sam/Documents/Roux` |
| `ROUX_GLOBAL_NOTES_DIR` | Global scope dir | `<root>/global` |
| `ROUX_GLOBAL_NOTES_FILE` | Global anchor | `<root>/global/notes.md` |
| `ROUX_REPO_SLUG` | Current session's repo slug | `phin-tech-roux` |
| `ROUX_REPO_NOTES_DIR` | Repo scope dir | `<root>/repos/<slug>` |
| `ROUX_REPO_NOTES_FILE` | Repo anchor | `<root>/repos/<slug>/notes.md` |
| `ROUX_SESSION_PROJECT` | Project slug (unset if no project) | `marketing-revamp` |
| `ROUX_SESSION_PROJECT_NOTES_DIR` | Project scope dir (unset if no project) | `<root>/projects/<slug>` |
| `ROUX_SESSION_PROJECT_NOTES_FILE` | Project anchor (unset if no project) | `<root>/projects/<slug>/notes.md` |
| `ROUX_SESSION_DIR` | Session scope dir | `<root>/sessions/<branch-slug>--<short-id>` |
| `ROUX_SESSION_NOTES_FILE` | Session anchor | `<root>/sessions/<branch-slug>--<short-id>/notes.md` |

- Directory vars are set even when the directory is empty — agents can drop arbitrary files without Roux materializing them first. Roux creates directories lazily on first write, so agents should `mkdir -p "$ROUX_REPO_NOTES_DIR"` before writing siblings.
- Project-related vars are **unset** (not empty) when the session has no project. Shell idioms like `${ROUX_SESSION_PROJECT:-no-project}` work naturally.
- All vars are read-only snapshots taken at PTY spawn time. Changing the project on a running session does not update env vars in already-running shells (user restarts the shell or resolves paths via `roux notes project path`).

### CLI — `roux notes`

Scope-as-subcommand, matching existing `roux session ...` pattern. All scoped commands default to the current session via env vars.

```
roux notes global   <show|append|write|path|open>
roux notes project  <show|append|write|path|open>      # errors if no project
roux notes repo     <show|append|write|path|open>
roux notes session  <show|append|write|path|open>

roux notes root                   # print $ROUX_NOTES_ROOT
roux notes repo list              # list known repo slugs + paths
roux notes repo rename <old> <new>
roux notes project rename <old> <new>
```

**Verbs:**

- `show` — print the file to stdout. `--json` wraps in `{ path, content, frontmatter }`.
- `append` — append content to the file. Reads from stdin or `--content "<text>"`. Creates the file (with frontmatter) if missing. Always prepends a single newline to ensure separation from prior content.
  - `--timestamp` — wraps the appended content in the timestamped-entry format (see above). Respects `notes.includeWebAnchors`.
- `write` — replace entire content from stdin or `--content`. Preserves/rewrites frontmatter (updates `updated` to now, keeps `created` and scope-specific fields).
- `path` — print the absolute file path. `--dir` prints the scope directory instead.
- `open` — open in `$EDITOR` (terminal) by default; `--app` opens in the OS default app for `.md` (Obsidian if registered). Tauri frontend exposes a matching context-menu item.

**Overrides:** every scoped command accepts `--session <id>`, `--repo <slug>`, `--project <slug>` to operate on a scope other than the current session's. Precedence: flag > current session env > error if neither resolves.

**Errors surface as non-zero exit codes + human message on stderr:**
- Project command with no project assigned → exit 2, "No project assigned to session".
- Unknown repo/project slug → exit 3, "Unknown slug '<x>'. Run `roux notes repo list` to see known slugs".
- Vault not writable → exit 4, "Cannot write to $ROUX_NOTES_ROOT".

### Settings

Two new keys in the existing settings store:

- `notes.vaultRoot: string` — absolute path. Default `~/Documents/Roux` (platform-appropriate). Setting UI is a path picker with "Reveal in Finder" / "Open in Obsidian" buttons.
- `notes.includeWebAnchors: boolean` — default `true`. Tooltip: "Include HTML anchor tags with each timestamped entry so entries can be deep-linked from any static site generator. Disable for cleaner raw markdown if you only read in Obsidian."

**Changing `notes.vaultRoot` does not move existing content.** Users copy/move the vault folder manually before pointing Roux at a new location. Roux only creates directories lazily under the current setting; it never relocates files.

### Backend Services

**New:** `src-tauri/src/services/notes.rs`:

- `VaultPath` abstraction with typed accessors: `root()`, `global_dir()`, `repo_dir(&slug)`, `project_dir(&slug)`, `session_dir(&slug)`, `*_notes_file(...)`.
- `NotesIndex` loader/writer for `.roux/repos.json` and `.roux/projects.json`.
- `resolve_repo_slug(repo_root)` and `resolve_project_slug(project_id)` — slug resolution with caching and collision handling.
- `read_scope(scope)`, `write_scope(scope, content)`, `append_scope(scope, content, AppendOpts)` — the four primitives the commands layer and CLI use.
- `frontmatter::ensure(path, scope)` — reads a file, guarantees frontmatter exists and is current for the scope, preserves body.
- `timestamped_entry::format(content, id, include_web_anchor)` — produces the entry block.
- Unit tests for slug resolution, frontmatter preservation, collision handling, and entry formatting.

**Modified:** `src-tauri/src/commands/projects.rs` — legacy `get_project_notes` / `set_project_notes` commands are **retired** (the frontend stops calling them). Removed from the command registry in `main.rs`.

**New commands** in `src-tauri/src/commands/notes.rs`:

- `notes_read(scope: NotesScopeRequest) -> Result<NotesRead, String>`
- `notes_write(scope: NotesScopeRequest, content: String) -> Result<(), String>`
- `notes_append(scope: NotesScopeRequest, content: String, timestamped: bool) -> Result<(), String>`
- `notes_path(scope: NotesScopeRequest, dir: bool) -> Result<String, String>`
- `notes_vault_root() -> Result<String, String>`
- `notes_rename_repo_slug(old: String, new: String) -> Result<(), String>`
- `notes_rename_project_slug(old: String, new: String) -> Result<(), String>`

`NotesScopeRequest` is `{ scope: "global" | "project" | "repo" | "session", session_id?: string, override_slug?: string }`. `session_id` defaults to the focused session and is used to resolve the current session's project/repo slugs. `override_slug` lets the CLI target a different repo/project (`--repo <slug>` / `--project <slug>`) without needing a session context.

**Filesystem watcher:** new module `src-tauri/src/services/notes_watcher.rs`, spawned during app init, publishes `notes-changed { path }` Tauri events. Lifecycle tied to `AppState`; watcher is rebuilt when `notes.vaultRoot` changes.

### CLI Backend

`roux notes` commands resolve `ROUX_SESSION_ID` / `ROUX_NOTES_ROOT` / etc. from env, build a `NotesScopeRequest`, and dispatch via the existing socket protocol (`src-tauri/src/socket.rs`). A new socket message `NotesRequest { kind: Read|Write|Append|Path, scope, ... }` mirrors the Tauri commands. This keeps a single source of truth (services layer) behind both the panel and the CLI.

### Frontend Changes

- `NotesPanel.svelte` rewritten: pill row, scope state (`"global" | "project" | "repo" | "session"`), per-session sticky selection via a new `sessions[sessionId].lastNotesScope` field in the sessions store, textarea bound to current scope's content, listener for `notes-changed` events.
- `src/lib/tauri.ts`: `getProjectNotes` / `setProjectNotes` removed. Replaced by `notesRead`, `notesWrite`, `notesAppend`, `notesPath`, `notesVaultRoot`.
- `src/lib/bindings.ts`: regenerated from Rust command signatures.
- `src/lib/stores/sessions.ts`: new per-session `lastNotesScope` field, defaulting to `"session"`.

### Migration

Runs once, on app startup, guarded by a `notes.migrated_v1: boolean` setting flag.

1. Ensure `notes.vaultRoot` exists (create if missing).
2. For each file in `roux_config_dir()/notes/*.txt`:
   - Derive project by looking up `<file-stem>` in the project store. Skip with a log if the project no longer exists.
   - Resolve (and cache) the project's slug.
   - If `$ROUX_NOTES_ROOT/projects/<slug>/notes.md` already exists, skip (idempotent).
   - Otherwise write it with the project frontmatter + the original text contents.
3. Leave the legacy `.txt` files in place as a backup.
4. Set `notes.migrated_v1 = true`.

Migration is silent on success, logged on any file-level failure, and never aborts app startup.

### Obsidian / SSG Compatibility Notes (user-facing docs)

The vault is Obsidian-compatible by construction, which also makes it compatible with Quartz, Hugo, Zola, MkDocs, and similar generators. We document but do not integrate:

- Point Obsidian at `$ROUX_NOTES_ROOT` to open the vault.
- If the user wants to publish a subset as a static site, **configure a publish filter on the generator side** (e.g., Quartz's `filterPlugins` to require `publish: true` frontmatter). Default Roux frontmatter does not include `publish:`. Users add it to notes they want to share.
- Obsidian block refs (`^entry-xxx`) render correctly in Obsidian and Quartz natively. Other generators typically need a plugin or render the `^` as literal text.
- The `<a id>` web anchors (when `notes.includeWebAnchors = true`) work in every generator that respects inline HTML.

## Testing Plan

### Rust unit tests (`src-tauri/src/services/notes.rs`)

- Slug resolution:
  - git dir with origin → remote-derived slug.
  - git dir without origin → path-derived slug.
  - non-git dir → path-derived slug.
  - duplicate slug → collision suffix `-2`, `-3`.
  - frozen after first resolution.
- Frontmatter ensure:
  - empty file → writes frontmatter + empty body.
  - existing file without frontmatter → prepends frontmatter.
  - existing file with frontmatter → preserves body, updates `updated`, leaves `created`.
  - existing file with unknown extra fields → preserves them.
- Timestamped entry formatting:
  - `includeWebAnchors = true` → anchor + heading + content + block ref.
  - `includeWebAnchors = false` → heading + content + block ref.
  - id is 8 hex chars, matches both occurrences.
- Append semantics:
  - plain append preserves frontmatter and appends with leading newline.
  - timestamped append on empty file creates frontmatter first, then entry.

### Rust integration tests

- CLI: `roux notes session show/append/write/path` with mocked `$ROUX_NOTES_ROOT` round-trips file contents.
- Scope override flags (`--repo`, `--project`, `--session`) resolve correctly.
- `roux notes project show` on session without project → exit code 2.
- Rename commands: `repo rename` moves folder, rewrites `repos.json`, leaves notes content intact.

### Frontend tests (Vitest, `src/lib/components/__tests__/NotesPanel.test.ts`)

- Pill selection updates textarea content.
- Project pill is disabled when `projectId` is null.
- Repo pill is disabled when `repoRoot` is empty.
- Last-selected scope is restored per session on panel reopen.
- `notes-changed` event for the current path triggers a reload when textarea is unchanged.
- `notes-changed` event for the current path with pending in-memory edits shows the conflict banner.

### Manual verification

- Open Roux, create a session, open the notes panel — session notes scope shows empty textarea.
- Type in each scope; switch pills; switch sessions; confirm sticky behavior.
- Edit `$ROUX_NOTES_ROOT/repos/<slug>/notes.md` in Obsidian — confirm Roux panel updates within ~300ms of save.
- In a Roux shell: `echo "foo" | roux notes session append --timestamp` — confirm entry block appears in panel immediately.
- Open `$ROUX_NOTES_ROOT` in Obsidian — no plugins needed, block refs work (`[[notes#^entry-xxx]]`), Dataview query `FROM "" WHERE type = "repo"` lists all repo notes.
- Reinstall from a build with the migration flag cleared and a populated legacy notes dir — verify migration populates `projects/<slug>/notes.md` and leaves the `.txt` files in place.

## Risks & Mitigations

- **Vault is outside Roux's app-support dir.** If a user deletes `~/Documents/Roux` (e.g. cleaning Documents), their notes vanish. Mitigation: the legacy `.txt` files remain as an on-startup backup of the old project notes; post-migration notes are the user's responsibility. The settings UI surfaces the path clearly.
- **Filesystem watcher on a user-chosen directory.** If the user points `notes.vaultRoot` at something huge (e.g. `~/Documents`), the watcher eats inotify / FSEvents budget. Mitigation: watch scope is strictly the configured root, not its parents. Document "point this at a dedicated folder" in settings tooltip.
- **Slug collisions between distinct repos with identical remotes** (e.g., two clones of the same fork at different paths with the same `origin`). Mitigation: the `-2`/`-3` suffix handles this automatically; `roux notes repo list` surfaces the mapping; `roux notes repo rename` lets the user fix it by hand.
- **Simultaneous edits in Obsidian and Roux panel.** Reload-on-focus + watcher handles the common case (edit in one, switch to the other). Truly simultaneous unfocused edits lose one side silently. Mitigation: the conflict banner appears the moment either side regains focus with divergent content. Full conflict resolution is out of scope for v1.
- **Frontmatter drift.** If Obsidian users manually edit frontmatter (e.g., adding tags), Roux must not clobber their additions. Mitigation: `frontmatter::ensure` preserves unknown fields and only touches known ones. `updated` is the only field Roux rewrites on every write.
- **Project/repo index corruption.** If `.roux/repos.json` is hand-edited into invalid JSON, Roux can't resolve slugs. Mitigation: on parse failure, log and fall back to in-memory resolution; a `roux notes repo rebuild-index` command (v2) can regenerate from session/project records.
- **Retiring legacy notes commands.** Any third-party plugin, script, or forked binary calling `get_project_notes` / `set_project_notes` breaks. Mitigation: the CLI has always been the public contract; Tauri commands are internal. No external consumers exist.

## Open Questions

None blocking. The following are deliberate deferrals to v2:

- `roux notes <scope> new <name>` and `list` for topic files (GitHub-issues-per-topic workflow).
- Panel-level multi-file browsing.
- Per-entry `roux notes <scope> show --entry <id>` (read/edit/delete by block-ref id).
- `roux notes spec new <topic>` helper for seeding spec files with a frontmatter template.
- Optional SSG config stubs (Quartz config in the vault, GitHub Actions publishing workflow template).
