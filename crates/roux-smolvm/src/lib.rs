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
    /// `true` when the host's SSH agent is forwarded into the guest.
    /// Field defaults to `false` for old smolvm versions whose
    /// `machine ls --json` doesn't emit `ssh_agent`.
    #[serde(default)]
    pub ssh_agent: bool,
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

/// Agents Roux knows how to (a) preflight before profile-replay and
/// (b) install on demand inside a smol guest. The
/// [`KnownAgent::from_str`] helper lets callers map a
/// `startup_command`'s leading binary token (e.g. "claude", "codex")
/// to a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownAgent {
    Claude,
    Codex,
}

impl KnownAgent {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    /// The binary name the guest sees on PATH.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Built-in default install script for this agent on a given
    /// distro key. Returned scripts are POSIX-shell one-liners that
    /// install prerequisites *and* the agent in a single line — no
    /// runtime distro detection inside the guest, because we already
    /// know the distro from the machine's image string.
    ///
    /// `distro` should be one of `"alpine"`, `"ubuntu"`, or
    /// `"default"` (resolved via [`distro_from_image`]). Anything
    /// else falls through to the `default` branch.
    ///
    /// User-defined overrides come from [`BootstrapConfig`] which is
    /// consulted before this hardcoded fallback.
    pub fn default_script_for_distro(self, distro: &str) -> &'static str {
        match (self, distro) {
            (Self::Claude, "alpine") => {
                "apk add --no-cache curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
            }
            (Self::Claude, "ubuntu") => {
                "apt-get update && apt-get install -y curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
            }
            (Self::Claude, _) => "curl -fsSL https://claude.ai/install.sh | bash",
            (Self::Codex, "alpine") => {
                "apk add --no-cache nodejs npm && npm install -g @openai/codex"
            }
            (Self::Codex, "ubuntu") => {
                "apt-get update && apt-get install -y nodejs npm && npm install -g @openai/codex"
            }
            (Self::Codex, _) => "npm install -g @openai/codex",
        }
    }
}

/// Map a smolvm machine's image string to a distro key for script
/// resolution. Returns `"alpine"`, `"ubuntu"`, or `"default"`. Lowest-
/// tech possible — leading-prefix match, no runtime guest probe.
///
/// Examples:
///   `Some("alpine")`              → `"alpine"`
///   `Some("alpine:3.19")`         → `"alpine"`
///   `Some("docker.io/alpine")`    → `"alpine"`
///   `Some("ubuntu:22.04")`        → `"ubuntu"`
///   `Some("ghcr.io/foo/ubuntu")`  → `"ubuntu"`
///   `Some("debian:12")`           → `"default"`  (future: extend)
///   `None`                        → `"default"`
///
/// "default" is returned for any image we don't recognize — the
/// caller's script (built-in or user-overridden) runs as-is and any
/// prereq error surfaces verbatim.
pub fn distro_from_image(image: Option<&str>) -> &'static str {
    let img = image.unwrap_or("").to_lowercase();
    if img == "alpine" || img.starts_with("alpine:") || img.contains("/alpine") {
        return "alpine";
    }
    if img == "ubuntu" || img.starts_with("ubuntu:") || img.contains("/ubuntu") {
        return "ubuntu";
    }
    "default"
}

/// `which`-style probe inside the guest. Returns `Ok(Some(path))` when
/// the binary is on the guest's PATH, `Ok(None)` when it isn't, and
/// `Err(_)` only when the smolvm CLI itself fails (e.g. machine isn't
/// running). Uses POSIX `command -v` so it works on busybox-only
/// images that lack `which`.
pub fn check_guest_binary(
    binary: &Path,
    machine_name: &str,
    guest_binary: &str,
) -> Result<Option<String>, SmolvmError> {
    let script = format!("command -v {}", shell_single_quote(guest_binary));
    let output = Command::new(binary)
        .args(["machine", "exec", "--name", machine_name, "--", "sh", "-c", &script])
        .output()
        .map_err(|source| SmolvmError::RunSmolvm { source })?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Ok(None)
        } else {
            Ok(Some(stdout))
        }
    } else {
        // `command -v` exits 1 with empty stdout when the binary isn't
        // found — that's the "missing" case, not an error. Non-empty
        // stderr means smolvm itself failed (machine down, exec
        // refused, etc.) and the caller should surface it.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.trim().is_empty() {
            Ok(None)
        } else {
            Err(SmolvmError::CommandFailed {
                command: "check guest binary".into(),
                stderr: stderr.into_owned(),
            })
        }
    }
}

