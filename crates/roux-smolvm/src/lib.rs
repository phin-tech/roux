//! Shell-out wrapper around the smol machines (`smolvm`) CLI.
//!
//! Mirrors the integration shape of `roux_worktrunk`: Roux uses whatever
//! `smolvm` binary the user has on PATH rather than linking smolvm
//! directly. Each function spawns the CLI, parses its output, and maps
//! failures into typed errors. The whole crate is a no-op when smolvm
//! isn't installed — `detect()` returns `None` and the rest of the
//! integration disappears from the UI.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SmolvmError {
    #[error("Failed to run smolvm: {source}")]
    RunSmolvm {
        #[source]
        source: std::io::Error,
    },
    #[error("smolvm {command} failed: {stderr}")]
    CommandFailed { command: String, stderr: String },
    #[error("Failed to parse smolvm output: {message}")]
    ParseFailed { message: String },
}

/// Resolved smolvm install. Returned by [`detect`] when a usable binary
/// is on PATH (or at the configured override).
#[derive(Debug, Clone)]
pub struct SmolvmBinary {
    pub path: PathBuf,
    /// Raw version string (e.g. "0.1.2"). Kept as a string rather than
    /// `semver::Version` because we don't enforce a minimum version yet
    /// and don't want to gate detection on a stricter schema than
    /// upstream guarantees.
    pub version: String,
}

/// One entry from `smolvm machine ls --json`. Field names mirror the
/// upstream JSON schema (see smolvm's `vm_common::list_vms`); we keep
/// them stable so the wire shape between Rust and TS is decoupled from
/// upstream renames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmolMachine {
    pub name: String,
    pub state: String,
    pub image: Option<String>,
    pub cpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub created_at: Option<String>,
    #[serde(default)]
    pub ephemeral: bool,
    #[serde(default)]
    pub network: bool,
}

/// Resolve the `smolvm` binary by trying, in order:
/// 1. An explicit settings override (when non-empty and resolves to a file).
/// 2. `which::which("smolvm")` on the process `PATH`.
///
/// Returns `None` when no binary is found or `smolvm --version` fails to
/// produce a parseable line. Distinguishing "missing" from "broken" isn't
/// worth a typed error here — the only consumer (the activity rail) just
/// hides the icon either way.
pub fn detect(settings_override: Option<&str>) -> Option<SmolvmBinary> {
    let path = match settings_override {
        Some(s) if !s.trim().is_empty() => {
            let p = PathBuf::from(s.trim());
            if p.is_file() {
                p
            } else {
                return None;
            }
        }
        _ => which::which("smolvm").ok()?,
    };

    let version = probe_version(&path)?;
    Some(SmolvmBinary { path, version })
}

fn probe_version(binary: &Path) -> Option<String> {
    let out = Command::new(binary).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_version_line(&stdout)
}

