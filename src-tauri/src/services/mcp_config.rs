use serde_json::{json, Value};
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
    let next_entry = target_entry(cli_path);

    if !root_obj.contains_key("mcpServers") {
        root_obj.insert("mcpServers".into(), json!({}));
    }
    let servers = root_obj
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "MCP host config field `mcpServers` must be a JSON object".to_string())?;

    let current_entry = servers.get(ROUX_SERVER_ID).cloned();
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create MCP host config directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&plan.next_config)
        .map_err(|e| format!("failed to serialize MCP host config: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("failed to write MCP host config: {e}"))?;
    Ok(plan)
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
    fn plan_config_rejects_malformed_json() {
        let err = plan_config(Some("{ nope"), "/bin/roux-cli").unwrap_err();
        assert!(err.contains("invalid MCP host config JSON"));
    }
}
