use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::profile::{ProfileSource, SpawnProfile};

const DEFAULT_THEME: &str = "deep-blue";

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    Repo,
    Project,
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
    pub theme: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_binary_path: Option<String>,
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
            theme: DEFAULT_THEME.to_string(),
            default_model: None,
            claude_binary_path: None,
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
            spawn_profiles: Vec::new(),
            trusted_workspaces: Vec::new(),
            status_bar_position: StatusBarPosition::Bottom,
            show_pane_hints_on_option: false,
            show_session_hints_on_command: true,
        }
    }
}

impl RouxSettings {
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.theme = normalize_theme(&s.theme);
        // Force-set source on user profiles regardless of what the JSON says,
        // so a malicious or copy-pasted profile cannot masquerade as built-in.
        for profile in &mut s.spawn_profiles {
            profile.source = ProfileSource::User;
        }
        s.repo_roots = normalize_repo_roots(&s.repo_roots);
        // One-way migration: if an older settings file only has the legacy
        // `cleanupWorktreesOnClose: true` flag, promote the new enum to
        // Always. The `Prompt` default already matches legacy `false`, so
        // no migration is needed in that direction. Keep the legacy bool
        // in sync so older code paths and pre-migration consumers agree.
        if s.cleanup_worktrees_on_close && s.worktree_cleanup_on_close == WorktreeCleanupMode::Prompt {
            s.worktree_cleanup_on_close = WorktreeCleanupMode::Always;
        }
        s.cleanup_worktrees_on_close = s.worktree_cleanup_on_close == WorktreeCleanupMode::Always;
        s
    }
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
        "steel-amber" | "slate-emerald" | "graphite-rose" | "nordic-night" | "cyber-audit"
        | "mocha-soft" | "paper-ink" | "github-day" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::RouxSettings;

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
}
