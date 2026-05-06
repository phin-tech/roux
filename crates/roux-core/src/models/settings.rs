use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::profile::{ProfileSource, SpawnProfile};

const DEFAULT_THEME: &str = "deep-blue";
const DEFAULT_TERMINAL_THEME: &str = "match-gui";

fn default_terminal_theme() -> String {
    DEFAULT_TERMINAL_THEME.to_string()
}

fn default_ui_font_family() -> String {
    "Geist, Inter, SF Pro Display, Segoe UI, sans-serif".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum TabPosition {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum StatusBarPosition {
    Top,
    #[default]
    Bottom,
}

/// Behavior when a session that owns a worktree is closed.
///
/// - `Never` — leave the worktree on disk
/// - `Prompt` — ask the user via a confirm dialog (current default, matches
///   the legacy `cleanupWorktreesOnClose: false` behavior)
/// - `Always` — remove without asking (matches legacy `true`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeCleanupMode {
    Never,
    #[default]
    Prompt,
    Always,
}

/// Which starting point the "New Worktree" affordance uses when the user
/// clicks the primary action directly (as opposed to hovering to pick a
/// specific base from the flyout). Only affects the default — the three
/// options remain available via the submenu / command palette either way.
///
/// - `CurrentBranch` — the session's current branch (matches legacy behavior)
/// - `Main` — the local `main` branch
/// - `OriginMain` — the remote `origin/main`, with a `git fetch origin` first
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeDefaultBase {
    #[default]
    CurrentBranch,
    Main,
    OriginMain,
}

/// Which backend Roux uses to create worktrees.
///
/// - `Auto` (default) — use `wt` when it is detected on the system;
///   otherwise fall back to native `git worktree add`. This is the
///   recommended setting: users without worktrunk see no change, users
///   with worktrunk get its hooks/templates/project config for free.
/// - `Git` — always use native git. Useful as an escape hatch if a
///   worktrunk hook is misbehaving.
/// - `Worktrunk` — always prefer `wt`. If `wt` fails for any reason,
///   Roux still falls back to native git so worktree creation never
///   breaks entirely — the setting expresses preference, not veto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeProvider {
    #[default]
    Auto,
    Git,
    Worktrunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    Repo,
    Project,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    PreRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySourceKind {
    #[default]
    LocalRepo,
    GitRepo,
}

/// Whether and how a Library source's skills are written into a
/// Claude-readable `.claude/skills/<name>/SKILL.md` directory.
///
/// - `Off`: Roux does not write skill files outside the Library.
/// - `Copy`: Roux writes a copy of each skill on sync; subsequent edits
///   to the synced file are detected via a content-hash manifest.
/// - `Symlink`: Roux symlinks each `.claude/skills/<name>/` entry back
///   to the source skill file. Edits in either place are the same edit.
///   On Windows, when symlinks are denied, Roux auto-degrades to `Copy`
///   for that sync run and emits a one-time toast event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum SkillSyncMode {
    #[default]
    Off,
    Copy,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySource {
    pub id: String,
    #[serde(default)]
    pub kind: LibrarySourceKind,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Per-source override for skill sync mode. `None` means "inherit
    /// the global default" (`RouxSettings::library_skill_sync_default`).
    #[serde(default)]
    pub skill_sync: Option<SkillSyncMode>,
}

/// What happens to a PTY when its pane is closed.
///
/// - `Kill` — the PTY process is killed immediately.
/// - `Detach` — the PTY keeps running in the background; it can be
///   re-attached to another pane later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum OnPaneCloseMode {
    Detach,
    #[default]
    Kill,
}

/// xterm.js renderer selection. `Auto` (default) tries WebGL and silently
/// falls back to the built-in DOM renderer if construction fails or the
/// WebGL context is lost. `On` is identical to `Auto` today — kept as a
/// distinct option because users have a clear mental model from VSCode's
/// `terminal.integrated.gpuAcceleration`. `Off` skips WebGL entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum GpuAcceleration {
    #[default]
    Auto,
    On,
    Off,
}

/// No-op variant used to verify the enum-experiment pipeline end to end.
/// Replace or remove once a real enum experiment lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExampleVariant {
    #[default]
    A,
    B,
    C,
}

