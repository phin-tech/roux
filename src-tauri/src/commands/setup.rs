use crate::services::setup as svc;

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    cli_installed: bool,
    gh_available: bool,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn check_setup_status() -> SetupStatus {
    SetupStatus {
        cli_installed: svc::is_cli_installed(),
        gh_available: svc::is_gh_available(),
    }
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

#[tauri::command]
#[specta::specta]
pub(crate) fn check_nono_installed() -> bool {
    svc::is_command_available("nono")
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_nono_profiles() -> Vec<String> {
    svc::list_nono_profiles()
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
    // Reinstalling the CLI is a side effect of install_hooks (it copies the
    // binary into ~/.local/bin before wiring up settings.json), so it's the
    // cheapest way to get a fresh CLI on disk today.
    svc::install_hooks().map_err(|e| e.to_string())
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
    // install_hooks copies the CLI to ~/.local/bin as a side effect, so this
    // also covers "CLI is stale" (version mismatch with bundled).
    if !svc::is_cli_installed() || !svc::is_cli_current() || !svc::is_hooks_installed() {
        svc::install_hooks().map_err(|e| e.to_string())?;
    }
    if !svc::is_skill_installed() {
        svc::install_skill().map_err(|e| e.to_string())?;
    }
    Ok(())
}
