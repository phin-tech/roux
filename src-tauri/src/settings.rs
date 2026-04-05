use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_THEME: &str = "deep-blue";

fn normalize_theme(theme: &str) -> String {
    match theme {
        "dark" | "deep-blue" => DEFAULT_THEME.to_string(),
        "steel-amber" | "slate-emerald" | "graphite-rose" => theme.to_string(),
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
    pub additional_flags: Vec<String>,
    pub task_panel_split: f64,
    pub task_panel_collapsed: bool,
}

impl Default for RouxSettings {
    fn default() -> Self {
        Self {
            tab_position: "left".to_string(),
            tab_width: 260,
            font_size: 14,
            font_family: "JetBrains Mono, IBM Plex Mono, SFMono-Regular, monospace".to_string(),
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
            additional_flags: vec![],
            task_panel_split: 0.4,
            task_panel_collapsed: false,
        }
    }
}

impl RouxSettings {
    pub fn normalized(mut self) -> Self {
        self.theme = normalize_theme(&self.theme);
        self
    }
}

fn config_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load_settings() -> RouxSettings {
    let path = settings_path();
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<RouxSettings>(&content).unwrap_or_default().normalized()
    } else {
        RouxSettings::default()
    }
}

pub fn save_settings(settings: &RouxSettings) -> Result<(), String> {
    let path = settings_path();
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json =
        serde_json::to_string_pretty(&settings.clone().normalized()).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}