/// Runtime feature flags surfaced under Settings → Experiments. Each field is
/// either a `bool` (toggle) or a small enum (multi-choice). Adding a field
/// here also requires adding a registry entry in `src/lib/experiments.ts` so
/// the UI knows how to render it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ExperimentsConfig {
    #[serde(default)]
    pub example_flag: bool,
    #[serde(default)]
    pub example_variant: ExampleVariant,
    #[serde(default)]
    pub simplified_session_tabs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxSettings {
    pub tab_position: TabPosition,
    pub tab_width: u32,
    pub font_size: u32,
    pub font_family: String,
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,
    pub line_height: f64,
    pub scrollback: u32,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub default_project_path: Option<String>,
    #[serde(default)]
    pub repo_roots: Vec<String>,
    #[serde(default = "default_true")]
    pub exclude_worktrees_from_repo_roots: bool,
    pub confirm_on_close: bool,
    pub restore_sessions_on_launch: bool,
    pub worktree_base_path: Option<String>,
    /// Legacy boolean kept for backward compatibility with settings files
    /// written before `worktree_cleanup_on_close` existed. The frontend no
    /// longer reads this directly; `normalized()` migrates it into the new
    /// enum and keeps the two in sync for older readers.
    pub cleanup_worktrees_on_close: bool,
    #[serde(default)]
    pub worktree_cleanup_on_close: WorktreeCleanupMode,
    /// Default starting point when the user clicks "New Worktree" directly.
    /// The flyout / command palette still let them override per-invocation.
    #[serde(default)]
    pub worktree_default_base: WorktreeDefaultBase,
    pub theme: String,
    /// Terminal color palette. `"match-gui"` (default) follows the GUI
    /// theme's bundled terminal palette; any other value names a standalone
    /// palette (one of the GUI-matching IDs or a built-in editor scheme like
    /// `dracula`, `solarized-dark`, etc.). Unknown IDs normalize back to
    /// `"match-gui"` so a future schema addition cannot brick old clients.
    #[serde(default = "default_terminal_theme")]
    pub terminal_theme: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_binary_path: Option<String>,
    /// Absolute path to the `gh` (GitHub CLI) binary. When set and non-empty,
    /// Roux uses it directly instead of resolving `gh` from `PATH`. macOS
    /// GUI apps inherit a minimal PATH that often excludes `/opt/homebrew/bin`
    /// and other shell-managed prefixes, so users on fish/zsh with Homebrew
    /// typically need to set this explicitly.
    #[serde(default)]
    pub gh_binary_path: Option<String>,
    /// Absolute path to the `git` binary. When set and non-empty, Roux uses
    /// it for git-backed Library sources instead of resolving `git` from the
    /// login-shell PATH or process PATH.
    #[serde(default)]
    pub git_binary_path: Option<String>,
    /// Absolute path to the `wt` (worktrunk) binary. When set and non-empty,
    /// Roux uses it directly instead of resolving `wt` from `PATH`. Same
    /// motivation as `gh_binary_path` — macOS GUI apps inherit a minimal
    /// PATH that often excludes `/opt/homebrew/bin`. Leave unset to resolve
    /// via the login-shell PATH and fall back to "no worktrunk available"
    /// when nothing is found.
    #[serde(default)]
    pub worktrunk_binary_path: Option<String>,
    /// Absolute path to the `smolvm` (smol machines) binary. Same motivation
    /// as `worktrunk_binary_path` — GUI apps inherit a minimal PATH on macOS.
    /// When unset, Roux resolves via PATH and falls back to "smolvm not
    /// installed" (the activity rail icon and integration UI hide entirely).
    #[serde(default)]
    pub smolvm_binary_path: Option<String>,
    /// Absolute path to the shell binary for terminal panes and login-shell
    /// PATH discovery. When set and non-empty, overrides automatic resolution
    /// from the OS login shell, then $SHELL.
    #[serde(default)]
    pub shell_binary_path: Option<String>,
    /// Which backend Roux uses to create worktrees. Default `Auto` prefers
    /// `wt` when available and falls back to git when not.
    #[serde(default)]
    pub worktree_provider: WorktreeProvider,
    pub additional_flags: Vec<String>,
    pub task_panel_split: f64,
    pub task_panel_collapsed: bool,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub enable_logging: bool,
    #[serde(default)]
    pub group_by: GroupBy,
    #[serde(default = "default_true")]
    pub confirm_on_quit: bool,
    /// Master kill-switch for the notification service's OS-notification
    /// fan-out. When false, notifications still land in the in-app pane but
    /// `tauri-plugin-notification` is never invoked. Defaults to true.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// When a background agent leaves the "attention" (waiting-for-answer)
    /// state, also clear the pane's `permissionInfo` so the Claude
    /// Allow/Deny affordance disappears alongside the notification.
    /// Defaults to true — rollback insurance only.
    #[serde(default = "default_true")]
    pub auto_clear_attention_state: bool,
    /// Whether Roux checks for updates silently on launch. Manual checks via
    /// Settings / command palette remain available regardless.
    #[serde(default = "default_true")]
    pub update_check_on_launch: bool,
    /// Experimental multi-scoped notes — when true (the default), the
    /// timestamped-append CLI verb prefixes each entry with an inline
    /// `<a id="entry-...">` HTML anchor so the entry is deep-linkable
    /// from any mainstream static-site generator. Disable for cleaner
    /// raw markdown if you only read the vault in Obsidian.
    #[serde(default = "default_true")]
    pub notes_include_web_anchors: bool,
    /// Experimental multi-scoped notes — override for the vault root
    /// directory. `None` means "use the default `~/Documents/Roux`".
    #[serde(default)]
    pub notes_vault_root: Option<String>,
    /// Experimental multi-scoped notes — set to `true` once the legacy
    /// `<project_id>.txt` files have been migrated into the vault. Guards
    /// against re-running the migration on every app launch.
    #[serde(default)]
    pub notes_migrated_v1: bool,
    /// Which update channel this user is subscribed to. `Stable` pulls from the
    /// standard `latest.json` manifest; `PreRelease` resolves the newest
    /// GitHub prerelease at check time and pulls its `latest-prerelease.json`.
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// User-defined spawn profiles. Edited as raw JSON in the settings file
    /// in v1 — the "Save as user profile" UI is a later addition. The settings
    /// loader force-sets `source: "user"` on each entry regardless of what
    /// the file says, so users can't forge a `"builtin"` marker.
    #[serde(default)]
    pub spawn_profiles: Vec<SpawnProfile>,
    /// Absolute paths of workspaces the user has marked trusted for the
    /// future project-profile loader (`.roux/profiles.json`). Reserved in
    /// phase 3; the loader that consumes this list ships later. Storing the
    /// field now means the trust prompt and its toggles will not require a
    /// settings schema bump when they arrive.
    #[serde(default)]
    pub trusted_workspaces: Vec<String>,
    /// Ordered git repositories that contribute reusable Library items
    /// under `.roux/library/prompts/` and `.roux/library/skills/`.
    /// The active repo is layered separately at runtime and wins over these.
    #[serde(default)]
    pub library_pinned_repos: Vec<String>,
    /// Ordered Library sources. Local repo sources read `.roux/library` from
    /// an existing checkout; Git repo sources are managed by Roux and synced
    /// through native Git operations.
    #[serde(default)]
    pub library_sources: Vec<LibrarySource>,
    #[serde(default)]
    pub status_bar_position: StatusBarPosition,
    /// Reveal the pane-number overlay while Option (⌥) is held. The
    /// `Option+digit` / `Option+HJKL` chord shortcuts are unaffected either way.
    #[serde(default)]
    pub show_pane_hints_on_option: bool,
    /// Reveal the session-shortcut overlay while the primary modifier
    /// (⌘ on macOS, Ctrl elsewhere) is held. Chord shortcuts are unaffected.
    #[serde(default = "default_true")]
    pub show_session_hints_on_command: bool,
    /// What happens to a PTY when its pane is closed.
    /// `Kill` (default): the process is killed immediately.
    /// `Detach`: the process keeps running and can be re-attached.
    #[serde(default)]
    pub on_pane_close: OnPaneCloseMode,
    /// Set to `true` once the global Library skills have been rewritten to
    /// the SKILL.md-compatible format (adds a `name:` field, strips legacy
    /// `variables:` blocks). Guards against re-running on every launch.
    #[serde(default)]
    pub library_skill_format_v2_migrated: bool,
    /// Default skill-sync mode applied to Library sources that don't
    /// override it (`LibrarySource::skill_sync`), to the global vault,
    /// and to the active session repo. Defaults to `Off` so existing
    /// users see no behavior change after upgrade.
    #[serde(default)]
    pub library_skill_sync_default: SkillSyncMode,
    /// xterm.js renderer hint. `Auto` (default) tries WebGL with DOM fallback.
    /// Setting changes apply to terminals created afterward; existing panes
    /// keep their renderer until reopened.
    #[serde(default)]
    pub gpu_acceleration: GpuAcceleration,
    /// When true, sessions whose worktree branch resolves to an open GitHub
    /// PR get a session-scoped `GithubPr` watch created automatically. The
    /// status-bar PR link is shown regardless of this setting; only the
    /// background watch creation is gated. Defaults to `false` so users
    /// don't get unexpected new watches on upgrade.
    #[serde(default)]
    pub auto_watch_session_pr: bool,
    /// Master switch for the gh-backed session-PR lookup. When true (the
    /// default), session activation triggers a `gh pr list --head <branch>`
    /// call to populate the status-bar PR chip and feed the auto-watch
    /// flow. When false, no gh call is made, the chip falls back to
    /// worktrunk's `ciUrl` only, and `auto_watch_session_pr` becomes a
    /// no-op. Useful for users who don't use GitHub PRs or want to avoid
    /// the gh subprocess on every session switch.
    #[serde(default = "default_true")]
    pub auto_lookup_session_pr: bool,
    /// User-facing MCP integration switch. The MCP server is still launched
    /// by MCP hosts via `roux-cli mcp`; this controls Roux's setup/status UX
    /// and whether host configuration buttons are presented as enabled.
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Last MCP host that Roux successfully configured, if any. Stored for
    /// Settings status only; host config files remain the source of truth.
    #[serde(default)]
    pub mcp_last_configured_host: Option<String>,
    /// Unix epoch milliseconds for the last successful MCP host config write.
    #[serde(default)]
    pub mcp_last_configured_at_ms: Option<u64>,
    /// Runtime feature flags. See `ExperimentsConfig`.
    #[serde(default)]
    pub experiments: ExperimentsConfig,
}

