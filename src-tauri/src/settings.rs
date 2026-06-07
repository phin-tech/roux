use std::fs;
use std::path::{Path, PathBuf};
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
    load_settings_from_path(&path)
}

fn load_settings_from_path(path: &Path) -> RouxSettings {
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_default();
        roux_core::load_settings_json_with_kanban_workflow(&content, |workflow_path| {
            let resolved = resolve_workflow_path(path, workflow_path);
            fs::read_to_string(&resolved).map_err(|err| {
                format!("failed to read workflow JSON {}: {err}", resolved.display())
            })
        })
    } else {
        RouxSettings::default()
    }
}

pub fn load_kanban_workflow_for_settings(settings: RouxSettings) -> RouxSettings {
    let path = settings_path();
    roux_core::load_kanban_workflow_for_settings(settings, |workflow_path| {
        let resolved = resolve_workflow_path(&path, workflow_path);
        let content = fs::read_to_string(&resolved)
            .map_err(|err| format!("failed to read workflow JSON {}: {err}", resolved.display()))?;
        roux_core::parse_kanban_workflow_json(&content)
            .map_err(|err| format!("failed to load workflow JSON {}: {err}", resolved.display()))
    })
}

fn resolve_workflow_path(settings_path: &Path, workflow_path: &str) -> PathBuf {
    let path = PathBuf::from(workflow_path);
    if path.is_absolute() {
        path
    } else {
        settings_path.parent().unwrap_or_else(|| Path::new(".")).join(path)
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
        let settings = RouxSettings {
            claude_binary_path: Some("/usr/local/bin/claude".to_string()),
            ..RouxSettings::default()
        };
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

    #[test]
    fn load_settings_from_path_applies_relative_workflow_path() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        fs::write(
            dir.path().join("workflow.json"),
            r#"{
                "id": "personal",
                "label": "Personal Flow",
                "phases": {
                    "planning": {
                        "category": "planning",
                        "label": "Shape",
                        "agentProfile": null,
                        "instructions": "Plan from file.",
                        "stages": {}
                    },
                    "implementation": {
                        "category": "implementation",
                        "label": "Build",
                        "agentProfile": "codex",
                        "instructions": "Implement from file.",
                        "stages": {}
                    },
                    "review": {
                        "category": "review",
                        "label": "Review",
                        "agentProfile": null,
                        "instructions": "",
                        "stages": {
                            "local_review": {
                                "label": "Local QA",
                                "agentProfile": null,
                                "instructions": "Check locally."
                            },
                            "pr_review": {
                                "label": "Team Review",
                                "agentProfile": null,
                                "instructions": "Check PR."
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let mut persisted = RouxSettings::default();
        persisted.kanban.workflow_path = Some("workflow.json".into());
        fs::write(&settings_path, serde_json::to_string_pretty(&persisted).unwrap()).unwrap();

        let settings = super::load_settings_from_path(&settings_path);

        assert_eq!(settings.kanban.workflow_path.as_deref(), Some("workflow.json"));
        assert_eq!(settings.kanban.workflow.id, "personal");
        assert_eq!(settings.kanban.workflow.label, "Personal Flow");
        assert_eq!(settings.kanban.planning_instructions(), "Plan from file.");
        assert!(settings.kanban.workflow_load_error.is_none());
    }

    #[test]
    fn load_settings_from_path_preserves_inline_workflow_when_external_load_fails() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let mut persisted = RouxSettings::default();
        persisted.kanban.workflow_path = Some("missing.json".into());
        persisted.kanban.workflow.id = "inline".into();
        persisted.kanban.workflow.label = "Inline Flow".into();
        persisted.kanban.workflow.phases.get_mut("planning").unwrap().label = "Plan Inline".into();
        persisted.kanban.workflow.phases.get_mut("planning").unwrap().instructions =
            "Inline planning.".into();
        fs::write(&settings_path, serde_json::to_string_pretty(&persisted).unwrap()).unwrap();

        let settings = super::load_settings_from_path(&settings_path);

        assert_eq!(settings.kanban.workflow.id, "inline");
        assert_eq!(settings.kanban.planning_instructions(), "Inline planning.");
        assert!(settings.kanban.workflow_load_error.as_deref().unwrap().contains("missing.json"));
    }
}
