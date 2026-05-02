use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

const ROUX_SERVER_ID: &str = "roux";
const TEMP_FILE_ATTEMPTS: u32 = 100;

type McpConfigResult<T> = std::result::Result<T, McpConfigError>;

#[derive(Debug, Error)]
pub(crate) enum McpConfigError {
    #[error("invalid MCP host config JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("MCP host config must be a JSON object")]
    InvalidRootObject,
    #[error("MCP host config field `mcpServers` must be a JSON object")]
    InvalidMcpServersField,
    #[error("failed to read MCP host config: {0}")]
    ReadConfig(#[source] std::io::Error),
    #[error("MCP host config path must include a parent directory")]
    MissingParentDir,
    #[error("failed to create MCP host config directory: {0}")]
    CreateConfigDir(#[source] std::io::Error),
    #[error("failed to serialize MCP host config: {0}")]
    SerializeConfig(#[source] serde_json::Error),
    #[error("MCP host config path must include a valid file name")]
    InvalidFileName,
    #[error("failed to create temporary MCP host config: {0}")]
    CreateTemp(#[source] std::io::Error),
    #[error("failed to create temporary MCP host config: exhausted temporary file name attempts")]
    TempNameExhausted,
    #[error("failed to write temporary MCP host config: {0}")]
    WriteTemp(#[source] std::io::Error),
    #[error("failed to sync temporary MCP host config: {0}")]
    SyncTemp(#[source] std::io::Error),
    #[error("failed to replace MCP host config: {0}")]
    ReplaceConfig(#[source] std::io::Error),
}

pub(crate) fn roux_cli_command_path() -> PathBuf {
    crate::paths::roux_config_dir().join("bin").join(crate::platform::roux_cli_file_name())
}

pub(crate) fn claude_desktop_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|base| base.join("Claude").join("claude_desktop_config.json"))
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        dirs::config_dir().map(|base| base.join("Claude").join("claude_desktop_config.json"))
    }
}

pub(crate) fn target_entry(cli_path: &str) -> Value {
    json!({
        "command": cli_path,
        "args": ["mcp"],
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigAction {
    Create,
    Update,
    Unchanged,
}

impl ConfigAction {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ConfigAction::Create => "create",
            ConfigAction::Update => "update",
            ConfigAction::Unchanged => "unchanged",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigPlan {
    pub(crate) action: ConfigAction,
    pub(crate) configured: bool,
    pub(crate) current_entry: Option<Value>,
    pub(crate) next_entry: Value,
    pub(crate) next_config: Value,
}

pub(crate) fn plan_config(existing: Option<&str>, cli_path: &str) -> McpConfigResult<ConfigPlan> {
    let mut root = match existing {
        Some(content) if !content.trim().is_empty() => {
            serde_json::from_str::<Value>(content).map_err(McpConfigError::InvalidJson)?
        }
        _ => json!({}),
    };

    let root_obj = root.as_object_mut().ok_or(McpConfigError::InvalidRootObject)?;
    if !root_obj.contains_key("mcpServers") {
        root_obj.insert("mcpServers".into(), json!({}));
    }
    let servers = root_obj
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or(McpConfigError::InvalidMcpServersField)?;

    let current_entry = servers.get(ROUX_SERVER_ID).cloned();
    let next_entry = merge_roux_entry(current_entry.as_ref(), cli_path);
    let configured = current_entry.as_ref() == Some(&next_entry);
    let action = if configured {
        ConfigAction::Unchanged
    } else if current_entry.is_some() {
        ConfigAction::Update
    } else {
        ConfigAction::Create
    };
    servers.insert(ROUX_SERVER_ID.into(), next_entry.clone());

    Ok(ConfigPlan { action, configured, current_entry, next_entry, next_config: root })
}

fn merge_roux_entry(current_entry: Option<&Value>, cli_path: &str) -> Value {
    let desired_entry = target_entry(cli_path);
    let Some(current_object) = current_entry.and_then(Value::as_object) else {
        return desired_entry;
    };
    let Some(desired_object) = desired_entry.as_object() else {
        return desired_entry;
    };

    let mut merged = current_object.clone();
    for (key, value) in desired_object {
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

pub(crate) fn plan_config_file(path: &PathBuf, cli_path: &str) -> McpConfigResult<ConfigPlan> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(McpConfigError::ReadConfig(e)),
    };
    plan_config(content.as_deref(), cli_path)
}

pub(crate) fn write_config_file(path: &PathBuf, cli_path: &str) -> McpConfigResult<ConfigPlan> {
    let plan = plan_config_file(path, cli_path)?;
    if plan.action == ConfigAction::Unchanged {
        return Ok(plan);
    }
    let parent = path.parent().ok_or(McpConfigError::MissingParentDir)?;
    std::fs::create_dir_all(parent).map_err(McpConfigError::CreateConfigDir)?;
    let json =
        serde_json::to_string_pretty(&plan.next_config).map_err(McpConfigError::SerializeConfig)?;
    atomic_write(path, json.as_bytes())?;
    Ok(plan)
}

fn atomic_write(path: &PathBuf, bytes: &[u8]) -> McpConfigResult<()> {
    let parent = path.parent().ok_or(McpConfigError::MissingParentDir)?;
    let file_name =
        path.file_name().and_then(|name| name.to_str()).ok_or(McpConfigError::InvalidFileName)?;
    let (tmp_path, mut file) = create_temp_config_file(parent, file_name)?;

    let write_result = (|| {
        file.write_all(bytes).map_err(McpConfigError::WriteTemp)?;
        file.sync_all().map_err(McpConfigError::SyncTemp)?;
        std::fs::rename(&tmp_path, path).map_err(McpConfigError::ReplaceConfig)
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_result
}

fn create_temp_config_file(
    parent: &std::path::Path,
    file_name: &str,
) -> McpConfigResult<(PathBuf, std::fs::File)> {
    for attempt in 0..TEMP_FILE_ATTEMPTS {
        let tmp_path = parent.join(format!(".{file_name}.tmp.{}.{}", std::process::id(), attempt));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp_path) {
            Ok(file) => return Ok((tmp_path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(McpConfigError::CreateTemp(e)),
        }
    }
    Err(McpConfigError::TempNameExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_config_creates_mcp_servers_when_missing() {
        let plan = plan_config(Some(r#"{"other": true}"#), "/bin/roux-cli").unwrap();

        assert_eq!(plan.action, ConfigAction::Create);
        assert_eq!(plan.next_config["other"], true);
        assert_eq!(plan.next_config["mcpServers"]["roux"]["command"], "/bin/roux-cli");
        assert_eq!(plan.next_config["mcpServers"]["roux"]["args"][0], "mcp");
    }

    #[test]
    fn plan_config_updates_only_roux_entry() {
        let existing = r#"{
            "mcpServers": {
                "other": { "command": "node" },
                "roux": { "command": "/old/roux-cli", "args": ["mcp"] }
            }
        }"#;
        let plan = plan_config(Some(existing), "/new/roux-cli").unwrap();

        assert_eq!(plan.action, ConfigAction::Update);
        assert_eq!(plan.next_config["mcpServers"]["other"]["command"], "node");
        assert_eq!(plan.current_entry.unwrap()["command"], "/old/roux-cli");
        assert_eq!(plan.next_config["mcpServers"]["roux"]["command"], "/new/roux-cli");
    }

    #[test]
    fn plan_config_preserves_unknown_roux_entry_fields() {
        let existing = r#"{
            "mcpServers": {
                "roux": {
                    "command": "/old/roux-cli",
                    "args": ["mcp"],
                    "env": { "ROUX_LOG": "debug" }
                }
            }
        }"#;
        let plan = plan_config(Some(existing), "/new/roux-cli").unwrap();

        assert_eq!(plan.action, ConfigAction::Update);
        assert_eq!(plan.next_config["mcpServers"]["roux"]["command"], "/new/roux-cli");
        assert_eq!(plan.next_config["mcpServers"]["roux"]["args"][0], "mcp");
        assert_eq!(plan.next_config["mcpServers"]["roux"]["env"]["ROUX_LOG"], "debug");
    }

    #[test]
    fn plan_config_reports_unchanged_when_entry_matches() {
        let existing = r#"{
            "mcpServers": {
                "roux": { "command": "/bin/roux-cli", "args": ["mcp"] }
            }
        }"#;
        let plan = plan_config(Some(existing), "/bin/roux-cli").unwrap();

        assert_eq!(plan.action, ConfigAction::Unchanged);
        assert!(plan.configured);
    }

    #[test]
    fn write_config_file_skips_unchanged_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude_desktop_config.json");
        let existing = r#"{ "mcpServers": { "roux": { "env": { "ROUX_LOG": "debug" }, "command": "/bin/roux-cli", "args": ["mcp"] } } }"#;
        std::fs::write(&path, existing).unwrap();

        let plan = write_config_file(&path, "/bin/roux-cli").unwrap();

        assert_eq!(plan.action, ConfigAction::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), existing);
    }

    #[test]
    fn write_config_file_creates_parent_directory_and_writes_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Claude").join("claude_desktop_config.json");

        let plan = write_config_file(&path, "/bin/roux-cli").unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let written_json: Value = serde_json::from_str(&written).unwrap();

        assert_eq!(plan.action, ConfigAction::Create);
        assert_eq!(written_json["mcpServers"]["roux"]["command"], "/bin/roux-cli");
        assert_eq!(written_json["mcpServers"]["roux"]["args"][0], "mcp");
    }

    #[test]
    fn create_temp_config_file_skips_existing_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let file_name = "claude_desktop_config.json";
        let existing = dir.path().join(format!(".{file_name}.tmp.{}.0", std::process::id()));
        std::fs::write(&existing, "already here").unwrap();

        let (tmp_path, _file) = create_temp_config_file(dir.path(), file_name).unwrap();

        assert_ne!(tmp_path, existing);
        assert!(tmp_path.ends_with(format!(".{file_name}.tmp.{}.1", std::process::id())));
    }

    #[test]
    fn plan_config_rejects_malformed_json() {
        let err = plan_config(Some("{ nope"), "/bin/roux-cli").unwrap_err();
        assert!(err.to_string().contains("invalid MCP host config JSON"));
    }
}