impl Default for RouxSettings {
    fn default() -> Self {
        Self {
            tab_position: TabPosition::Left,
            tab_width: 260,
            font_size: 14,
            font_family: "JetBrains Mono, IBM Plex Mono, SFMono-Regular, monospace".to_string(),
            ui_font_family: default_ui_font_family(),
            line_height: 1.2,
            scrollback: 5000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            default_project_path: None,
            repo_roots: Vec::new(),
            exclude_worktrees_from_repo_roots: true,
            confirm_on_close: true,
            restore_sessions_on_launch: true,
            worktree_base_path: None,
            cleanup_worktrees_on_close: false,
            worktree_cleanup_on_close: WorktreeCleanupMode::Prompt,
            worktree_default_base: WorktreeDefaultBase::CurrentBranch,
            theme: DEFAULT_THEME.to_string(),
            terminal_theme: DEFAULT_TERMINAL_THEME.to_string(),
            default_model: None,
            claude_binary_path: None,
            gh_binary_path: None,
            git_binary_path: None,
            worktrunk_binary_path: None,
            smolvm_binary_path: None,
            shell_binary_path: None,
            worktree_provider: WorktreeProvider::default(),
            additional_flags: Vec::new(),
            task_panel_split: 0.5,
            task_panel_collapsed: true,
            sidebar_collapsed: false,
            enable_logging: false,
            group_by: GroupBy::Repo,
            confirm_on_quit: true,
            notifications_enabled: true,
            auto_clear_attention_state: true,
            update_check_on_launch: true,
            notes_include_web_anchors: true,
            notes_vault_root: None,
            notes_migrated_v1: false,
            update_channel: UpdateChannel::default(),
            spawn_profiles: Vec::new(),
            trusted_workspaces: Vec::new(),
            library_pinned_repos: Vec::new(),
            library_sources: Vec::new(),
            status_bar_position: StatusBarPosition::Bottom,
            show_pane_hints_on_option: false,
            show_session_hints_on_command: true,
            on_pane_close: OnPaneCloseMode::Kill,
            library_skill_format_v2_migrated: false,
            library_skill_sync_default: SkillSyncMode::Off,
            gpu_acceleration: GpuAcceleration::Auto,
            auto_watch_session_pr: false,
            auto_lookup_session_pr: true,
            mcp_enabled: false,
            mcp_last_configured_host: None,
            mcp_last_configured_at_ms: None,
            experiments: ExperimentsConfig::default(),
        }
    }
}

