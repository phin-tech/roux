use crate::services::mcp_config as svc;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum McpHostId {
    ClaudeDesktop,
}

impl McpHostId {
    fn as_str(self) -> &'static str {
        match self {
            McpHostId::ClaudeDesktop => "claudeDesktop",
        }
    }

    fn label(self) -> &'static str {
        match self {
            McpHostId::ClaudeDesktop => "Claude Desktop",
        }
    }

    fn config_path(self) -> Option<std::path::PathBuf> {
        match self {
            McpHostId::ClaudeDesktop => svc::claude_desktop_config_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpHostStatus {
    id: McpHostId,
    label: String,
    config_path: Option<String>,
    config_exists: bool,
    configured: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpStatus {
    enabled: bool,
    cli_installed: bool,
    cli_current: bool,
    cli_path: String,
    last_configured_host: Option<String>,
    last_configured_at_ms: Option<u64>,
    hosts: Vec<McpHostStatus>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpHostConfigPreview {
    host: McpHostId,
    label: String,
    config_path: String,
    config_exists: bool,
    action: String,
    configured: bool,
    current_entry_json: Option<String>,
    next_entry_json: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_mcp_status(state: tauri::State<AppState>) -> McpStatus {
    let settings = state.settings.lock().unwrap().clone();
    let cli_path = svc::roux_cli_command_path();
    McpStatus {
        enabled: settings.mcp_enabled,
        cli_installed: crate::services::setup::is_cli_installed(),
        cli_current: crate::services::setup::is_cli_current(),
        cli_path: cli_path.to_string_lossy().to_string(),
        last_configured_host: settings.mcp_last_configured_host,
        last_configured_at_ms: settings.mcp_last_configured_at_ms,
        hosts: vec![host_status(McpHostId::ClaudeDesktop, &cli_path.to_string_lossy())],
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_preview_mcp_host_config(host: McpHostId) -> Result<McpHostConfigPreview, String> {
    preview_host(host)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_configure_mcp_host(
    host: McpHostId,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<McpHostConfigPreview, String> {
    let path = host
        .config_path()
        .ok_or_else(|| format!("{} config path is unavailable on this platform", host.label()))?;
    let cli_path = svc::roux_cli_command_path().to_string_lossy().to_string();
    let plan = svc::write_config_file(&path, &cli_path)?;
    if let Err(error) = record_configured_host(host, state, app) {
        eprintln!("roux: failed to record MCP host configuration metadata: {error}");
    }
    Ok(preview_from_plan(host, path, plan))
}

fn record_configured_host(
    host: McpHostId,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let configured_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let settings = crate::services::settings::update_mcp_config_metadata(
        host.as_str().to_string(),
        configured_at,
    )
    .map_err(|e| e.to_string())?;
    let mut current_settings = state.settings.lock().map_err(|e| e.to_string())?;
    current_settings.mcp_last_configured_host = settings.mcp_last_configured_host;
    current_settings.mcp_last_configured_at_ms = settings.mcp_last_configured_at_ms;
    let settings = current_settings.clone();
    drop(current_settings);
    app.emit("settings-changed", &settings).map_err(|e| e.to_string())
}

fn host_status(host: McpHostId, cli_path: &str) -> McpHostStatus {
    let Some(path) = host.config_path() else {
        return McpHostStatus {
            id: host,
            label: host.label().to_string(),
            config_path: None,
            config_exists: false,
            configured: false,
            error: Some("config path unavailable on this platform".into()),
        };
    };

    let config_exists = path.exists();
    match svc::plan_config_file(&path, cli_path) {
        Ok(plan) => McpHostStatus {
            id: host,
            label: host.label().to_string(),
            config_path: Some(path.to_string_lossy().to_string()),
            config_exists,
            configured: plan.configured,
            error: None,
        },
        Err(error) => McpHostStatus {
            id: host,
            label: host.label().to_string(),
            config_path: Some(path.to_string_lossy().to_string()),
            config_exists,
            configured: false,
            error: Some(error),
        },
    }
}

fn preview_host(host: McpHostId) -> Result<McpHostConfigPreview, String> {
    let path = host
        .config_path()
        .ok_or_else(|| format!("{} config path is unavailable on this platform", host.label()))?;
    let cli_path = svc::roux_cli_command_path().to_string_lossy().to_string();
    let plan = svc::plan_config_file(&path, &cli_path)?;
    Ok(preview_from_plan(host, path, plan))
}

fn preview_from_plan(
    host: McpHostId,
    path: std::path::PathBuf,
    plan: svc::ConfigPlan,
) -> McpHostConfigPreview {
    McpHostConfigPreview {
        host,
        label: host.label().to_string(),
        config_exists: path.exists(),
        config_path: path.to_string_lossy().to_string(),
        action: plan.action.as_str().to_string(),
        configured: plan.configured,
        current_entry_json: plan
            .current_entry
            .and_then(|entry| serde_json::to_string_pretty(&entry).ok()),
        next_entry_json: serde_json::to_string_pretty(&plan.next_entry).unwrap_or_default(),
    }
}
