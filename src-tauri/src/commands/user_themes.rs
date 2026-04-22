//! Tauri commands for user-supplied terminal themes loaded from
//! `~/.config/roux/themes/*.itermcolors`.

use std::fs;

use roux_core::models::{
    scan_user_terminal_themes as core_scan, UserTerminalTheme, UserThemeError,
};

use crate::paths::roux_config_dir;

fn themes_dir() -> std::path::PathBuf {
    roux_config_dir().join("themes")
}

/// List user-supplied terminal themes. The directory is created on first
/// call so it's discoverable by the user. Files that fail to parse are
/// dropped silently from the response (logged to stderr) — a single bad
/// file should not poison the whole picker.
#[tauri::command]
#[specta::specta]
pub(crate) fn list_user_terminal_themes() -> Vec<UserTerminalTheme> {
    let dir = themes_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("[user-themes] failed to create {}: {e}", dir.display());
        return Vec::new();
    }

    let mut out = Vec::new();
    for result in core_scan(&dir) {
        match result {
            Ok(theme) => out.push(theme),
            Err(e) => eprintln!("[user-themes] {}", describe(&e)),
        }
    }
    out
}

fn describe(err: &UserThemeError) -> String {
    err.to_string()
}

/// Absolute path to `~/.config/roux/themes/`. Created if missing so the
/// "Reveal" button always lands on a real folder.
#[tauri::command]
#[specta::specta]
pub(crate) fn user_themes_dir() -> String {
    let dir = themes_dir();
    let _ = fs::create_dir_all(&dir);
    dir.display().to_string()
}