impl RouxSettings {
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.theme = normalize_theme(&s.theme);
        s.terminal_theme = normalize_terminal_theme(&s.terminal_theme);
        // Force-set source on user profiles regardless of what the JSON says,
        // so a malicious or copy-pasted profile cannot masquerade as built-in.
        for profile in &mut s.spawn_profiles {
            profile.source = ProfileSource::User;
        }
        s.repo_roots = normalize_repo_roots(&s.repo_roots);
        s.library_pinned_repos = normalize_repo_roots(&s.library_pinned_repos);
        if s.library_sources.is_empty() && !s.library_pinned_repos.is_empty() {
            s.library_sources = s
                .library_pinned_repos
                .iter()
                .enumerate()
                .map(|(index, path)| LibrarySource {
                    id: stable_source_id("local", path),
                    kind: LibrarySourceKind::LocalRepo,
                    name: source_name_from_path(path),
                    enabled: true,
                    order: index as u32,
                    path: Some(path.clone()),
                    url: None,
                    branch: None,
                    skill_sync: None,
                })
                .collect();
        }
        s.library_sources = normalize_library_sources(&s.library_sources);
        // One-way migration: if an older settings file only has the legacy
        // `cleanupWorktreesOnClose: true` flag, promote the new enum to
        // Always. The `Prompt` default already matches legacy `false`, so
        // no migration is needed in that direction. Keep the legacy bool
        // in sync so older code paths and pre-migration consumers agree.
        if s.cleanup_worktrees_on_close
            && s.worktree_cleanup_on_close == WorktreeCleanupMode::Prompt
        {
            s.worktree_cleanup_on_close = WorktreeCleanupMode::Always;
        }
        s.cleanup_worktrees_on_close = s.worktree_cleanup_on_close == WorktreeCleanupMode::Always;
        s
    }
}

