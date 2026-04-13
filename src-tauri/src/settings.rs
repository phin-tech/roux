use std::fs;
use std::path::PathBuf;
use thiserror::Error;

use crate::paths::roux_config_dir;
pub use roux_core::RouxSettings;

fn settings_path() -> PathBuf {
    roux_config_dir().join("settings.json")
}
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("{source}")]
    CreateSettingsDir {
        #[source]
        source: std::io::Error,
    },
    #[error("{source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("{source}")]
    Write {
        #[source]
        source: std::io::Error,
    },
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

pub fn save_settings(settings: &RouxSettings) -> Result<(), SettingsError> {
    let path = settings_path();
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|source| SettingsError::CreateSettingsDir { source })?;
    let json = serde_json::to_string_pretty(&settings.clone().normalized())
        .map_err(|source| SettingsError::Serialize { source })?;
    fs::write(&path, json).map_err(|source| SettingsError::Write { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

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

    #[test]
    fn settings_error_display_keeps_user_facing_message_shape() {
        let error = SettingsError::Write { source: io::Error::other("disk full") };

        assert_eq!(error.to_string(), "disk full");
    }
}
