//! Shared filesystem path policy for Roux processes.

use std::path::PathBuf;

pub const ROUX_BASE_PATH_ENV: &str = "ROUX_BASE_PATH";

/// Root directory for all Roux state.
///
/// Defaults to `~/.config/roux`, unless `ROUX_BASE_PATH` points at an
/// absolute directory path. The environment override replaces the root
/// exactly; callers append their own file or directory names.
pub fn roux_config_dir() -> PathBuf {
    roux_config_dir_from(std::env::var_os(ROUX_BASE_PATH_ENV).map(PathBuf::from), home_dir())
}

/// The built-in default root, `~/.config/roux`, ignoring any
/// `ROUX_BASE_PATH` override.
///
/// Use this only when you need the canonical location independent of the
/// override — e.g. deciding whether a legacy directory differs from the
/// default. For the effective root that callers should read/write, use
/// [`roux_config_dir`].
pub fn default_roux_config_dir() -> PathBuf {
    home_dir_or_temp_from(home_dir()).join(".config").join("roux")
}

/// The `ROUX_BASE_PATH` override, if set to an absolute directory path.
///
/// Returns `None` when the variable is unset, empty, or relative (the cases
/// where [`roux_config_dir`] falls back to [`default_roux_config_dir`]).
pub fn roux_base_path_override() -> Option<PathBuf> {
    roux_base_path_override_from(std::env::var_os(ROUX_BASE_PATH_ENV).map(PathBuf::from))
}

fn roux_config_dir_from(base_path: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    if let Some(base_path) = roux_base_path_override_from(base_path) {
        return base_path;
    }

    home_dir_or_temp_from(home).join(".config").join("roux")
}

fn roux_base_path_override_from(base_path: Option<PathBuf>) -> Option<PathBuf> {
    base_path.filter(|path| !path.as_os_str().to_string_lossy().is_empty() && path.is_absolute())
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

fn home_dir_or_temp_from(home: Option<PathBuf>) -> PathBuf {
    home.filter(|path| path.is_absolute()).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_fixture_path(name: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!(r"C:\{name}"))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/tmp/{name}"))
        }
    }

    #[test]
    fn absolute_roux_base_path_overrides_default_root() {
        let base = absolute_fixture_path("roux-test");
        let home = absolute_fixture_path("roux-home");

        assert_eq!(roux_config_dir_from(Some(base.clone()), Some(home)), base);
    }

    #[test]
    fn empty_roux_base_path_falls_back_to_default_root() {
        let home = absolute_fixture_path("roux-home");

        assert_eq!(
            roux_config_dir_from(Some(PathBuf::from("")), Some(home.clone())),
            home.join(".config").join("roux"),
        );
    }

    #[test]
    fn relative_roux_base_path_falls_back_to_default_root() {
        let home = absolute_fixture_path("roux-home");

        assert_eq!(
            roux_config_dir_from(Some(PathBuf::from("relative/roux")), Some(home.clone())),
            home.join(".config").join("roux"),
        );
    }
}
