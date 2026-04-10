use serde::{Deserialize, Serialize};

const DEFAULT_THEME: &str = "deep-blue";

fn default_ui_font_family() -> String {
    "Geist, Inter, SF Pro Display, Segoe UI, sans-serif".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self::Block
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TabPosition {
    Left,
    Right,
}

impl Default for TabPosition {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    Repo,
    Project,
}

impl Default for GroupBy {
    fn default() -> Self {
        Self::Repo
    }
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
    pub confirm_on_close: bool,
    pub restore_sessions_on_launch: bool,
    pub worktree_base_path: Option<String>,
    pub cleanup_worktrees_on_close: bool,
    pub theme: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_binary_path: Option<String>,
    pub additional_flags: Vec<String>,
    pub task_panel_split: f64,
    pub task_panel_collapsed: bool,
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
            confirm_on_close: true,
            restore_sessions_on_launch: true,
            worktree_base_path: None,
            cleanup_worktrees_on_close: false,
            theme: DEFAULT_THEME.to_string(),
            default_model: None,
            claude_binary_path: None,
            additional_flags: Vec::new(),
            task_panel_split: 0.5,
            task_panel_collapsed: true,
            enable_logging: false,
            group_by: GroupBy::Repo,
            confirm_on_quit: true,
            notifications_enabled: true,
        }
    }
}

impl RouxSettings {
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.theme = normalize_theme(&s.theme);
        s
    }
}

fn normalize_theme(theme: &str) -> String {
    match theme {
        "dark" | "deep-blue" => DEFAULT_THEME.to_string(),
        "steel-amber" | "slate-emerald" | "graphite-rose" | "nordic-night" | "cyber-audit"
        | "mocha-soft" | "paper-ink" | "github-day" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}
