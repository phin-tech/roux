//! Helpers for resolving the user's `smolvm` install. Pattern-mirrors
//! `setup::resolve_wt_binary` so the activity rail and the smol-machines
//! commands use the same precedence rules.

use std::path::PathBuf;

use roux_smolvm::SmolvmBinary;

/// Path to the user-editable bootstrap config:
/// `~/.config/roux/smolvm-bootstraps.toml`. The file is optional —
/// `roux_smolvm::BootstrapConfig::load_or_default` falls back to
/// built-in defaults when it's missing, so we never need to ensure
/// the file exists at app start. The "Edit bootstrap config" panel
/// action creates it on demand with a populated template.
pub(crate) fn bootstrap_config_path() -> PathBuf {
    crate::paths::roux_config_dir().join("smolvm-bootstraps.toml")
}

/// Path to the per-machine Smolfile registry:
/// `~/.config/roux/smolmachines.json`. JSON map of
/// `{ machine_name: smolfile_absolute_path }`. Only written when at
/// least one machine has a linked Smolfile (lazily by
/// [`record_smolfile_path`]).
fn smolmachines_registry_path() -> PathBuf {
    crate::paths::roux_config_dir().join("smolmachines.json")
}

/// Read the per-machine Smolfile path for `name`. Returns `None` for
/// missing-file, missing-key, JSON parse errors, or any other read
/// failure — callers treat all of those as "no Smolfile linked".
pub(crate) fn smolfile_path_for_machine(name: &str) -> Option<PathBuf> {
    let map = read_smolmachines_registry().ok()?;
    map.get(name).map(PathBuf::from)
}

/// Record `(name, path)` in the Smolfile registry. Creates the file
/// (and the parent dir) on first use. Errors bubble up so the caller
/// can surface a useful message.
pub(crate) fn record_smolfile_path(
    name: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let registry_path = smolmachines_registry_path();
    let mut map = read_smolmachines_registry().unwrap_or_default();
    map.insert(name.to_string(), path.to_string_lossy().into_owned());

    if let Some(parent) = registry_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create config dir {parent:?}: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("could not serialize smolmachines registry: {e}"))?;
    std::fs::write(&registry_path, json)
        .map_err(|e| format!("could not write {registry_path:?}: {e}"))
}

fn read_smolmachines_registry()
-> Result<std::collections::BTreeMap<String, String>, std::io::Error> {
    let path = smolmachines_registry_path();
    let body = std::fs::read_to_string(&path)?;
    serde_json::from_str(&body).map_err(std::io::Error::other)
}

/// Tauri-command-friendly variant: returns an empty map (not an
/// error) when the registry doesn't exist yet, so the frontend can
/// always render — no machines have a Smolfile linked yet → all
/// "Persist via Smolfile" actions take the recreate path.
pub(crate) fn read_smolmachines_registry_for_command()
-> Result<std::collections::BTreeMap<String, String>, String> {
    match read_smolmachines_registry() {
        Ok(map) => Ok(map),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(format!("could not read smolmachines registry: {e}")),
    }
}

/// Resolve the `smolvm` binary as a typed [`SmolvmBinary`].
///
/// Precedence:
///   1. `settings.smolvm_binary_path` override (trimmed, non-empty).
///   2. Login-shell PATH (so a user who installs smolvm via Homebrew sees
///      it from a GUI app launch on macOS, where the inherited PATH is
///      minimal).
///   3. Process PATH.
///
/// Returns `None` when nothing is found or `smolvm --version` fails — the
/// only consumer (the activity rail) collapses both into "not installed".
pub(crate) fn resolve_smolvm_binary() -> Option<SmolvmBinary> {
    let override_path =
        crate::services::setup::nonempty_path(crate::settings::load_settings().smolvm_binary_path.as_deref());
    if let Some(path) = override_path.as_deref() {
        return roux_smolvm::detect(Some(path));
    }
    if let Some(extra) = crate::services::setup::login_shell_path_os() {
        if let Some(path) = crate::services::setup::find_in_path_env(extra.as_os_str(), "smolvm") {
            return roux_smolvm::detect(Some(&path));
        }
    }
    roux_smolvm::detect(None)
}
