//! Helpers for resolving the user's `smolvm` install. Pattern-mirrors
//! `setup::resolve_wt_binary` so the activity rail and the smol-machines
//! commands use the same precedence rules.

use roux_smolvm::SmolvmBinary;

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