/// User-overridable config for the bootstrap + install pipeline.
///
/// Loaded from `~/.config/roux/smolvm-bootstraps.toml` (path resolved
/// by the caller — `roux-smolvm` doesn't depend on `dirs` and stays
/// pure). Anything not specified in the file falls back to the
/// hardcoded defaults baked into [`KnownAgent`] / [`Distro`]. The
/// overlay is per-key, not all-or-nothing — a user file with only
/// `[agents.claude]` keeps Codex and the distro mappings on built-in
/// values.
///
/// Schema is intentionally narrow:
///
/// ```toml
/// [agents.claude]
/// prereqs = ["curl", "bash", "ca-certificates"]
/// install = "curl -fsSL https://claude.ai/install.sh | bash"
///
/// [agents.codex]
/// prereqs = ["nodejs", "npm"]
/// install = "npm install -g @openai/codex"
///
/// [distros.alpine]
/// match_id = ["alpine"]
/// package_install = "apk add --no-cache {packages}"
/// ```
///
/// User-overridable install scripts, keyed by agent and distro.
/// Schema is intentionally flat — one bash one-liner per (agent,
/// distro) cell. No template substitution, no prereq lists, no
/// runtime distro abstraction. The whole job of this struct is
/// "let the user edit a script without rebuilding."
///
/// ```toml
/// [agents.claude]
/// alpine  = "apk add --no-cache curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
/// ubuntu  = "apt-get update && apt-get install -y curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
/// default = "curl -fsSL https://claude.ai/install.sh | bash"
/// ```
///
/// Resolution order at install time is library (Phase 2.7, future) →
/// this config → [`KnownAgent::default_script_for_distro`] hardcoded
/// fallback. Per-key — overriding only `claude.alpine` keeps the rest
/// on built-in defaults.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub agents: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

impl BootstrapConfig {
    /// Read and parse the config from `path`, falling back to an
    /// empty config (built-in defaults only) on any error — missing
    /// file, malformed TOML, permission denied, etc. The fallback is
    /// silent because the file is optional; users opt in by creating
    /// it.
    pub fn load_or_default(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Resolve the install script for `(agent, distro)`. Returns the
    /// user override when present, else the hardcoded default from
    /// [`KnownAgent::default_script_for_distro`]. The `distro` arg is
    /// expected to come from [`distro_from_image`] and be one of
    /// `"alpine"` / `"ubuntu"` / `"default"` (anything else is
    /// treated as `"default"` by the lookup chain).
    ///
    /// **Phase 2.7 hook:** when library support lands, this is the
    /// single function library lookup wraps. install_agent and
    /// cmd_install_smolvm_agent_persist call only this — they don't
    /// know about config layering at all.
    pub fn agent_script(&self, agent: KnownAgent, distro: &str) -> String {
        if let Some(per_distro) = self.agents.get(agent.binary_name()) {
            if let Some(script) = per_distro.get(distro) {
                return script.clone();
            }
            if let Some(fallback) = per_distro.get("default") {
                return fallback.clone();
            }
        }
        agent.default_script_for_distro(distro).to_string()
    }

    /// Render a starter file populated with current built-in defaults
    /// + comments explaining each section. The Tauri "Edit bootstrap
    /// config" command writes this when the file doesn't exist yet,
    /// so users have something concrete to edit instead of a blank
    /// page.
    pub fn default_file_contents() -> &'static str {
        DEFAULT_BOOTSTRAP_CONFIG_TOML
    }
}

const DEFAULT_BOOTSTRAP_CONFIG_TOML: &str = r#"# Roux smol-machine install scripts.
#
# Edit and save — Roux re-reads this file on every "Install Claude /
# Codex" click, so changes take effect without restarting the app.
# Anything you remove or omit falls back to Roux's hardcoded
# defaults.
#
# Schema: one bash one-liner per (agent, distro). Roux picks the
# right one based on the machine's image. "default" runs when the
# image isn't a recognized distro. No template substitution — the
# string is run verbatim via `smolvm machine exec --name <m> -- sh -c`.

[agents.claude]
alpine  = "apk add --no-cache curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
ubuntu  = "apt-get update && apt-get install -y curl bash ca-certificates && curl -fsSL https://claude.ai/install.sh | bash"
default = "curl -fsSL https://claude.ai/install.sh | bash"

[agents.codex]
alpine  = "apk add --no-cache nodejs npm && npm install -g @openai/codex"
ubuntu  = "apt-get update && apt-get install -y nodejs npm && npm install -g @openai/codex"
default = "npm install -g @openai/codex"
"#;

