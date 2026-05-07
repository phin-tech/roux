//! Tauri commands for the smol-machines sidebar.
//!
//! Pattern-mirrors `commands::worktrees`: each command that shells out to
//! `smolvm` is `async` and runs the subprocess via
//! `tauri::async_runtime::spawn_blocking` so the webview thread never
//! blocks on the CLI.

use serde::Serialize;

use crate::services::library as library_svc;
use crate::services::smolvm as svc;
use crate::state::AppState;

/// Resolve the install script for `(agent, distro)` with the full
/// Phase 2.7 layered chain: library item → bootstrap config TOML →
/// hardcoded built-in. The library lookup uses Roux's standard layer
/// stack (global + sources + active-repo if a session is in scope).
fn resolve_install_script(
    state: &AppState,
    agent: roux_smolvm::KnownAgent,
    distro: &str,
) -> String {
    if let Ok(settings) = state.settings.lock().map(|guard| guard.clone()) {
        let global_root = settings
            .notes_vault_root
            .as_ref()
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(crate::paths::default_notes_vault_root);
        let layers = library_svc::layers(
            global_root,
            &settings.library_sources,
            &crate::paths::roux_config_dir().join("library-sources"),
            None, // no active repo — install is panel-driven, not session-driven
        );
        if let Some(script) = library_svc::find_smolvm_script_in_layers(
            &layers,
            agent.binary_name(),
            distro,
        ) {
            return script;
        }
    }

    // Fall back to bootstrap config TOML / built-in.
    let config = roux_smolvm::BootstrapConfig::load_or_default(&svc::bootstrap_config_path());
    config.agent_script(agent, distro)
}