/// Pull a version-looking token out of `smolvm --version` output. Accepts
/// formats like `smolvm 0.1.2`, `smolvm v0.1.2`, or anything where the
/// second whitespace-separated token starts with a digit.
fn parse_version_line(s: &str) -> Option<String> {
    for token in s.split_whitespace() {
        let candidate = token.strip_prefix('v').unwrap_or(token);
        if candidate.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// List smol machines via `smolvm machine ls --json`.
///
/// Tolerant to two output shapes the upstream CLI has shipped: a single
/// JSON array, or NDJSON (one machine per line). If neither parses we
/// surface a `ParseFailed` so the UI can show a diagnostic instead of an
/// empty list.
pub fn list_machines(binary: &Path) -> Result<Vec<SmolMachine>, SmolvmError> {
    let output = Command::new(binary)
        .args(["machine", "ls", "--json"])
        .output()
        .map_err(|source| SmolvmError::RunSmolvm { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SmolvmError::CommandFailed {
            command: "machine ls".into(),
            stderr: stderr.into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_list(&stdout)
}

fn parse_list(stdout: &str) -> Result<Vec<SmolMachine>, SmolvmError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(machines) = serde_json::from_str::<Vec<SmolMachine>>(trimmed) {
        return Ok(machines);
    }
    let mut out = Vec::new();
    for line in trimmed.lines().filter(|l| !l.trim().is_empty()) {
        let m: SmolMachine = serde_json::from_str(line).map_err(|e| SmolvmError::ParseFailed {
            message: format!("could not parse `smolvm machine ls --json` line ({e}): {line}"),
        })?;
        out.push(m);
    }
    Ok(out)
}

pub fn start_machine(binary: &Path, name: &str) -> Result<(), SmolvmError> {
    // `machine start` uses `--name <NAME>` (positional name was rejected
    // upstream as of 0.1.x). Same for stop. Delete is the odd one out —
    // it takes a positional name plus `--force` to skip its interactive
    // prompt; without `--force`, the subprocess blocks forever waiting
    // on stdin and the Tauri spawn_blocking task wedges the panel.
    run_simple(binary, &["machine", "start", "--name", name], "machine start")
}

pub fn stop_machine(binary: &Path, name: &str) -> Result<(), SmolvmError> {
    run_simple(binary, &["machine", "stop", "--name", name], "machine stop")
}

pub fn delete_machine(binary: &Path, name: &str) -> Result<(), SmolvmError> {
    // `--force` is required: smolvm otherwise prompts for confirmation
    // on stdin, which we don't have. The user-facing `window.confirm`
    // in SmolMachinesPanel.svelte plays the role of that prompt.
    run_simple(binary, &["machine", "delete", "--force", name], "machine delete")
}

/// Options for creating a smol machine. Mirrors the subset of
/// `smolvm machine create` flags we expose in the panel UI.
///
/// When `smolfile_path` is set, smolvm reads image/network/cpus/etc.
/// from the Smolfile — `image` and `network` are still passed through if
/// the caller fills them, and the upstream CLI decides precedence. The
/// panel UI hides those fields when a Smolfile is selected (per the
/// product decision in the integration plan), so in practice they're
/// only sent for the no-Smolfile case.
pub struct CreateOpts<'a> {
    pub name: &'a str,
    pub smolfile_path: Option<&'a Path>,
    pub image: Option<&'a str>,
    pub network: bool,
}

pub fn create_machine(binary: &Path, opts: &CreateOpts) -> Result<(), SmolvmError> {
    let args = build_create_args(opts);
    let output = Command::new(binary)
        .args(args.iter().map(std::ffi::OsString::as_os_str))
        .output()
        .map_err(|source| SmolvmError::RunSmolvm { source })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SmolvmError::CommandFailed {
            command: "machine create".into(),
            stderr: stderr.into_owned(),
        });
    }
    Ok(())
}

/// Pure arg-vector builder. Split out so we can unit-test the flag
/// composition without spawning a subprocess.
fn build_create_args(opts: &CreateOpts) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut args: Vec<OsString> =
        vec!["machine".into(), "create".into(), opts.name.into()];
    if let Some(path) = opts.smolfile_path {
        args.push("--smolfile".into());
        args.push(path.as_os_str().into());
    }
    if let Some(image) = opts.image {
        args.push("--image".into());
        args.push(image.into());
    }
    if opts.network {
        args.push("--net".into());
    }
    args
}