/// Run a known agent's install script inside the guest. The script
/// is resolved via [`BootstrapConfig::agent_script`] which consults
/// user overrides first and falls back to the per-(agent, distro)
/// hardcoded default.
///
/// `image` is the machine's image string (e.g. `"alpine:3.19"`),
/// used by [`distro_from_image`] to pick the right script. Pass
/// `None` if unknown — the `"default"` script will run.
///
/// Synchronous (blocks until install finishes); the UI shows a
/// spinner and a final success/error toast.
pub fn install_agent(
    binary: &Path,
    machine_name: &str,
    agent: KnownAgent,
    image: Option<&str>,
    config: &BootstrapConfig,
) -> Result<(), SmolvmError> {
    let distro = distro_from_image(image);
    let script = config.agent_script(agent, distro);
    run_install_script(binary, machine_name, agent, &script)
}

/// Run a pre-resolved install script inside the guest. Used by the
/// Tauri command path when the script comes from a Phase 2.7 library
/// item rather than the bootstrap config — that resolution needs
/// access to AppState (settings, sources, active session) which the
/// pure `roux_smolvm` crate doesn't have, so callers there resolve
/// the script themselves and call this.
pub fn run_install_script(
    binary: &Path,
    machine_name: &str,
    agent: KnownAgent,
    script: &str,
) -> Result<(), SmolvmError> {
    run_in_guest_sh(
        binary,
        machine_name,
        script,
        &format!("install {}", agent.binary_name()),
    )
}

/// Outcome of [`smolfile_append_init`]. `Appended` means the line
/// was added; `AlreadyPresent` means the array already contained an
/// equal entry (string equality after trimming) and the file was
/// untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Appended,
    AlreadyPresent,
}

/// Append a single shell line to a Smolfile's `[dev].init` array,
/// preserving comments and whitespace via `toml_edit`'s round-trip.
///
/// Behavior:
/// - Creates the `[dev]` table if missing.
/// - Creates the `init` array if missing.
/// - Idempotent: returns `AlreadyPresent` and skips the write when an
///   identical line (after trimming) already exists in the array.
/// - The Smolfile's existing comments and key ordering are preserved
///   in the rest of the file; only the `init` array gains a new entry.
///
/// Errors: I/O failures reading or writing the file, malformed TOML
/// at parse time. Both are wrapped in `SmolvmError::ParseFailed` for
/// caller consistency — the smolvm CLI isn't involved here.
pub fn smolfile_append_init(
    path: &Path,
    line: &str,
) -> Result<AppendOutcome, SmolvmError> {
    use toml_edit::{value, Array, DocumentMut, Item, Table};

    let original = std::fs::read_to_string(path).map_err(|e| SmolvmError::ParseFailed {
        message: format!("could not read Smolfile {path:?}: {e}"),
    })?;
    let mut doc = original
        .parse::<DocumentMut>()
        .map_err(|e| SmolvmError::ParseFailed {
            message: format!("could not parse Smolfile {path:?}: {e}"),
        })?;

    // Ensure [dev] exists.
    if !doc.contains_key("dev") {
        doc["dev"] = Item::Table(Table::new());
    }
    let dev = doc["dev"].as_table_mut().ok_or_else(|| SmolvmError::ParseFailed {
        message: format!("Smolfile {path:?}: `dev` is not a table"),
    })?;

    // Ensure init array exists.
    if !dev.contains_key("init") {
        dev["init"] = value(Array::new());
    }
    let init = dev["init"]
        .as_array_mut()
        .ok_or_else(|| SmolvmError::ParseFailed {
            message: format!("Smolfile {path:?}: `[dev].init` is not an array"),
        })?;

    // Idempotency: skip if an equal entry is already there.
    let trimmed_target = line.trim();
    for existing in init.iter() {
        if let Some(s) = existing.as_str() {
            if s.trim() == trimmed_target {
                return Ok(AppendOutcome::AlreadyPresent);
            }
        }
    }

    init.push(line);

    std::fs::write(path, doc.to_string()).map_err(|e| SmolvmError::ParseFailed {
        message: format!("could not write Smolfile {path:?}: {e}"),
    })?;
    Ok(AppendOutcome::Appended)
}