/// Return-shape for the activity-rail detection probe. Mirrors
/// `IntegrationDetection` in `commands::setup` but lives here so the
/// smol-machines bindings stay self-contained.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SmolvmDetection {
    pub binary_path: Option<String>,
    pub version: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_detect_smolvm() -> SmolvmDetection {
    tauri::async_runtime::spawn_blocking(|| match svc::resolve_smolvm_binary() {
        Some(install) => SmolvmDetection {
            binary_path: Some(install.path.to_string_lossy().into_owned()),
            version: Some(install.version),
        },
        None => SmolvmDetection { binary_path: None, version: None },
    })
    .await
    .unwrap_or(SmolvmDetection { binary_path: None, version: None })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_smol_machines() -> Result<Vec<roux_core::SmolMachine>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::list_machines(&install.path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("list_smol_machines task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_start_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::start_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("start_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_stop_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::stop_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("stop_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_delete_smol_machine(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::delete_machine(&install.path, &name).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("delete_smol_machine task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_create_smol_machine(
    request: roux_core::SmolMachineCreateRequest,
) -> Result<(), String> {
    let smolfile_path = request.smolfile_path.clone();
    let machine_name = request.name.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_core::smolvm::create_machine(&install.path, &request).map_err(|e| e.to_string())?;
        // Track the Smolfile link so "Persist via Smolfile" can write
        // back to it later. Only write the registry when a Smolfile
        // was actually provided — machines created without one stay
        // unlinked and the panel falls through to the "create + recreate"
        // flow.
        if let Some(path) = smolfile_path.as_deref().filter(|p| !p.trim().is_empty()) {
            svc::record_smolfile_path(&machine_name, std::path::Path::new(path))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("create_smol_machine task panicked: {e}"))?
}

/// `which`-style probe inside a guest. Returns the resolved guest path
/// when the binary is on the guest's PATH, `None` when it isn't, or an
/// error string when the smolvm CLI itself fails. Used by the
/// frontend's profile-replay preflight (see `profileRunner.ts`) to
/// short-circuit `claude`/`codex` startup commands when the agent
/// isn't installed in the guest.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_check_smolvm_binary(
    machine_name: String,
    binary: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary()
            .ok_or_else(|| "smolvm is not installed".to_string())?;
        roux_smolvm::check_guest_binary(&install.path, &machine_name, &binary)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("check_smolvm_binary task panicked: {e}"))?
}

/// Install a known agent inside a smol machine. v1 supports `"claude"`
/// and `"codex"` (case-insensitive). The install pipeline reads
/// `~/.config/roux/smolvm-bootstraps.toml` if present so users can
/// customize prereqs / install commands / distro mapping without
/// rebuilding Roux. Missing or malformed file falls back to
/// hardcoded defaults.
///
/// Synchronous from the user's perspective — the panel shows a
/// spinner until the install finishes (typically <60s).
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_install_smolvm_agent(
    machine_name: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let known = roux_smolvm::KnownAgent::from_str(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Resolve script on the async-runtime thread (needs AppState
    // lock) before handing off to the blocking subprocess work.
    let install = svc::resolve_smolvm_binary()
        .ok_or_else(|| "smolvm is not installed".to_string())?;
    let machines =
        roux_smolvm::list_machines(&install.path).map_err(|e| e.to_string())?;
    let image = machines.iter().find(|m| m.name == machine_name).and_then(|m| m.image.clone());
    let distro = roux_smolvm::distro_from_image(image.as_deref());
    let script = resolve_install_script(&state, known, distro);

    tauri::async_runtime::spawn_blocking(move || {
        roux_smolvm::run_install_script(&install.path, &machine_name, known, &script)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("install_smolvm_agent task panicked: {e}"))?
}

/// Outcome of `cmd_install_smolvm_agent_persist`. Mirrors
/// `roux_smolvm::AppendOutcome` plus a `NeedsRecreate` variant the
/// frontend uses to render the "create Smolfile + recreate machine"
/// confirm modal.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum PersistOutcome {
    /// Line was appended to an existing Smolfile.
    Appended { smolfile_path: String },
    /// An identical line was already present; file untouched.
    AlreadyPresent { smolfile_path: String },
    /// The machine has no linked Smolfile. The frontend should show a
    /// confirm modal explaining the create + recreate flow, then call
    /// `cmd_install_smolvm_agent_recreate` if the user confirms.
    NeedsRecreate {
        proposed_smolfile_path: String,
        image: Option<String>,
        script: String,
    },
}

/// Persist an agent install by writing into the machine's linked
/// Smolfile `[dev].init`. When no Smolfile is linked, returns
/// `NeedsRecreate` so the frontend can prompt the user before any
/// destructive action.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_install_smolvm_agent_persist(
    machine_name: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<PersistOutcome, String> {
    let known = roux_smolvm::KnownAgent::from_str(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Resolve script on the async runtime thread (needs AppState
    // lock for library lookup) before the blocking section.
    let install = svc::resolve_smolvm_binary()
        .ok_or_else(|| "smolvm is not installed".to_string())?;
    let machines =
        roux_smolvm::list_machines(&install.path).map_err(|e| e.to_string())?;
    let machine = machines
        .iter()
        .find(|m| m.name == machine_name)
        .cloned()
        .ok_or_else(|| format!("smol machine '{machine_name}' not found"))?;
    let distro = roux_smolvm::distro_from_image(machine.image.as_deref());
    let script = resolve_install_script(&state, known, distro);

    tauri::async_runtime::spawn_blocking(move || -> Result<PersistOutcome, String> {
        if let Some(smolfile_path) = svc::smolfile_path_for_machine(&machine_name) {
            // Linked: append in place.
            let outcome = roux_smolvm::smolfile_append_init(&smolfile_path, &script)
                .map_err(|e| e.to_string())?;
            let path_str = smolfile_path.to_string_lossy().into_owned();
            Ok(match outcome {
                roux_smolvm::AppendOutcome::Appended => {
                    PersistOutcome::Appended { smolfile_path: path_str }
                }
                roux_smolvm::AppendOutcome::AlreadyPresent => {
                    PersistOutcome::AlreadyPresent { smolfile_path: path_str }
                }
            })
        } else {
            // No Smolfile linked → propose recreate. The frontend
            // shows a confirm modal and calls
            // `cmd_install_smolvm_agent_recreate` if the user agrees.
            let proposed = crate::paths::roux_config_dir()
                .join("smolmachines")
                .join(format!("{machine_name}.toml"));
            Ok(PersistOutcome::NeedsRecreate {
                proposed_smolfile_path: proposed.to_string_lossy().into_owned(),
                image: machine.image.clone(),
                script,
            })
        }
    })
    .await
    .map_err(|e| format!("install_smolvm_agent_persist task panicked: {e}"))?
}

/// Destructive: regenerate the machine from a Roux-managed Smolfile.
/// Only called after the user confirms the modal that follows
/// `PersistOutcome::NeedsRecreate`. Stops + deletes + recreates +
/// starts the machine; records the new Smolfile link so subsequent
/// "Persist via Smolfile" calls take the in-place append path.
///
/// Best-effort with breadcrumbs: if recreate fails after delete, the
/// generated Smolfile path is in the error message so the user can
/// recover via `smolvm machine create <name> -s <path>`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_install_smolvm_agent_recreate(
    machine_name: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let known = roux_smolvm::KnownAgent::from_str(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Resolve script + read machine state on the async thread (needs
    // AppState) before the blocking destructive section.
    let install = svc::resolve_smolvm_binary()
        .ok_or_else(|| "smolvm is not installed".to_string())?;
    let machines =
        roux_smolvm::list_machines(&install.path).map_err(|e| e.to_string())?;
    let machine = machines
        .iter()
        .find(|m| m.name == machine_name)
        .cloned()
        .ok_or_else(|| format!("smol machine '{machine_name}' not found"))?;
    let image = machine.image.clone().ok_or_else(|| {
        format!(
            "smol machine '{machine_name}' has no image — cannot recreate. Recreate it manually with `smolvm machine create -s <smolfile>`."
        )
    })?;
    let network = machine.network;
    let ssh_agent = machine.ssh_agent;
    let distro = roux_smolvm::distro_from_image(Some(&image));
    let script = resolve_install_script(&state, known, distro);

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {

        let smolfile_path = crate::paths::roux_config_dir()
            .join("smolmachines")
            .join(format!("{machine_name}.toml"));
        if let Some(parent) = smolfile_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("could not create {parent:?}: {e}")
            })?;
        }
        // Smolfile body. `ssh_agent = true` is preserved from the
        // original machine so private-repo cloning inside the guest
        // keeps working after recreation.
        let body = format!(
            "image     = \"{image}\"\nnet       = {net}\nssh_agent = {ssh}\n\n[dev]\ninit = [{script}]\n",
            image = image.replace('"', "\\\""),
            net = network,
            ssh = ssh_agent,
            script = toml_string_literal(&script),
        );
        std::fs::write(&smolfile_path, body)
            .map_err(|e| format!("could not write Smolfile {smolfile_path:?}: {e}"))?;

        let path_for_breadcrumb = smolfile_path.to_string_lossy().into_owned();

        // Step 2: stop. Idempotent on already-stopped machines.
        roux_smolvm::stop_machine(&install.path, &machine_name)
            .map_err(|e| format!("stop machine: {e} — Smolfile written at {path_for_breadcrumb}"))?;

        // Step 3: delete.
        roux_smolvm::delete_machine(&install.path, &machine_name).map_err(|e| {
            format!(
                "delete machine: {e} — Smolfile written at {path_for_breadcrumb}; recover with `smolvm machine create {machine_name} -s {path_for_breadcrumb}`"
            )
        })?;

        // Step 4: recreate. The Smolfile is the source of truth for
        // image/net/ssh_agent here; the CLI flags echo what we wrote
        // to it so smolvm can't reject for inconsistency.
        let create_opts = roux_smolvm::CreateOpts {
            name: &machine_name,
            smolfile_path: Some(&smolfile_path),
            image: None,
            network: false,
            ssh_agent,
        };
        roux_smolvm::create_machine(&install.path, &create_opts).map_err(|e| {
            format!(
                "recreate machine: {e} — Smolfile is at {path_for_breadcrumb}; recover with `smolvm machine create {machine_name} -s {path_for_breadcrumb}`"
            )
        })?;

        // Step 5: start.
        roux_smolvm::start_machine(&install.path, &machine_name)
            .map_err(|e| format!("start machine: {e}"))?;

        // Step 6: record link so future persist calls take the
        // append path.
        svc::record_smolfile_path(&machine_name, &smolfile_path)?;

        Ok(())
    })
    .await
    .map_err(|e| format!("install_smolvm_agent_recreate task panicked: {e}"))?
}

/// Quote a string as a TOML basic string literal. Escapes `"` and
/// backslashes; doesn't try to handle control characters because
/// install scripts shouldn't contain them.
fn toml_string_literal(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Returns the Smolfile path for each machine that has one linked
/// (via Roux's create form or the recreate flow). The frontend uses
/// this to show whether "Persist via Smolfile" will take the append
/// path or the recreate path before the user clicks.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_list_smol_machine_smolfiles()
-> Result<std::collections::BTreeMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(svc::read_smolmachines_registry_for_command)
        .await
        .map_err(|e| format!("list_smol_machine_smolfiles task panicked: {e}"))?
}

/// Open the smolvm bootstrap config file in the user's default
/// editor. Creates the file (with current built-in defaults as a
/// pre-populated starter) if it doesn't exist yet, so the user has
/// something concrete to edit.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_open_smolvm_bootstrap_config(
    app: tauri::AppHandle,
) -> Result<String, String> {
    use tauri_plugin_opener::OpenerExt;

    let path = svc::bootstrap_config_path();
    tauri::async_runtime::spawn_blocking({
        let path = path.clone();
        move || -> Result<(), String> {
            if !path.exists() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("could not create config dir {parent:?}: {e}")
                    })?;
                }
                std::fs::write(
                    &path,
                    roux_smolvm::BootstrapConfig::default_file_contents(),
                )
                .map_err(|e| format!("could not write {path:?}: {e}"))?;
            }
            Ok(())
        }
    })
    .await
    .map_err(|e| format!("bootstrap config init task panicked: {e}"))??;

    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("could not open {path:?} in default editor: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}
