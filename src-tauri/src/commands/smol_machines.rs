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

/// Owned snapshot of the inputs `resolve_install_script_with_inputs`
/// needs from `AppState`. Extracting this on the async runtime thread
/// (cheap — just a mutex read + clone) lets the actual library lookup
/// and the bootstrap-config read both run inside `spawn_blocking`,
/// alongside the smolvm CLI calls. Without this split we'd either
/// have to lock `AppState` from inside the blocking closure
/// (impossible — `State` has a non-`Send` lifetime) or do filesystem
/// I/O on the executor.
struct InstallScriptInputs {
    global_root: std::path::PathBuf,
    library_sources: Vec<roux_core::LibrarySource>,
    library_sources_dir: std::path::PathBuf,
}

fn install_script_inputs(state: &AppState) -> InstallScriptInputs {
    let library_sources_dir = crate::paths::roux_config_dir().join("library-sources");
    if let Ok(settings) = state.settings.lock().map(|guard| guard.clone()) {
        let global_root = settings
            .notes_vault_root
            .as_ref()
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(crate::paths::default_notes_vault_root);
        InstallScriptInputs {
            global_root,
            library_sources: settings.library_sources.clone(),
            library_sources_dir,
        }
    } else {
        InstallScriptInputs {
            global_root: crate::paths::default_notes_vault_root(),
            library_sources: Vec::new(),
            library_sources_dir,
        }
    }
}

