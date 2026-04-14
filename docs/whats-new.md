# What's New

This page tracks major shipped features across Roux's full history.

## April 2026 (foundation: Apr 4-9)

- **Core app + PTY/session lifecycle**: Tauri + Svelte app shell, persistent sessions, PTY management, typed IPC wrappers, reconnect flow, and close/quit safeguards.
- **Pane system**: split panes, stack mode, pane navigation/movement/resize/fullscreen, same-direction flattening, and durable pane-tree persistence.
- **Terminal + document workflows**: shell panes, markdown viewer/editor pane, command palette, pane rerun flow, and command panes for keep-open task output.
- **Projects, themes, and notes**: project grouping, repo grouping, session UI polish, multi-theme support, and project notes sidebar.
- **Task runner**: discover tasks from `package.json`, `Taskfile`, `Makefile`, and `Justfile`; run and manage tasks from sidebar and panes.
- **CLI bridge**: shipped `roux-cli`, socket bridge, first-load install flow, and scripting entry points.
- **Watches**: long-running watch service with shell/HTTP/GitHub checks, PR watch kind, clickable PR/check/review links, and watch-focused UX.
- **Platform + release tooling**: Windows support, macOS signing/notarization flow, release automation, and improved logging/runtime diagnostics.

## April 10, 2026

- **Notification service rollout**: multi-phase notification system with in-app store, unread badges, `roux notify`, OSC parsing support, and focus-aware delivery policy.
- **Auto-updater**: in-app update checks/install flow with signed release verification and restart handling.
- **Session restore upgrades**: reconnect restores full saved pane layouts (including shell panes/splits) and uses live shell cwd for better recovery.
- **Shortcut upgrades**: session sidebar toggle (`cmd+\`) and quick-jump digits for sessions/panes with modifier overlays.
- **Bundled PTY CLI aliases**: spawned shells expose `roux` / `roux-cli` directly.

## April 11, 2026

- **Spawn profiles + agent integration**: improved profile-driven startup and provider-aware pane/session behavior.

## April 12, 2026

- **Session layouts**: KDL-based multi-pane templates at session creation.
- **Leader mode**: Vimish command surface (`cmd+;`) for pane/session workflows.
- **CLI expansion**: stronger scripting and agent-to-agent control paths.
- **Nono integration**: layout-level sandbox wrapping with allow-dir controls.
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