fn normalize_library_sources(sources: &[LibrarySource]) -> Vec<LibrarySource> {
    let mut dedup = HashSet::new();
    let mut cleaned = Vec::new();
    for (fallback_order, source) in sources.iter().enumerate() {
        let mut next = source.clone();
        next.name = next.name.trim().to_string();
        next.path = next.path.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        next.url = next.url.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        next.branch = next.branch.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let identity = match next.kind {
            LibrarySourceKind::LocalRepo => next.path.clone(),
            LibrarySourceKind::GitRepo => next.url.clone(),
        };
        let Some(identity) = identity else {
            continue;
        };
        if next.id.trim().is_empty() {
            let prefix = match next.kind {
                LibrarySourceKind::LocalRepo => "local",
                LibrarySourceKind::GitRepo => "git",
            };
            let branch = next.branch.clone().unwrap_or_default();
            next.id = stable_source_id(prefix, &format!("{identity}@{branch}"));
        } else {
            next.id = next.id.trim().to_string();
        }
        if next.name.is_empty() {
            next.name = match next.kind {
                LibrarySourceKind::LocalRepo => source_name_from_path(&identity),
                LibrarySourceKind::GitRepo => source_name_from_url(&identity),
            };
        }
        if next.order == 0 && fallback_order > 0 {
            next.order = fallback_order as u32;
        }
        if dedup.insert(next.id.clone()) {
            cleaned.push(next);
        }
    }
    cleaned.sort_by_key(|source| source.order);
    for (index, source) in cleaned.iter_mut().enumerate() {
        source.order = index as u32;
    }
    cleaned
}

