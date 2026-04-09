use serde::{Deserialize, Serialize};
use std::fs;

use crate::platform;

const DEFAULT_THEME: &str = "deep-blue";

fn default_ui_font_family() -> String {
    "Geist, Inter, SF Pro Display, Segoe UI, sans-serif".to_string()
}

fn normalize_theme(theme: &str) -> String {
    match theme {
        "dark" | "deep-blue" => DEFAULT_THEME.to_string(),
        "steel-amber" | "slate-emerald" | "graphite-rose" | "nordic-night" | "cyber-audit"
        | "mocha-soft" | "paper-ink" | "github-day" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouxSettings {
    pub tab_position: String,
    pub tab_width: u32,
    pub font_size: u32,
    pub font_family: String,
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,
    pub line_height: f64,
    pub scrollback: u32,
    pub cursor_style: String,
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
    #[serde(default = "default_group_by")]
    pub group_by: String,
}

fn default_group_by() -> String {
    "repo".to_string()
}

impl Default for RouxSettings {
    fn default() -> Self {
        Self {
            tab_position: "left".to_string(),
            tab_width: 260,
            font_size: 14,
            font_family: "JetBrains Mono, IBM Plex Mono, SFMono-Regular, monospace".to_string(),
            ui_font_family: default_ui_font_family(),
            line_height: 1.2,
            scrollback: 5000,
            cursor_style: "block".to_string(),
            cursor_blink: true,
            default_project_path: None,
            confirm_on_close: true,
            restore_sessions_on_launch: true,
            worktree_base_path: None,
            cleanup_worktrees_on_close: false,
            theme: DEFAULT_THEME.to_string(),
            default_model: None,
            claude_binary_path: None,
            additional_flags: vec![],
            task_panel_split: 0.4,
            task_panel_collapsed: false,
            enable_logging: false,
            group_by: default_group_by(),
        }
    }
}

impl RouxSettings {
    pub fn normalized(mut self) -> Self {
        self.theme = normalize_theme(&self.theme);
        self
    }
}

pub fn load_settings() -> RouxSettings {
    let path = platform::settings_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<RouxSettings>(&content).unwrap_or_default().normalized()
    } else {
        RouxSettings::default()
    }
}

pub fn save_settings(settings: &RouxSettings) -> Result<(), String> {
    let path = platform::settings_path();
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json =
        serde_json::to_string_pretty(&settings.clone().normalized()).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_has_no_claude_binary_path() {
        let settings = RouxSettings::default();
        assert_eq!(settings.claude_binary_path, None);
    }

    #[test]
    fn claude_binary_path_round_trips_through_json() {
        let mut settings = RouxSettings::default();
        settings.claude_binary_path = Some("/usr/local/bin/claude".to_string());
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: RouxSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.claude_binary_path, Some("/usr/local/bin/claude".to_string()));
    }

    #[test]
    fn settings_without_claude_binary_path_deserializes_as_none() {
        // Simulates loading a settings file from before this field existed
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "uiFontFamily": "sans-serif",
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
        assert_eq!(settings.claude_binary_path, None);
    }
}
