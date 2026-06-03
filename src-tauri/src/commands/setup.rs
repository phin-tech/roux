use crate::services::agent_notifications as agent_notifs;
use crate::services::setup as svc;

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntegrationDetection {
    binary_path: Option<String>,
    version: Option<String>,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    cli_installed: bool,
    gh_available: bool,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentNotificationProviderStatus {
    provider: String,
    label: String,
    status: String,
    detail: Option<String>,
    config_path: Option<String>,
    installable: bool,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentNotificationSetupStatus {
    providers: Vec<AgentNotificationProviderStatus>,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexNotificationConfigPreview {
    config_path: String,
    configured: bool,
    current_value: Option<String>,
    next_content: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_setup_status() -> SetupStatus {
    SetupStatus { cli_installed: svc::is_cli_installed(), gh_available: svc::is_gh_available() }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_agent_notification_setup_status() -> AgentNotificationSetupStatus {
    let cli_installed = svc::is_cli_installed();
    let cli_current = svc::is_cli_current();
    let hooks_installed = svc::is_hooks_installed();
    let claude_status = if hooks_installed && cli_current {
        "installed"
    } else if cli_installed && !cli_current {
        "stale"
    } else {
        "missing"
    };
    let claude_detail = if hooks_installed && cli_current {
        Some("Claude Code hooks are installed.".to_string())
    } else if cli_installed && !cli_current {
        match svc::installed_cli_version() {
            Some(version) => Some(format!(
                "CLI is stale; installed {}, bundled {}.",
                version,
                svc::bundled_cli_version()
            )),
            None => {
                Some(format!("CLI is stale; bundled version is {}.", svc::bundled_cli_version()))
            }
        }
    } else if !cli_installed {
        Some("Roux CLI is missing; configuring hooks will install it first.".to_string())
    } else {
        Some("Claude Code hooks are missing or incomplete.".to_string())
    };

    let codex_provider = match agent_notifs::codex_config_path() {
        Some(path) => match agent_notifs::preview_codex_notification_config_at(&path) {
            Ok(preview) => {
                let detail = if preview.configured {
                    Some("Codex TUI notifications are set to always.".to_string())
                } else if let Some(value) = preview.current_value {
                    Some(format!("notification_condition is currently `{value}`."))
                } else {
                    Some("notification_condition is not set.".to_string())
                };
                AgentNotificationProviderStatus {
                    provider: "codex".to_string(),
                    label: "Codex".to_string(),
                    status: if preview.configured { "installed" } else { "missing" }.to_string(),
                    detail,
                    config_path: Some(path.display().to_string()),
                    installable: true,
                }
            }
            Err(e) => AgentNotificationProviderStatus {
                provider: "codex".to_string(),
                label: "Codex".to_string(),
                status: "error".to_string(),
                detail: Some(e.to_string()),
                config_path: Some(path.display().to_string()),
                installable: true,
            },
        },
        None => AgentNotificationProviderStatus {
            provider: "codex".to_string(),
            label: "Codex".to_string(),
            status: "unavailable".to_string(),
            detail: Some("Could not determine home directory.".to_string()),
            config_path: None,
            installable: false,
        },
    };

    AgentNotificationSetupStatus {
        providers: vec![
            AgentNotificationProviderStatus {
                provider: "claude".to_string(),
                label: "Claude Code".to_string(),
                status: claude_status.to_string(),
                detail: claude_detail,
                config_path: None,
                installable: true,
            },
            codex_provider,
        ],
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_preview_codex_notification_config(
) -> Result<CodexNotificationConfigPreview, String> {
    let path = agent_notifs::codex_config_path()
        .ok_or_else(|| "Could not determine Codex config path".to_string())?;
    let preview =
        agent_notifs::preview_codex_notification_config_at(&path).map_err(|e| e.to_string())?;
    Ok(CodexNotificationConfigPreview {
        config_path: preview.config_path.display().to_string(),
        configured: preview.configured,
        current_value: preview.current_value,
        next_content: preview.next_content,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_configure_codex_notification_config() -> Result<(), String> {
    let path = agent_notifs::codex_config_path()
        .ok_or_else(|| "Could not determine Codex config path".to_string())?;
    agent_notifs::configure_codex_notification_config_at(&path).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_setup_needed() -> bool {
    !svc::is_cli_installed()
        || !svc::is_cli_current()
        || !svc::is_hooks_installed()
        || !svc::is_skill_installed()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn run_setup() -> Result<(), String> {
    svc::install_hooks().map_err(|e| e.to_string())?;
    svc::install_skill().map_err(|e| e.to_string())?;
    Ok(())
}

// ── Doctor panel ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorItem {
    /// Stable id used by the frontend to dispatch reinstall actions.
    id: String,
    /// Human-readable label.
    label: String,
    /// "installed" | "missing" | "unavailable"
    status: String,
    /// Optional detail string (install path, version, reason).
    detail: Option<String>,
    /// Whether this item has a reinstall action.
    installable: bool,
}

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorStatus {
    notices: Vec<String>,
    items: Vec<DoctorItem>,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_doctor_status() -> DoctorStatus {
    let cli_installed = svc::is_cli_installed();
    let cli_installed_version = svc::installed_cli_version();
    let cli_bundled_version = svc::bundled_cli_version();
    let cli_current = cli_installed_version.as_deref() == Some(cli_bundled_version);
    let hooks_installed = svc::is_hooks_installed();
    let skill_installed = svc::is_skill_installed();
    let gh_available = svc::is_gh_available();

    let cli_status = if !cli_installed {
        "missing"
    } else if !cli_current {
        "stale"
    } else {
        "installed"
    };
    let cli_detail = match (&cli_installed_version, cli_current) {
        (Some(v), true) => Some(format!("version {}", v)),
        (Some(v), false) => Some(format!("installed {} — bundled {}", v, cli_bundled_version)),
        (None, _) if cli_installed => Some("version unknown".to_string()),
        _ => None,
    };

    let skill_detail = crate::skill::installed_version()
        .map(|v| format!("version {}", v))
        .or_else(|| crate::skill::skill_install_path().map(|p| p.display().to_string()));

    DoctorStatus {
        notices: svc::startup_notices(),
        items: vec![
            DoctorItem {
                id: "cli".to_string(),
                label: "Roux CLI".to_string(),
                status: cli_status.to_string(),
                detail: cli_detail,
                installable: true,
            },
            DoctorItem {
                id: "hooks".to_string(),
                label: "Claude Code hooks".to_string(),
                status: if hooks_installed { "installed" } else { "missing" }.to_string(),
                detail: None,
                installable: true,
            },
            DoctorItem {
                id: "skill".to_string(),
                label: "Claude Code skill".to_string(),
                status: if skill_installed { "installed" } else { "missing" }.to_string(),
                detail: skill_detail,
                installable: true,
            },
            DoctorItem {
                id: "gh".to_string(),
                label: "GitHub CLI (gh)".to_string(),
                status: if gh_available { "installed" } else { "unavailable" }.to_string(),
                detail: if gh_available {
                    None
                } else {
                    Some("Optional. Install from https://cli.github.com.".to_string())
                },
                installable: false,
            },
        ],
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) fn reinstall_cli() -> Result<(), String> {
    svc::install_cli().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn reinstall_hooks() -> Result<(), String> {
    svc::install_hooks().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn reinstall_skill() -> Result<(), String> {
    svc::install_skill().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn install_all_missing() -> Result<(), String> {
    let cli_needs_install = !svc::is_cli_installed() || !svc::is_cli_current();
    if !svc::is_hooks_installed() {
        svc::install_hooks().map_err(|e| e.to_string())?;
    } else if cli_needs_install {
        svc::install_cli().map_err(|e| e.to_string())?;
    }
    if !svc::is_skill_installed() {
        svc::install_skill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_gh() -> IntegrationDetection {
    tauri::async_runtime::spawn_blocking(|| {
        let result = crate::services::setup::detect_gh();
        IntegrationDetection {
            binary_path: result.as_ref().map(|(p, _)| p.clone()),
            version: result.map(|(_, v)| v),
        }
    })
    .await
    .unwrap_or(IntegrationDetection { binary_path: None, version: None })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_git() -> IntegrationDetection {
    tauri::async_runtime::spawn_blocking(|| {
        let result = crate::services::setup::detect_git();
        IntegrationDetection {
            binary_path: result.as_ref().map(|(p, _)| p.clone()),
            version: result.map(|(_, v)| v),
        }
    })
    .await
    .unwrap_or(IntegrationDetection { binary_path: None, version: None })
}