fn run_simple(binary: &Path, args: &[&str], label: &str) -> Result<(), SmolvmError> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|source| SmolvmError::RunSmolvm { source })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SmolvmError::CommandFailed {
            command: label.into(),
            stderr: stderr.into_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_plain() {
        assert_eq!(parse_version_line("smolvm 0.1.2\n").as_deref(), Some("0.1.2"));
    }

    #[test]
    fn parse_version_with_v_prefix() {
        assert_eq!(parse_version_line("smolvm v0.1.2\n").as_deref(), Some("0.1.2"));
    }

    #[test]
    fn parse_version_with_suffix() {
        assert_eq!(parse_version_line("smolvm 0.1.2 (abc123)\n").as_deref(), Some("0.1.2"));
    }

    #[test]
    fn parse_version_unparseable() {
        assert_eq!(parse_version_line("not a version"), None);
    }

    #[test]
    fn parse_list_empty_stdout_is_empty_vec() {
        assert!(parse_list("").unwrap().is_empty());
        assert!(parse_list("   \n  ").unwrap().is_empty());
    }

    #[test]
    fn parse_list_array_form() {
        let json = r#"[
            {"name":"alpha","state":"running","image":"ubuntu","cpus":2,"memory_mib":1024,"created_at":"2026-05-01","ephemeral":false,"network":true},
            {"name":"beta","state":"stopped","image":null,"cpus":null,"memory_mib":null,"created_at":null,"ephemeral":true,"network":false}
        ]"#;
        let machines = parse_list(json).unwrap();
        assert_eq!(machines.len(), 2);
        assert_eq!(machines[0].name, "alpha");
        assert_eq!(machines[0].state, "running");
        assert_eq!(machines[0].image.as_deref(), Some("ubuntu"));
        assert_eq!(machines[0].cpus, Some(2));
        assert!(machines[0].network);
        assert_eq!(machines[1].name, "beta");
        assert!(machines[1].ephemeral);
        assert!(!machines[1].network);
    }

    #[test]
    fn parse_list_ndjson_form() {
        let ndjson = "{\"name\":\"alpha\",\"state\":\"running\"}\n{\"name\":\"beta\",\"state\":\"stopped\"}\n";
        let machines = parse_list(ndjson).unwrap();
        assert_eq!(machines.len(), 2);
        assert_eq!(machines[0].name, "alpha");
        assert_eq!(machines[1].name, "beta");
    }

    #[test]
    fn parse_list_defaults_for_missing_optional_fields() {
        // Older smolvm builds may not emit ephemeral/network. Treat as false.
        let json = r#"[{"name":"alpha","state":"running"}]"#;
        let machines = parse_list(json).unwrap();
        assert_eq!(machines[0].name, "alpha");
        assert!(!machines[0].ephemeral);
        assert!(!machines[0].network);
    }

    #[test]
    fn parse_list_invalid_json_returns_typed_error() {
        let bad = "{not valid json";
        let err = parse_list(bad).unwrap_err();
        assert!(matches!(err, SmolvmError::ParseFailed { .. }));
    }

    fn args_to_strings(args: &[std::ffi::OsString]) -> Vec<String> {
        args.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn build_create_args_name_only() {
        let args = build_create_args(&CreateOpts {
            name: "my-vm",
            smolfile_path: None,
            image: None,
            network: false,
        });
        assert_eq!(args_to_strings(&args), vec!["machine", "create", "my-vm"]);
    }

    #[test]
    fn build_create_args_with_smolfile() {
        let path = Path::new("/tmp/Smolfile");
        let args = build_create_args(&CreateOpts {
            name: "vm",
            smolfile_path: Some(path),
            image: None,
            network: false,
        });
        assert_eq!(
            args_to_strings(&args),
            vec!["machine", "create", "vm", "--smolfile", "/tmp/Smolfile"]
        );
    }

    #[test]
    fn build_create_args_with_image_and_network() {
        let args = build_create_args(&CreateOpts {
            name: "vm",
            smolfile_path: None,
            image: Some("alpine"),
            network: true,
        });
        assert_eq!(
            args_to_strings(&args),
            vec!["machine", "create", "vm", "--image", "alpine", "--net"]
        );
    }

    #[test]
    fn build_create_args_smolfile_with_image_still_passes_image() {
        // The panel hides image/network when a Smolfile is selected, but at
        // the crate level we don't second-guess the caller — if both are
        // supplied we forward both and let smolvm decide precedence. This
        // keeps the crate tested independently of UI policy.
        let path = Path::new("/Users/sam/Smolfile");
        let args = build_create_args(&CreateOpts {
            name: "vm",
            smolfile_path: Some(path),
            image: Some("alpine"),
            network: true,
        });
        assert_eq!(
            args_to_strings(&args),
            vec![
                "machine",
                "create",
                "vm",
                "--smolfile",
                "/Users/sam/Smolfile",
                "--image",
                "alpine",
                "--net"
            ]
        );
    }
}