/// Run a `sh -c <script>` inside the guest and wrap any failure as a
/// `CommandFailed` whose `command` field describes the step (so the
/// panel error reads "smolvm bootstrap failed: …" instead of a
/// generic "machine exec failed").
fn run_in_guest_sh(
    binary: &Path,
    machine_name: &str,
    script: &str,
    label: &str,
) -> Result<(), SmolvmError> {
    let output = Command::new(binary)
        .args(["machine", "exec", "--name", machine_name, "--", "sh", "-c", script])
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

/// POSIX-shell single-quote a string. Each embedded `'` becomes
/// `'\''`. Used for guest binary names passed through `sh -c`. Not a
/// general shell-escape — only correct for arguments where the whole
/// value is the quoted unit.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
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
    /// Forward the host's running SSH agent into the guest so `git
    /// clone git@…` works inside the VM. Private keys never leave
    /// the host — the hypervisor enforces this. The user must have
    /// an agent running with keys (`ssh-add -l` on the host).
    pub ssh_agent: bool,
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
    if opts.ssh_agent {
        args.push("--ssh-agent".into());
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
            ssh_agent: false,
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
            ssh_agent: false,
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
            ssh_agent: false,
        });
        assert_eq!(
            args_to_strings(&args),
            vec!["machine", "create", "vm", "--image", "alpine", "--net"]
        );
    }

    #[test]
    fn known_agent_from_str_is_case_insensitive() {
        assert_eq!(KnownAgent::from_str("claude"), Some(KnownAgent::Claude));
        assert_eq!(KnownAgent::from_str("Claude"), Some(KnownAgent::Claude));
        assert_eq!(KnownAgent::from_str("CLAUDE"), Some(KnownAgent::Claude));
        assert_eq!(KnownAgent::from_str("codex"), Some(KnownAgent::Codex));
        assert_eq!(KnownAgent::from_str("aider"), None);
        assert_eq!(KnownAgent::from_str(""), None);
    }

    #[test]
    fn distro_from_image_alpine_variants() {
        assert_eq!(distro_from_image(Some("alpine")), "alpine");
        assert_eq!(distro_from_image(Some("alpine:3.19")), "alpine");
        assert_eq!(distro_from_image(Some("docker.io/alpine")), "alpine");
        assert_eq!(distro_from_image(Some("ALPINE:latest")), "alpine");
    }

    #[test]
    fn distro_from_image_ubuntu_variants() {
        assert_eq!(distro_from_image(Some("ubuntu")), "ubuntu");
        assert_eq!(distro_from_image(Some("ubuntu:22.04")), "ubuntu");
        assert_eq!(distro_from_image(Some("ghcr.io/foo/ubuntu-base")), "ubuntu");
    }

    #[test]
    fn distro_from_image_unknown_fallthrough() {
        assert_eq!(distro_from_image(Some("debian:12")), "default");
        assert_eq!(distro_from_image(Some("fedora:39")), "default");
        assert_eq!(distro_from_image(Some("")), "default");
        assert_eq!(distro_from_image(None), "default");
    }

    #[test]
    fn known_agent_default_script_alpine() {
        let script = KnownAgent::Claude.default_script_for_distro("alpine");
        assert!(script.contains("apk add"));
        assert!(script.contains("curl -fsSL https://claude.ai/install.sh | bash"));
    }

    #[test]
    fn known_agent_default_script_ubuntu() {
        let script = KnownAgent::Codex.default_script_for_distro("ubuntu");
        assert!(script.contains("apt-get install"));
        assert!(script.contains("npm install -g @openai/codex"));
    }

    #[test]
    fn known_agent_default_script_unknown_distro_uses_default_branch() {
        // Anything that isn't alpine/ubuntu falls through to the
        // bare installer (no prereq install). Caller's responsibility
        // to make sure the guest has the prereqs.
        assert_eq!(
            KnownAgent::Claude.default_script_for_distro("debian"),
            "curl -fsSL https://claude.ai/install.sh | bash",
        );
    }

    #[test]
    fn bootstrap_config_user_override_wins() {
        let toml = r#"
[agents.claude]
alpine = "echo CUSTOM-ALPINE"
"#;
        let config: BootstrapConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.agent_script(KnownAgent::Claude, "alpine"),
            "echo CUSTOM-ALPINE",
        );
        // ubuntu falls through to built-in default since the config
        // didn't override it.
        assert!(config
            .agent_script(KnownAgent::Claude, "ubuntu")
            .contains("apt-get install"));
    }

    #[test]
    fn bootstrap_config_default_key_used_for_unrecognized_distro() {
        let toml = r#"
[agents.claude]
default = "echo CUSTOM-DEFAULT"
"#;
        let config: BootstrapConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.agent_script(KnownAgent::Claude, "default"),
            "echo CUSTOM-DEFAULT",
        );
        // alpine isn't overridden but a "default" exists → that's
        // used as the per-distro fallback before built-in.
        assert_eq!(
            config.agent_script(KnownAgent::Claude, "alpine"),
            "echo CUSTOM-DEFAULT",
        );
    }

    #[test]
    fn bootstrap_config_empty_falls_back_to_builtin() {
        let config = BootstrapConfig::default();
        assert!(config
            .agent_script(KnownAgent::Claude, "alpine")
            .contains("apk add"));
        assert!(config
            .agent_script(KnownAgent::Codex, "ubuntu")
            .contains("apt-get install"));
    }

    fn write_temp_smolfile(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Smolfile");
        std::fs::write(&path, contents).expect("write");
        (dir, path)
    }

    #[test]
    fn smolfile_append_init_appends_to_existing_array() {
        let (_dir, path) = write_temp_smolfile(
            r#"image = "alpine:3.19"

[dev]
init = ["echo hi"]
"#,
        );
        let outcome = smolfile_append_init(&path, "echo bye").unwrap();
        assert_eq!(outcome, AppendOutcome::Appended);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"echo hi\""));
        assert!(body.contains("\"echo bye\""));
        // image line is still there — round-trip preserved the rest.
        assert!(body.contains("image = \"alpine:3.19\""));
    }

    #[test]
    fn smolfile_append_init_creates_dev_section_when_missing() {
        let (_dir, path) = write_temp_smolfile(
            r#"# A comment that must survive.
image = "alpine:3.19"
"#,
        );
        smolfile_append_init(&path, "echo hi").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# A comment that must survive."));
        assert!(body.contains("[dev]"));
        assert!(body.contains("\"echo hi\""));
    }

    #[test]
    fn smolfile_append_init_creates_init_when_dev_exists_but_init_missing() {
        let (_dir, path) = write_temp_smolfile(
            r#"image = "alpine:3.19"

[dev]
volumes = []
"#,
        );
        smolfile_append_init(&path, "echo hi").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("init"));
        assert!(body.contains("\"echo hi\""));
        // pre-existing [dev] keys still there.
        assert!(body.contains("volumes"));
    }

    #[test]
    fn smolfile_append_init_idempotent_when_line_already_present() {
        let (_dir, path) = write_temp_smolfile(
            r#"image = "alpine:3.19"

[dev]
init = ["echo hi"]
"#,
        );
        let outcome = smolfile_append_init(&path, "echo hi").unwrap();
        assert_eq!(outcome, AppendOutcome::AlreadyPresent);
        // File body unchanged.
        let body = std::fs::read_to_string(&path).unwrap();
        let count = body.matches("echo hi").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn smolfile_append_init_preserves_comments_in_array() {
        // toml_edit doesn't perfectly preserve comments inside an
        // inline array across all edits, but at minimum the
        // surrounding comments + other keys should survive.
        let (_dir, path) = write_temp_smolfile(
            r#"# top comment
image = "alpine:3.19"

[dev]
# bootstrap commands run on every machine start
init = ["echo hi"]
"#,
        );
        smolfile_append_init(&path, "echo bye").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# top comment"));
        assert!(body.contains("# bootstrap commands run on every machine start"));
    }

    #[test]
    fn shell_single_quote_simple() {
        assert_eq!(shell_single_quote("claude"), "'claude'");
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quote() {
        // Guard: a hostile `guest_binary` like `foo'; rm -rf /; '`
        // would otherwise break out of single-quoting and run as a
        // command. Embedded `'` becomes `'\''` (close, escaped, reopen).
        assert_eq!(shell_single_quote("foo'bar"), "'foo'\\''bar'");
    }

    #[test]
    fn build_create_args_ssh_agent_emits_flag() {
        let args = build_create_args(&CreateOpts {
            name: "vm",
            smolfile_path: None,
            image: Some("alpine"),
            network: true,
            ssh_agent: true,
        });
        // Flag order is stable (name, --smolfile, --image, --net,
        // --ssh-agent) so the test asserts the full vector.
        assert_eq!(
            args_to_strings(&args),
            vec!["machine", "create", "vm", "--image", "alpine", "--net", "--ssh-agent"],
        );
    }

    #[test]
    fn build_create_args_ssh_agent_off_omits_flag() {
        let args = build_create_args(&CreateOpts {
            name: "vm",
            smolfile_path: None,
            image: Some("alpine"),
            network: false,
            ssh_agent: false,
        });
        assert!(!args_to_strings(&args).contains(&"--ssh-agent".to_string()));
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
            ssh_agent: false,
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
