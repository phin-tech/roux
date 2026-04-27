//! Keymap loader: embeds built-in presets via `include_str!`, reads the
//! user's `keymap.kdl`, and returns a fully-merged [`ParsedKeymap`] to the
//! frontend.
//!
//! The pure parser lives in `roux_core::models::keymap`; this module is the
//! I/O boundary. On first launch, if no `keymap.kdl` exists we write the
//! `default` preset so users can open an existing file and edit it.

use std::path::{Path, PathBuf};

use roux_core::{merge_keymaps, parse_keymap_kdl, KeymapParseError, ParsedKeymap};

const BUILTIN_PRESETS: &[(&str, &str)] = &[
    ("default", include_str!("keymap/presets/default.kdl")),
    ("tmux", include_str!("keymap/presets/tmux.kdl")),
];

pub fn keymap_path() -> PathBuf {
    crate::paths::roux_config_dir().join("keymap.kdl")
}

pub fn builtin_preset(name: &str) -> Option<&'static str> {
    BUILTIN_PRESETS.iter().find(|(n, _)| *n == name).map(|(_, src)| *src)
}

/// Load and fully resolve the active keymap. If the file does not exist,
/// writes the `default` preset to disk first so the file is available for
/// the user to edit.
pub fn load_active_keymap() -> Result<ParsedKeymap, KeymapLoadError> {
    let path = keymap_path();
    let text = read_or_bootstrap(&path)?;
    resolve(&text)
}

fn read_or_bootstrap(path: &Path) -> Result<String, KeymapLoadError> {
    if path.exists() {
        return std::fs::read_to_string(path).map_err(KeymapLoadError::Io);
    }
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(KeymapLoadError::Io)?;
    }
    // Bootstrap: write the default preset to disk so the user has a file to
    // edit. Using the preset contents directly (not just `preset "default"`)
    // keeps it self-documenting.
    let default_src = builtin_preset("default").ok_or(KeymapLoadError::MissingPreset)?;
    std::fs::write(path, default_src).map_err(KeymapLoadError::Io)?;
    Ok(default_src.to_string())
}

/// Parse and resolve the given keymap text. If the document has
/// `preset "<name>"`, that preset is parsed first and the user overlay
/// merged on top per [`merge_keymaps`].
pub fn resolve(src: &str) -> Result<ParsedKeymap, KeymapLoadError> {
    let parsed = parse_keymap_kdl(src).map_err(KeymapLoadError::Parse)?;
    match parsed.preset_ref.clone() {
        Some(name) => {
            let preset_src = builtin_preset(&name)
                .ok_or_else(|| KeymapLoadError::UnknownPreset(name.clone()))?;
            let preset = parse_keymap_kdl(preset_src).map_err(KeymapLoadError::Parse)?;
            Ok(merge_keymaps(preset, parsed))
        }
        None => Ok(parsed),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeymapLoadError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(KeymapParseError),
    #[error("unknown preset `{0}`")]
    UnknownPreset(String),
    #[error("missing built-in default preset")]
    MissingPreset,
}

impl KeymapLoadError {
    pub fn to_user_string(&self) -> String {
        self.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return the fully-resolved active keymap. On first launch this creates
/// `~/.config/roux/keymap.kdl` from the default preset before returning.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_keymap() -> Result<ParsedKeymap, String> {
    load_active_keymap().map_err(|e| e.to_user_string())
}

/// Write `contents` to `~/.config/roux/keymap.kdl`. Does not parse — the
/// caller (typically the Settings UI) is responsible for validating before
/// calling. On success the frontend typically follows up with `get_keymap`
/// to reload.
#[tauri::command]
#[specta::specta]
pub(crate) fn set_keymap(contents: String) -> Result<(), String> {
    let path = keymap_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create keymap directory: {e}"))?;
    }
    std::fs::write(&path, contents).map_err(|e| format!("failed to write keymap: {e}"))
}

/// Return the raw KDL source of a built-in preset. Used by the Settings UI
/// to offer "copy preset to keymap.kdl" and by docs.
#[tauri::command]
#[specta::specta]
pub(crate) fn get_builtin_keymap_preset(name: String) -> Result<String, String> {
    builtin_preset(&name).map(|s| s.to_string()).ok_or_else(|| format!("unknown preset `{name}`"))
}

/// Return the absolute path to the user's `keymap.kdl`. Used by the Settings
/// UI to offer "Open in editor".
#[tauri::command]
#[specta::specta]
pub(crate) fn get_keymap_path() -> String {
    keymap_path().to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_resolve_cleanly() {
        for (name, src) in BUILTIN_PRESETS {
            let parsed = parse_keymap_kdl(src)
                .unwrap_or_else(|e| panic!("preset {name} failed to parse: {e}"));
            assert!(
                parsed.warnings.is_empty(),
                "preset {name} produced warnings: {:?}",
                parsed.warnings
            );
            assert!(!parsed.prefixes.is_empty(), "preset {name} has no prefixes");
        }
    }

    #[test]
    fn resolve_without_preset_returns_parsed_as_is() {
        let src = r#"bind "Cmd+KeyK" "app.command-palette""#;
        let km = resolve(src).unwrap();
        assert_eq!(km.direct_binds.len(), 1);
    }

    #[test]
    fn resolve_with_preset_merges() {
        let src = r#"
            preset "default"
            bind "Cmd+KeyK" "app.quit"
        "#;
        let km = resolve(src).unwrap();
        // The user's Cmd+KeyK → app.quit should override the preset's
        // Cmd+KeyK → app.command-palette.
        let cmd_k = km
            .direct_binds
            .iter()
            .find(|b| match &b.key {
                roux_core::KeyRef::Physical { mods, code } => {
                    code == "KeyK" && mods.len() == 1 && mods.contains(&roux_core::Modifier::Cmd)
                }
                _ => false,
            })
            .expect("Cmd+KeyK bind present");
        assert_eq!(cmd_k.action, roux_core::KeymapAction::Command { id: "app.quit".into() });
    }

    #[test]
    fn unknown_preset_errors() {
        let src = r#"preset "zellij""#;
        let err = resolve(src).unwrap_err();
        assert!(matches!(err, KeymapLoadError::UnknownPreset(_)));
    }
}