fn stable_source_id(prefix: &str, value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}

fn source_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Library Source")
        .to_string()
}

fn source_name_from_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    trimmed
        .rsplit(['/', ':'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Git Library")
        .to_string()
}

fn normalize_repo_roots(roots: &[String]) -> Vec<String> {
    let mut dedup = HashSet::new();
    let mut cleaned = Vec::new();
    for root in roots {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        if dedup.insert(trimmed.to_string()) {
            cleaned.push(trimmed.to_string());
        }
    }
    cleaned
}

fn normalize_theme(theme: &str) -> String {
    match theme {
        "dark" | "deep-blue" => DEFAULT_THEME.to_string(),
        "midnight-copper" | "steel-amber" | "slate-emerald" | "graphite-rose"
        | "nordic-night" | "cyber-audit" | "mocha-soft" | "paper-ink" | "github-day"
        | "warm-burnout-dark" | "warm-burnout-light" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

fn normalize_terminal_theme(theme: &str) -> String {
    // User-supplied themes from `~/.config/roux/themes/*.itermcolors` are
    // identified by `user:<stem>`. The validator can't enumerate them
    // (they live on disk and the file may be temporarily missing on
    // load), so accept any non-empty `user:*` ID and let the resolver
    // fall back to "match-gui" at render time if the file is gone.
    if let Some(rest) = theme.strip_prefix("user:") {
        if !rest.is_empty() {
            return theme.to_string();
        }
    }
    match theme {
        // Sentinel: follow the GUI theme's bundled terminal palette.
        "match-gui"
        // GUI-matching palettes (one per GUI preset).
        | "deep-blue" | "midnight-copper" | "steel-amber" | "slate-emerald"
        | "graphite-rose" | "nordic-night" | "cyber-audit" | "mocha-soft"
        | "paper-ink" | "github-day" | "warm-burnout-dark" | "warm-burnout-light"
        // Editor-style palettes (iterm2colorschemes-inspired).
        | "dracula" | "solarized-dark" | "solarized-light" | "monokai"
        | "nord" | "gruvbox-dark" | "tokyo-night" | "one-dark"
        | "catppuccin-mocha" | "github-dark"
        // Light editor palettes.
        | "github-light" | "one-light" | "catppuccin-latte" | "tokyo-night-day"
        | "gruvbox-light" | "tomorrow" | "ayu-light" => theme.to_string(),
        _ => DEFAULT_TERMINAL_THEME.to_string(),
    }
}

#[cfg(test)]
mod terminal_theme_tests {
    use super::normalize_terminal_theme;

    #[test]
    fn user_prefix_passes_through() {
        assert_eq!(normalize_terminal_theme("user:dracula-mod"), "user:dracula-mod");
        assert_eq!(normalize_terminal_theme("user:my fav"), "user:my fav");
    }

    #[test]
    fn empty_user_prefix_falls_back() {
        assert_eq!(normalize_terminal_theme("user:"), "match-gui");
    }

    #[test]
    fn unknown_falls_back_to_match_gui() {
        assert_eq!(normalize_terminal_theme("not-a-theme"), "match-gui");
    }
}

#[cfg(test)]
mod theme_tests {
    use super::{normalize_theme, DEFAULT_THEME};

    #[test]
    fn known_presets_round_trip() {
        for preset in [
            "midnight-copper",
            "steel-amber",
            "slate-emerald",
            "graphite-rose",
            "nordic-night",
            "cyber-audit",
            "mocha-soft",
            "paper-ink",
            "github-day",
            "warm-burnout-dark",
            "warm-burnout-light",
        ] {
            assert_eq!(normalize_theme(preset), preset, "preset {preset} should round-trip");
        }
    }

    #[test]
    fn legacy_dark_alias_normalizes_to_default() {
        assert_eq!(normalize_theme("dark"), DEFAULT_THEME);
        assert_eq!(normalize_theme("deep-blue"), DEFAULT_THEME);
    }

    #[test]
    fn unknown_falls_back_to_default() {
        assert_eq!(normalize_theme("not-a-theme"), DEFAULT_THEME);
        assert_eq!(normalize_theme(""), DEFAULT_THEME);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        stable_source_id, ExampleVariant, ExperimentsConfig, LibrarySource, LibrarySourceKind,
        RouxSettings, SkillSyncMode, UpdateChannel,
    };

    #[test]
    fn hint_overlay_defaults_preserve_command_drop_option() {
        let defaults = RouxSettings::default();
        assert!(!defaults.show_pane_hints_on_option);
        assert!(defaults.show_session_hints_on_command);

        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert!(!parsed.show_pane_hints_on_option);
        assert!(parsed.show_session_hints_on_command);
    }

    #[test]
    fn normalized_repo_roots_trims_empty_and_duplicates() {
        let settings = RouxSettings {
            repo_roots: vec![
                "  /tmp/src  ".to_string(),
                "".to_string(),
                " /tmp/src ".to_string(),
                "/tmp/other".to_string(),
            ],
            ..RouxSettings::default()
        };

        let normalized = settings.normalized();
        assert_eq!(normalized.repo_roots, vec!["/tmp/src", "/tmp/other"]);
    }

    #[test]
    fn settings_without_update_channel_defaults_to_stable() {
        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.update_channel, UpdateChannel::Stable);
    }

    #[test]
    fn settings_without_git_binary_path_deserializes_as_none() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.git_binary_path, None);
    }

    #[test]
    fn stable_source_id_is_deterministic() {
        assert_eq!(
            stable_source_id("git", "https://example.com/team/lib@main"),
            "git-abf3a29bca551e36"
        );
    }

    #[test]
    fn settings_without_repo_roots_deserializes_with_default() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert!(settings.repo_roots.is_empty());
    }

    #[test]
    fn settings_default_skill_sync_is_off() {
        let settings = RouxSettings::default();
        assert_eq!(settings.library_skill_sync_default, SkillSyncMode::Off);
    }

    #[test]
    fn settings_without_experiments_deserializes_with_default() {
        // Pre-existing settings.json files written before the experiments
        // field existed must continue to load with all flags off.
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert!(!settings.experiments.example_flag);
        assert_eq!(settings.experiments.example_variant, ExampleVariant::A);
    }

    #[test]
    fn experiments_partial_payload_fills_missing_with_defaults() {
        // A flag added later (e.g. only `exampleVariant` set, `exampleFlag`
        // absent) must not fail deserialization — each inner field is
        // `#[serde(default)]`.
        let json = r#"{ "exampleVariant": "c" }"#;
        let exp: ExperimentsConfig = serde_json::from_str(json).unwrap();
        assert!(!exp.example_flag);
        assert_eq!(exp.example_variant, ExampleVariant::C);
        assert!(!exp.simplified_session_tabs);
    }

    #[test]
    fn experiments_default_simplified_session_tabs_off() {
        // Legacy settings written before `simplifiedSessionTabs` existed must
        // deserialize cleanly with the flag defaulting to `false`.
        let json = r#"{ "exampleFlag": true }"#;
        let exp: ExperimentsConfig = serde_json::from_str(json).unwrap();
        assert!(exp.example_flag);
        assert!(!exp.simplified_session_tabs);
    }

    #[test]
    fn library_source_skill_sync_defaults_to_none_when_missing() {
        let json = r#"{
            "id": "src-1",
            "kind": "localRepo",
            "name": "Repo",
            "enabled": true,
            "order": 0,
            "path": "/repo"
        }"#;
        let source: LibrarySource = serde_json::from_str(json).unwrap();
        assert_eq!(source.skill_sync, None);
    }

    #[test]
    fn library_source_skill_sync_round_trips() {
        let source = LibrarySource {
            id: "src-1".into(),
            kind: LibrarySourceKind::LocalRepo,
            name: "Repo".into(),
            enabled: true,
            order: 0,
            path: Some("/repo".into()),
            url: None,
            branch: None,
            skill_sync: Some(SkillSyncMode::Symlink),
        };
        let json = serde_json::to_string(&source).unwrap();
        let parsed: LibrarySource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skill_sync, Some(SkillSyncMode::Symlink));
    }
}
