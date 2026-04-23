use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

/// Minimum supported `wt` version. Below this, `detect_wt` returns `None`
/// so Roux falls back to the native git path.
///
/// The floor is the latest published worktrunk at the time of writing
/// (0.44.0) — the version our integration tests exercise. Bumping this
/// floor is a deliberate decision backed by an integration test that
/// exercises the newly required feature or field shape.
pub const MIN_WT_VERSION: &str = "0.44.0";

#[derive(Debug, Clone)]
pub struct WtBinary {
    pub path: PathBuf,
    pub version: Version,
}

/// Resolve the `wt` binary by trying, in order:
/// 1. An explicit settings override (when non-empty).
/// 2. `which::which("wt")` on the process `PATH`.
///
/// Returns `None` when no binary is found or `wt --version` is unparseable
/// or below `MIN_WT_VERSION`.
pub fn detect_wt(settings_override: Option<&str>) -> Option<WtBinary> {
    let path = match settings_override {
        Some(s) if !s.trim().is_empty() => {
            let p = PathBuf::from(s.trim());
            if p.exists() {
                p
            } else {
                return None;
            }
        }
        _ => which::which("wt").ok()?,
    };

    let version = probe_version(&path)?;
    let floor = Version::parse(MIN_WT_VERSION).ok()?;
    if version < floor {
        return None;
    }
    Some(WtBinary { path, version })
}

/// `true` iff `repo_path/.config/wt.toml` exists. Independent of whether
/// the `wt` binary is installed — a repo can carry the config without
/// the user having the CLI on their machine, and vice versa.
pub fn detect_wt_config(repo_path: &Path) -> bool {
    repo_path.join(".config").join("wt.toml").is_file()
}

fn probe_version(wt_path: &Path) -> Option<Version> {
    let out = Command::new(wt_path).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_version_line(&stdout)
}

/// Pull the first `MAJOR.MINOR.PATCH` substring out of `wt --version` output.
/// worktrunk emits something like `wt 0.44.0` or `worktrunk 0.44.0 (abc123)`;
/// we accept any line that contains a parseable semver token.
pub(crate) fn parse_version_line(s: &str) -> Option<Version> {
    for token in s.split_whitespace() {
        // Strip a leading `v` so `v0.44.0` parses too.
        let candidate = token.strip_prefix('v').unwrap_or(token);
        if let Ok(v) = Version::parse(candidate) {
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_line_plain() {
        assert_eq!(
            parse_version_line("wt 0.44.0\n").unwrap(),
            Version::parse("0.44.0").unwrap()
        );
    }

    #[test]
    fn parse_version_line_with_v_prefix() {
        assert_eq!(
            parse_version_line("worktrunk v0.44.0\n").unwrap(),
            Version::parse("0.44.0").unwrap()
        );
    }

    #[test]
    fn parse_version_line_with_commit_suffix() {
        assert_eq!(
            parse_version_line("worktrunk 0.44.0 (abc123)\n").unwrap(),
            Version::parse("0.44.0").unwrap()
        );
    }

    #[test]
    fn parse_version_line_unparseable() {
        assert!(parse_version_line("not a version at all").is_none());
    }
}
