use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

const ROUX_SERVER_ID: &str = "roux";

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

pub(crate) fn plan_config(existing: Option<&str>, cli_path: &str) -> Result<ConfigPlan, String> {
    let mut root = match existing {
        Some(content) if !content.trim().is_empty() => serde_json::from_str::<Value>(content)
            .map_err(|e| format!("invalid MCP host config JSON: {e}"))?,
        _ => json!({}),
    };

    let root_obj =
        root.as_object_mut().ok_or_else(|| "MCP host config must be a JSON object".to_string())?;
    if !root_obj.contains_key("mcpServers") {
        root_obj.insert("mcpServers".into(), json!({}));
    }
    let servers = root_obj
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "MCP host config field `mcpServers` must be a JSON object".to_string())?;

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

pub(crate) fn plan_config_file(path: &PathBuf, cli_path: &str) -> Result<ConfigPlan, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("failed to read MCP host config: {e}")),
    };
    plan_config(content.as_deref(), cli_path)
}

pub(crate) fn write_config_file(path: &PathBuf, cli_path: &str) -> Result<ConfigPlan, String> {
    let plan = plan_config_file(path, cli_path)?;
    if plan.action == ConfigAction::Unchanged {
        return Ok(plan);
    }
    let parent = path
        .parent()
        .ok_or_else(|| "MCP host config path must include a parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create MCP host config directory: {e}"))?;
    let json = serde_json::to_string_pretty(&plan.next_config)
        .map_err(|e| format!("failed to serialize MCP host config: {e}"))?;
    atomic_write(path, json.as_bytes())?;
    Ok(plan)
}

fn atomic_write(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "MCP host config path must include a parent directory".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "MCP host config path must include a valid file name".to_string())?;
    let tmp_path = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));

    let write_result = (|| {
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("failed to create temporary MCP host config: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("failed to write temporary MCP host config: {e}"))?;
        file.sync_all().map_err(|e| format!("failed to sync temporary MCP host config: {e}"))?;
        std::fs::rename(&tmp_path, path)
            .map_err(|e| format!("failed to replace MCP host config: {e}"))
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_result
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
    fn plan_config_rejects_malformed_json() {
        let err = plan_config(Some("{ nope"), "/bin/roux-cli").unwrap_err();
        assert!(err.contains("invalid MCP host config JSON"));
    }
}