/// Resolve the install script for `(agent, distro)` with the full
/// Phase 2.7 layered chain: library item → bootstrap config TOML →
/// hardcoded built-in. Pure given `inputs` — safe to call inside
/// `spawn_blocking`. The library layer walk and the bootstrap TOML
/// read are both filesystem I/O, so this should not run on the
/// async executor.
fn resolve_install_script_with_inputs(
    inputs: &InstallScriptInputs,
    agent: roux_smolvm::KnownAgent,
    distro: &str,
) -> String {
    let layers = library_svc::layers(
        inputs.global_root.clone(),
        &inputs.library_sources,
        &inputs.library_sources_dir,
        None, // no active repo — install is panel-driven, not session-driven
    );
    if let Some(script) =
        library_svc::find_smolvm_script_in_layers(&layers, agent.binary_name(), distro)
    {
        return script;
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
    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary().ok_or_else(|| "smolvm is not installed".to_string())?;
        let machine_name = request.name.clone();

        // Decide whether we need a Roux-managed Smolfile.
        //
        // The user-provided Smolfile (if any) is authoritative: we
        // don't modify it and don't override it. If the user supplied
        // *only* a proxy URL but no Smolfile, we need to generate one
        // because there's no other way to inject `[dev].init` into a
        // smolvm machine — the CLI doesn't take init lines as flags.
        let user_smolfile = request
            .smolfile_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from);
        let proxy_url = request
            .host_proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let (final_request, recorded_smolfile) = if let Some(path) = user_smolfile {
            // User-provided Smolfile is authoritative. Proxy URL is
            // silently ignored if both are set — the user is expected
            // to wire `[dev].init` themselves.
            (request, Some(path))
        } else if let Some(url) = proxy_url {
            // No user Smolfile + proxy URL → generate a managed one.
            let managed_path = svc::managed_smolfile_path(&machine_name);
            if let Some(parent) = managed_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("could not create {parent:?}: {e}"))?;
            }
            let body = svc::generate_managed_smolfile(
                request.image.as_deref().filter(|s| !s.is_empty()),
                request.network,
                request.ssh_agent,
                Some(url),
                None, // no install init line yet — that comes via the persist flow
            );
            std::fs::write(&managed_path, body)
                .map_err(|e| format!("could not write {managed_path:?}: {e}"))?;
            // Override the request to point at our managed Smolfile.
            // Image / network / ssh_agent stay on the request so they
            // also flow through as CLI flags — smolvm reconciles them
            // with the Smolfile (we wrote them to the file too, so
            // they agree).
            let mut updated = request;
            updated.smolfile_path = Some(managed_path.to_string_lossy().into_owned());
            (updated, Some(managed_path))
        } else {
            // No Smolfile, no proxy URL → existing flag-based create
            // path. Nothing to track.
            (request, None)
        };

        roux_core::smolvm::create_machine(&install.path, &final_request).map_err(|e| e.to_string())?;
        if let Some(path) = recorded_smolfile {
            svc::record_smolfile_path(&machine_name, &path)?;
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
    let known = roux_smolvm::KnownAgent::parse(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Snapshot AppState-derived inputs on the async runtime thread.
    // Smolvm CLI calls (binary resolve, list, install) all happen
    // inside the blocking closure below.
    let inputs = install_script_inputs(&state);

    tauri::async_runtime::spawn_blocking(move || {
        let install = svc::resolve_smolvm_binary()
            .ok_or_else(|| "smolvm is not installed".to_string())?;
        let machines =
            roux_smolvm::list_machines(&install.path).map_err(|e| e.to_string())?;
        let image = machines
            .iter()
            .find(|m| m.name == machine_name)
            .and_then(|m| m.image.clone());
        let distro = roux_smolvm::distro_from_image(image.as_deref());
        let script = resolve_install_script_with_inputs(&inputs, known, distro);
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
    let known = roux_smolvm::KnownAgent::parse(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Snapshot AppState-derived inputs on the async runtime thread.
    // Smolvm CLI calls + library/bootstrap script resolution happen
    // inside the blocking closure below.
    let inputs = install_script_inputs(&state);

    tauri::async_runtime::spawn_blocking(move || -> Result<PersistOutcome, String> {
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
        let script = resolve_install_script_with_inputs(&inputs, known, distro);

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
    let known = roux_smolvm::KnownAgent::parse(&agent).ok_or_else(|| {
        format!("unknown agent '{agent}'; supported: claude, codex")
    })?;
    // Snapshot AppState-derived inputs on the async runtime thread.
    // Smolvm CLI calls + library/bootstrap script resolution happen
    // inside the blocking closure below.
    let inputs = install_script_inputs(&state);

    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
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
        let script = resolve_install_script_with_inputs(&inputs, known, distro);

        let smolfile_path = crate::paths::roux_config_dir()
            .join("smolmachines")
            .join(format!("{machine_name}.toml"));
        if let Some(parent) = smolfile_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!("could not create {parent:?}: {e}")
            })?;
        }
        // Smolfile generation: today this command is only invoked
        // from the NeedsRecreate fallback (no linked Smolfile exists),
        // so the file usually doesn't exist yet. Write a fresh body
        // in that case. If a managed Smolfile is already present —
        // either because a prior recreate ran, or because a future
        // caller invokes recreate on a linked machine — preserve its
        // existing [dev].init / [dev].volumes / proxy env by appending
        // the install line in place rather than overwriting. This
        // protects the proxy URL written by `cmd_create_smol_machine`
        // when the machine was originally created with a proxy.
        // `ssh_agent = true` is preserved from the original machine so
        // private-repo cloning inside the guest keeps working after
        // recreation.
        if smolfile_path.exists() {
            roux_smolvm::smolfile_append_init(&smolfile_path, &script)
                .map_err(|e| format!("could not update Smolfile {smolfile_path:?}: {e}"))?;
        } else {
            let body = format!(
                "image     = \"{image}\"\nnet       = {net}\nssh_agent = {ssh}\n\n[dev]\ninit = [{script}]\n",
                image = image.replace('"', "\\\""),
                net = network,
                ssh = ssh_agent,
                script = toml_string_literal(&script),
            );
            std::fs::write(&smolfile_path, body)
                .map_err(|e| format!("could not write Smolfile {smolfile_path:?}: {e}"))?;
        }

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
        // image / net / ssh_agent / volumes / proxy-env-init here;
        // the CLI flags echo what we wrote to it so smolvm can't
        // reject for inconsistency.
        let create_opts = roux_smolvm::CreateOpts {
            name: &machine_name,
            smolfile_path: Some(&smolfile_path),
            image: None,
            network,
            ssh_agent,
            volumes: &[],
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

/// Start the user-configured managed HTTP proxy. No-op if already
/// running. Returns the live status so the UI can update without a
/// follow-up `cmd_managed_proxy_status` round-trip.
///
/// `RouxSettings.managed_proxy` must be set; otherwise returns a
/// typed error pointing the user at Settings → Smol Machines.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_start_managed_proxy(
    state: tauri::State<'_, AppState>,
) -> Result<crate::services::managed_proxy::ManagedProxyStatus, String> {
    let config = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .managed_proxy
        .clone()
        .ok_or_else(|| {
            "managed proxy is not configured (Settings → Smol Machines)".to_string()
        })?;
    let proxy_state = state.managed_proxy.clone();
    tauri::async_runtime::spawn_blocking(move || proxy_state.start(&config))
        .await
        .map_err(|e| format!("start_managed_proxy task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_stop_managed_proxy(
    state: tauri::State<'_, AppState>,
) -> Result<crate::services::managed_proxy::ManagedProxyStatus, String> {
    let proxy_state = state.managed_proxy.clone();
    tauri::async_runtime::spawn_blocking(move || Ok(proxy_state.stop()))
        .await
        .map_err(|e| format!("stop_managed_proxy task panicked: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cmd_managed_proxy_status(
    state: tauri::State<'_, AppState>,
) -> crate::services::managed_proxy::ManagedProxyStatus {
    state.managed_proxy.status()
}

/// Result of [`cmd_check_worktree_mount`]. Tells the panel whether
/// the session's worktree is reachable from inside the VM via an
/// existing `[dev].volumes` mount in the linked Smolfile.
///
/// `Mounted` and `NoLinkedSmolfile` mean "no action surfaced" — the
/// frontend won't show a banner. `NotMounted` is the only case that
/// triggers the auto-mount UX.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum WorktreeMountCheck {
    /// Worktree path is covered by an existing volume spec. The
    /// matching host side is returned for diagnostic display.
    Mounted { host: String },
    /// The Smolfile exists, but no volume spec covers the worktree.
    /// `proposedSpec` is the same-path mount Roux would append if the
    /// user accepts the auto-mount.
    NotMounted {
        smolfile_path: String,
        proposed_spec: String,
    },
    /// The machine has no linked Smolfile — Roux can't auto-mount
    /// without one. The frontend should keep quiet in this case;
    /// users with a manually-managed machine know what they're doing.
    NoLinkedSmolfile,
}

/// Check whether `worktree_path` is reachable inside `machine_name`'s
/// guest. "Reachable" means: the linked Smolfile contains a
/// `[dev].volumes` entry whose host side is a path-prefix of
/// `worktree_path`. Same-path mapping (host == guest) is the common
/// case, which makes `--workdir <host_path>` resolve identically
/// inside the guest.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_check_worktree_mount(
    machine_name: String,
    worktree_path: String,
) -> Result<WorktreeMountCheck, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let Some(smolfile_path) = svc::smolfile_path_for_machine(&machine_name) else {
            return Ok(WorktreeMountCheck::NoLinkedSmolfile);
        };
        if !smolfile_path.exists() {
            return Ok(WorktreeMountCheck::NoLinkedSmolfile);
        }
        let volumes = roux_smolvm::smolfile_get_volumes(&smolfile_path)
            .map_err(|e| format!("could not read Smolfile: {e}"))?;

        // A worktree is "covered" when an existing host-side path is
        // either equal to or a parent of it. Strict path-component
        // match — string-prefix would falsely accept `/Users/me/code`
        // as covering `/Users/me/code-other`.
        let wt = std::path::Path::new(&worktree_path);
        let covered_by = volumes.iter().find_map(|spec| {
            let host = roux_smolvm::volume_spec_host(spec)?;
            let host_path = std::path::Path::new(host);
            if wt == host_path || wt.starts_with(host_path) {
                Some(host.to_string())
            } else {
                None
            }
        });

        if let Some(host) = covered_by {
            return Ok(WorktreeMountCheck::Mounted { host });
        }

        // Same-path mount: makes `--workdir <wt>` work directly.
        let proposed_spec = format!("{worktree_path}:{worktree_path}");
        Ok(WorktreeMountCheck::NotMounted {
            smolfile_path: smolfile_path.to_string_lossy().into_owned(),
            proposed_spec,
        })
    })
    .await
    .map_err(|e| format!("check_worktree_mount task panicked: {e}"))?
}

/// Append a `host:guest[:ro]` spec to the linked Smolfile's
/// `[dev].volumes`. Idempotent — `AlreadyPresent` when the exact spec
/// is already there. Errors when the machine has no linked Smolfile
/// (frontend should skip the mount UX in that case).
///
/// The spec takes effect on the next `smolvm machine create` for the
/// machine — smolvm volumes are baked at create time. The caller is
/// expected to inform the user that a recreate (or the existing
/// recreate flow) is required to apply it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cmd_append_worktree_mount(
    machine_name: String,
    spec: String,
) -> Result<MountAppendOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let Some(smolfile_path) = svc::smolfile_path_for_machine(&machine_name) else {
            return Err(format!(
                "machine '{machine_name}' has no linked Smolfile; \
                 cannot append a volume spec"
            ));
        };
        let outcome = roux_smolvm::smolfile_append_volume(&smolfile_path, &spec)
            .map_err(|e| format!("smolfile append: {e}"))?;
        Ok(MountAppendOutcome {
            kind: match outcome {
                roux_smolvm::AppendOutcome::Appended => "appended",
                roux_smolvm::AppendOutcome::AlreadyPresent => "alreadyPresent",
            }
            .to_string(),
            smolfile_path: smolfile_path.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| format!("append_worktree_mount task panicked: {e}"))?
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MountAppendOutcome {
    /// `"appended"` when a new line was added; `"alreadyPresent"` when
    /// the exact spec was already in `[dev].volumes`.
    pub kind: String,
    /// Absolute path to the Smolfile that was (or wasn't) modified.
    pub smolfile_path: String,
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
