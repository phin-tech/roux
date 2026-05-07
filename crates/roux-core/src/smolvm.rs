//! Glue layer between the `roux-smolvm` shellout crate and the rest of
//! roux-core. Mirrors the role `worktree.rs` plays for `roux-worktrunk`:
//! the heavy lifting (subprocess, parsing) lives in the sibling crate;
//! this module exposes a stable `Serialize + specta::Type` shape that
//! crosses the IPC boundary into TypeScript.

use serde::Serialize;

pub use roux_smolvm::SmolvmError;

/// Wire-shape of one smol machine. Mirrors `roux_smolvm::SmolMachine` but
/// derives `specta::Type` so it appears in the generated TS bindings.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SmolMachine {
    pub name: String,
    pub state: String,
    pub image: Option<String>,
    pub cpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub created_at: Option<String>,
    pub ephemeral: bool,
    pub network: bool,
    /// `true` when the host's SSH agent is forwarded into the guest.
    pub ssh_agent: bool,
}

impl From<roux_smolvm::SmolMachine> for SmolMachine {
    fn from(m: roux_smolvm::SmolMachine) -> Self {
        Self {
            name: m.name,
            state: m.state,
            image: m.image,
            cpus: m.cpus,
            memory_mib: m.memory_mib,
            created_at: m.created_at,
            ephemeral: m.ephemeral,
            network: m.network,
            ssh_agent: m.ssh_agent,
        }
    }
}

/// Detection result returned to the activity rail. `binary_path` is the
/// only field consumers branch on; `version` is informational for the
/// settings panel and diagnostics view.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SmolvmDetection {
    pub binary_path: Option<String>,
    pub version: Option<String>,
}

/// Probe for a usable smolvm install. `settings_override` lets the user
/// pin a specific binary path via `RouxSettings::smolvm_binary_path`.
pub fn detect(settings_override: Option<&str>) -> SmolvmDetection {
    match roux_smolvm::detect(settings_override) {
        Some(install) => SmolvmDetection {
            binary_path: Some(install.path.to_string_lossy().into_owned()),
            version: Some(install.version),
        },
        None => SmolvmDetection { binary_path: None, version: None },
    }
}

/// List smol machines. Returns `Ok(vec![])` when smolvm reports no
/// machines; returns a typed error if the binary fails or output is
/// unparseable.
pub fn list_machines(binary: &std::path::Path) -> Result<Vec<SmolMachine>, SmolvmError> {
    Ok(roux_smolvm::list_machines(binary)?.into_iter().map(SmolMachine::from).collect())
}

pub fn start_machine(binary: &std::path::Path, name: &str) -> Result<(), SmolvmError> {
    roux_smolvm::start_machine(binary, name)
}

pub fn stop_machine(binary: &std::path::Path, name: &str) -> Result<(), SmolvmError> {
    roux_smolvm::stop_machine(binary, name)
}

pub fn delete_machine(binary: &std::path::Path, name: &str) -> Result<(), SmolvmError> {
    roux_smolvm::delete_machine(binary, name)
}

/// Wire-shape of a create-machine request. Mirrors `roux_smolvm::CreateOpts`
/// but holds owned values so it can travel across the IPC boundary as a
/// single specta-typed Tauri argument.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SmolMachineCreateRequest {
    pub name: String,
    pub smolfile_path: Option<String>,
    pub image: Option<String>,
    pub network: bool,
    /// Forward the host's SSH agent into the guest so `git clone
    /// git@…` works inside the VM. Private keys never leave the
    /// host — the hypervisor enforces it. Default `false`; the create
    /// form has a checkbox.
    #[serde(default)]
    pub ssh_agent: bool,
    /// HTTP(S) proxy URL the guest should route outbound requests
    /// through. When set and no Smolfile is provided, Roux generates
    /// a managed Smolfile that exports `HTTP_PROXY` / `HTTPS_PROXY`
    /// in the guest. When the user provides their own Smolfile, this
    /// field is silently ignored — their Smolfile is authoritative
    /// and they're expected to wire the proxy env themselves.
    #[serde(default)]
    pub host_proxy_url: Option<String>,
    /// Host paths to mount into the guest as `host:guest[:ro]` specs.
    /// Each entry is forwarded verbatim to `smolvm machine create -v`.
    /// Defaults to empty. The panel UI uses this for Phase 2.9
    /// worktree mounts; same-path mounting (`/Users/me/code/foo:
    /// /Users/me/code/foo`) lets `--workdir <host_worktree>` resolve
    /// inside the VM.
    #[serde(default)]
    pub volumes: Vec<String>,
}

pub fn create_machine(
    binary: &std::path::Path,
    req: &SmolMachineCreateRequest,
) -> Result<(), SmolvmError> {
    let smolfile_path = req.smolfile_path.as_deref().map(std::path::Path::new);
    // Empty strings from the form should be treated as "unset" — smolvm
    // would otherwise reject `--image ""` with a confusing error.
    let image = req.image.as_deref().filter(|s| !s.is_empty());
    let host_proxy_url = req
        .host_proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let opts = roux_smolvm::CreateOpts {
        name: &req.name,
        smolfile_path,
        image,
        network: req.network,
        ssh_agent: req.ssh_agent,
        host_proxy_url,
        volumes: &req.volumes,
    };
    roux_smolvm::create_machine(binary, &opts)
}
