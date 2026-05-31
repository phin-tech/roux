# What's New

This page tracks major shipped features across Roux's full history.

## April 2026 (foundation: Apr 4-9)

- **Core app + PTY/session lifecycle**: Tauri + Svelte app shell, persistent sessions, PTY management, typed IPC wrappers, reconnect flow, and close/quit safeguards.
- **Pane system**: split panes, stack mode, pane navigation/movement/resize/fullscreen, same-direction flattening, and durable pane-tree persistence.
- **Terminal + document workflows**: shell panes, markdown viewer/editor pane, command palette, pane rerun flow, and command panes for keep-open task output.
- **Projects, themes, and notes**: project grouping, repo grouping, session UI polish, multi-theme support, and project notes sidebar.
- **Task runner**: discover tasks from `package.json`, `Taskfile`, `Makefile`, and `Justfile`; run and manage tasks from sidebar and panes.
- **CLI bridge**: shipped `roux`, socket bridge, first-load install flow, and scripting entry points.
- **Watches**: long-running watch service with shell/HTTP/GitHub checks, PR watch kind, clickable PR/check/review links, and watch-focused UX.
- **Platform + release tooling**: Windows support, macOS signing/notarization flow, release automation, and improved logging/runtime diagnostics.

## April 10, 2026

- **Notification service rollout**: multi-phase notification system with in-app store, unread badges, `roux notify`, OSC parsing support, and focus-aware delivery policy.
- **Auto-updater**: in-app update checks/install flow with signed release verification and restart handling.
- **Session restore upgrades**: reconnect restores full saved pane layouts (including shell panes/splits) and uses live shell cwd for better recovery.
- **Shortcut upgrades**: session sidebar toggle (`cmd+\`) and quick-jump digits for sessions/panes with modifier overlays.
- **Bundled PTY CLI shim**: spawned shells expose `roux` directly.

## April 11, 2026

- **Spawn profiles + agent integration**: improved profile-driven startup and provider-aware pane/session behavior.

## April 12, 2026

- **Session layouts**: KDL-based multi-pane templates at session creation.
- **Leader mode**: Vimish command surface (`cmd+;`) for pane/session workflows.
- **CLI expansion**: stronger scripting and agent-to-agent control paths.
- **Settings redesign**: category-based settings modal with clearer organization.
- **Status bar positioning**: top/bottom status bar placement setting.

## April 13, 2026

- **Doctor panel**: integration health checks + reinstall/update actions.
- **Claude skill + env injection**: richer `ROUX_*` context in spawned PTYs.
- **Repository-root quick-pick**: fast repo search under configured roots in New Session.
- **Sidebar/session card refinements**: denser layout and header-level notification surfacing.

## April 14, 2026

- **Session from PR URL**: New Session accepts GitHub PR URLs and prepares review branches/worktrees.
- **Worktree templates + close policy**: templated worktree base path plus keep/ask/remove cleanup mode.
- **Optional modifier overlay toggles**: independent settings for Option pane hints and Command session hints.
- **Attention notification auto-dismiss**: notifications clear when attention state exits.
- **GitHub CLI resolution improvements**: login-shell PATH lookup plus explicit `gh` binary override.
- **Configurable keymap**: every keyboard shortcut — including the leader prefix and chord trees — moved out of hardcoded handlers into a KDL file at `~/.config/roux/keymap.kdl`. Switch to a built-in `tmux` preset with a single line, layer your own overrides on top, declare sticky/passthrough modes, and pick per-tree HUD visibility (`always` / `delayed <ms>` / `never`). Reload from the palette without a restart. See [Keymap](features/keymap.md).

## April 17, 2026

- **Collapsible session rail**: ++cmd+"\\"++ now collapses the full sidebar into a narrow rail of session dots, so you can reclaim horizontal space without giving up fast session switching.
- **Updater channels**: Settings gained a user-selectable **Stable / Pre-release (Alpha)** channel, letting alpha users follow prerelease builds without changing the default stable path for everyone else.

## April 18, 2026

- **Multi-scoped notes vault (experimental)**: the project-only scratchpad expanded into a four-scope, Obsidian-compatible vault. The ++cmd+b++ panel now supports **Session / Repo / Project / Global** notes with sticky per-session selection. Under the hood, each session writes to `~/Documents/Roux/<scope>/...`, giving you a plain markdown vault you can open in Obsidian, Quartz, Hugo, or other markdown tooling. PTYs now expose `ROUX_*_NOTES_*` env vars so agents can locate the right file without guessing, and `roux notes <scope> <verb>` adds CLI support for show / append / write / path / search-by-tag, plus `--topic` and `--tag` for more structured note sets. Existing project notes migrate to `projects/<slug>/notes.md` on first launch, with the originals kept as backup. Subject to change until the experimental banner is removed. See [Notes](features/notes.md).

## April 20, 2026

- **Worktree base selection**: new worktrees can now start from **current branch**, **main**, or **origin/main**. The same choices are exposed in the New Worktree flows, and Settings now lets you pick the default starting point.

## April 21, 2026

- **Native menu bar**: Roux now ships File/Edit/View/Session/Pane/Tools/Window/Help menus on macOS, Windows, and Linux, wired to the same command registry and active keymap as the palette.
- **Sessions History**: closing a session now soft-archives it into a history pane instead of immediately deleting the record. Archived rows can be restored, opened in Notes, cleaned up on disk, or permanently deleted later.

## April 22, 2026

- **Independent terminal themes**: terminal colors are now selected separately from the GUI theme, so you can keep a light UI with a dark shell, or the reverse.
- **User-imported terminal themes**: drop iTerm2 `.itermcolors` files into `~/.config/roux/themes/` and Roux will surface them in Settings alongside the built-in palettes.
- **v0.5.0**: the late-April UI and workflow polish shipped as the `v0.5.0` release.

## April 23, 2026

- **Multiline prompt editor groundwork**: Roux added a dedicated editor for preparing pasted CLI commands before sending them to a terminal, plus clipboard-seeded opening via ++cmd+shift+v++.
- **Draggable pane dividers**: split pane dividers are now draggable for quick manual resize, in addition to the existing keybindings.

## April 28, 2026

- **Library skill sync (experimental)**: Library skills can now be mirrored into Claude-readable `.claude/skills/<name>/SKILL.md` directories so Claude loads them automatically without copy-paste. Pick **Off / Copy / Symlink** as the global default in the Library sources panel; on Windows without Developer Mode, symlink mode auto-degrades to copy. Sync is conservative — it never overwrites a file Roux didn't write, and unsync hash-checks before deleting so locally-edited files are kept. Skills lose support for `{{ variable }}` placeholders to keep the format compatible with Claude's skill loader; existing global-vault skills are silently rewritten on first launch (the `name:` field is added, legacy `variables:` blocks are stripped). Run from the command palette (**Sync Library Skills**, **Unsync All Library Skills**) or the **Sync now** button in the Library panel. See [Library → Skill sync](features/library.md#skill-sync).
- **Auto WebGL terminal renderer**: terminal panes now recover gracefully when the WebGL context is lost (GPU process crash, suspended tab, too many WebGL contexts) — the WebGL addon is disposed and xterm reverts to its built-in DOM renderer without dropping the pane. Settings → Terminal gained a **GPU acceleration** dropdown (`Auto` / `On (WebGL)` / `Off (DOM)`) modeled on VSCode's `terminal.integrated.gpuAcceleration`. Setting changes apply to terminals opened afterward. See [Settings → GPU acceleration](settings.md#gpu-acceleration).

## May 1, 2026

- **Project workspaces**: Projects now have a full editor for repo lists, generated session blueprints, project prompts, and context paths. Group the sidebar by project to spawn saved blueprint sessions, or use **Spawn Project Session** from the command palette. Supported agent profiles receive the project prompt at startup, and spawned PTYs expose `ROUX_PROJECT_PROMPT` plus `ROUX_PROJECT_CONTEXT_PATHS` for custom automation. See [Projects](features/projects.md).

## May 2, 2026

- **Roux MCP integration**: Roux now ships `roux mcp`, a stdio MCP server that lets supported MCP hosts inspect and safely act on Roux sessions, panes, latest terminal output, and notes through the running desktop app. Settings gained an **Agent Integrations** section with **Enable Roux MCP**, CLI/current status, Claude Desktop host setup, dry-run preview, and scoped config writes that preserve unrelated host config. v1 intentionally excludes arbitrary shell execution, PTY kill, worktree removal, permanent session deletion, and broad filesystem mutation. See [CLI bridge → `roux mcp`](features/cli.md#roux-mcp) and [Settings → Agent Integrations](settings.md#agent-integrations).

## May 3, 2026

- **Warp-style multiline editor refresh**: the prompt editor is now a compact dock at the bottom of each terminal pane. ++ctrl+g++ toggles it for the focused pane, ++cmd+shift+e++ toggles it from anywhere, and ++cmd+shift+v++ opens with clipboard contents. Selecting terminal text and pressing ++ctrl+g++ seeds the editor from that selection. ++cmd+enter++ sends input and keeps the editor open, including an Enter press for the target terminal.
- **Editor ergonomics**: added shell-like local editing keys, command correction pills for common mistakes (`gti`, `git statsu`, missing `npm run`), and context chips for target, cwd, git branch/state, and spawn profile. See [Multiline Editor](features/editor.md).

## May 22, 2026

- **Standalone CLI crate + daemon foundation**: `roux` now builds from the separate `crates/roux-cli` crate instead of the Tauri desktop package. The desktop app package/binary is `roux-desktop`, and bundles the CLI sidecar as `roux`. The new experimental `roux daemon` command starts the shared runtime service host, owns the Roux command socket when running, and exposes `roux daemon status` plus headless session/project metadata commands as a first step toward daemon-owned sessions; Roux.app still owns interactive PTYs today. See [CLI bridge](features/cli.md) and [V2 session daemon](v2/session-daemon.md).
