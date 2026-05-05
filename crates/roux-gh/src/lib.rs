//! Async wrapper around the user's `gh` CLI.
//!
//! Roux shells out to `gh` instead of speaking to GitHub directly so we honor
//! the user's `~/.config/gh/hosts.yml`, OAuth tokens, enterprise hosts, and
//! whatever else `gh` already negotiates. The wrapper centralizes process
//! spawning, error classification, and the small number of subcommands Roux
//! actually needs.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use thiserror::Error;
use tokio::process::Command;

/// A handle to a `gh` binary at a known path. Cheap to clone (just a `PathBuf`).
#[derive(Debug, Clone)]
pub struct GhCli {
    gh_bin: PathBuf,
}

impl Default for GhCli {
    /// Returns a `GhCli` that resolves `gh` via `$PATH` at spawn time.
    fn default() -> Self {
        Self::new("gh")
    }
}

impl GhCli {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { gh_bin: path.into() }
    }

    pub fn gh_bin(&self) -> &Path {
        &self.gh_bin
    }

    /// `gh pr view <number> --repo <slug> --json <fields>`. Returns stdout
    /// as JSON-bearing UTF-8. `cwd` only affects `gh`'s repo auto-detection,
    /// which we explicitly bypass with `--repo`; pass a real directory when
    /// available so `gh` doesn't barf on a dangling cwd.
    pub async fn pr_view(
        &self,
        slug: &str,
        number: u32,
        fields: &str,
        cwd: Option<&Path>,
    ) -> Result<String, GhError> {
        let number_str = number.to_string();
        let args = ["pr", "view", &number_str, "--repo", slug, "--json", fields];
        self.run(cwd, &args).await
    }

    /// `gh pr list --head <branch> --state open --limit 1 --json <fields>` in
    /// `cwd`. `cwd` is required: `gh pr list` reads the repo from cwd's git
    /// remotes when `--repo` is omitted.
    pub async fn pr_list_by_head(
        &self,
        branch: &str,
        fields: &str,
        cwd: &Path,
    ) -> Result<String, GhError> {
        let args =
            ["pr", "list", "--head", branch, "--state", "open", "--limit", "1", "--json", fields];
        self.run(Some(cwd), &args).await
    }

    /// `gh pr list --search "head:<branch> repo:<slug> is:pr is:open"
    /// --limit 1 --json <fields>` in `cwd`. GitHub's search index is
    /// eventually consistent — freshly-opened PRs can take seconds to show
    /// up. The caller usually wants to fall back to `Ok(empty json array)`
    /// rather than treat permission failures as fatal; we expose the error
    /// kind so the caller can decide.
    pub async fn pr_search_by_head(
        &self,
        branch: &str,
        slug: &str,
        fields: &str,
        cwd: &Path,
    ) -> Result<String, GhError> {
        let query = format!("head:{branch} repo:{slug} is:pr is:open");
        let args = ["pr", "list", "--search", &query, "--limit", "1", "--json", fields];
        self.run(Some(cwd), &args).await
    }

    /// `gh repo view --json nameWithOwner` in `cwd`. Returns stdout (caller
    /// parses the JSON). `gh` reads the repo from `cwd`'s git remotes, so
    /// this is a per-path call.
    pub async fn repo_view_name_with_owner(&self, cwd: &Path) -> Result<String, GhError> {
        let args = ["repo", "view", "--json", "nameWithOwner"];
        self.run(Some(cwd), &args).await
    }

    /// `gh repo clone <slug> <target>`. The caller is responsible for
    /// ensuring `target_dir`'s parent exists and `target_dir` itself does
    /// not already exist.
    pub async fn repo_clone(&self, slug: &str, target_dir: &Path) -> Result<(), GhError> {
        let target_str = target_dir.to_string_lossy().into_owned();
        let args = ["repo", "clone", slug, &target_str];
        self.run(None, &args).await.map(|_| ())
    }

    /// Probe `gh --version`. Returns `(binary_path, version_string)` on
    /// success — version is the third whitespace token of the first line.
    /// Sync because it's only called on startup.
    pub fn version_blocking(&self) -> Option<(String, String)> {
        let out = std::process::Command::new(&self.gh_bin).arg("--version").output().ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        // "gh version 2.60.1 (2024-12-11)"
        let version = stdout.lines().next()?.split_whitespace().nth(2)?.to_string();
        Some((self.gh_bin.to_string_lossy().into_owned(), version))
    }

    async fn run<I, S>(&self, cwd: Option<&Path>, args: I) -> Result<String, GhError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(&self.gh_bin);
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Kill the gh child if the future is dropped (e.g. Tauri command
        // cancellation, test timeout). Without this a `gh repo clone` can
        // continue running detached.
        cmd.kill_on_drop(true);

        let output = cmd.output().await.map_err(|source| GhError::Spawn { source })?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(classify(&stderr))
    }
}

/// Typed errors from `gh` invocations. The `NotAuthenticated` and `NotFound`
/// variants are the two shapes the UI cares about specifically; everything
/// else collapses into `Other`.
#[derive(Debug, Error)]
pub enum GhError {
    #[error("gh is not authenticated — run 'gh auth login' and retry")]
    NotAuthenticated,
    #[error("not found")]
    NotFound,
    #[error("failed to run gh: {source}")]
    Spawn {
        #[source]
        source: std::io::Error,
    },
    #[error("gh failed: {0}")]
    Other(String),
}

impl GhError {
    /// True for permission-shaped failures (auth + 403/404 patterns) where
    /// the caller may want to fall back rather than surface an error.
    pub fn is_auth_or_not_found(&self) -> bool {
        matches!(self, GhError::NotAuthenticated | GhError::NotFound)
    }
}

/// Resolve the path to the user's `gh` binary.
///
/// Resolution precedence:
///   1. `override_path` (e.g. a settings value), if non-empty.
///   2. First match in `extra_path` (e.g. a login-shell `PATH` for GUI
///      launches that don't inherit the user's shell PATH).
///   3. First match in the process `PATH`.
///   4. Bare `"gh"` — `Command::new` then errors naturally.
pub fn resolve_bin(override_path: Option<&str>, extra_path: Option<&OsStr>) -> PathBuf {
    if let Some(path) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(path);
    }
    let executable = gh_executable_name();
    if let Some(path_env) = extra_path {
        if let Some(found) = find_in_path_env(path_env, executable) {
            return found;
        }
    }
    if let Some(path_env) = std::env::var_os("PATH") {
        if let Some(found) = find_in_path_env(&path_env, executable) {
            return found;
        }
    }
    PathBuf::from(executable)
}

fn find_in_path_env(path_env: &OsStr, executable: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_env) {
        let candidate = dir.join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn gh_executable_name() -> &'static str {
    if cfg!(windows) {
        "gh.exe"
    } else {
        "gh"
    }
}

/// True iff a `gh` binary is locatable via [`resolve_bin`] with the given
/// override + extra-path hints. Cheap (no subprocess spawn).
pub fn is_available(override_path: Option<&str>, extra_path: Option<&OsStr>) -> bool {
    if let Some(path) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        return Path::new(path).is_file();
    }
    let executable = gh_executable_name();
    if let Some(path_env) = extra_path {
        if find_in_path_env(path_env, executable).is_some() {
            return true;
        }
    }
    if let Some(path_env) = std::env::var_os("PATH") {
        if find_in_path_env(&path_env, executable).is_some() {
            return true;
        }
    }
    false
}

fn classify(stderr: &str) -> GhError {
    let s = stderr.to_lowercase();
    if s.contains("authentication") || s.contains("gh auth") || s.contains("not logged in") {
        GhError::NotAuthenticated
    } else if s.contains("could not resolve") || s.contains("not found") || s.contains("404") {
        GhError::NotFound
    } else {
        let trimmed = stderr.trim();
        let snippet: String = trimmed.chars().take(200).collect();
        GhError::Other(snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_auth_messages_route_to_not_authenticated() {
        assert!(matches!(classify("authentication required"), GhError::NotAuthenticated));
        assert!(matches!(classify("you are not logged in"), GhError::NotAuthenticated));
        assert!(matches!(classify("Try `gh auth login`"), GhError::NotAuthenticated));
    }

    #[test]
    fn classify_404_routes_to_not_found() {
        assert!(matches!(classify("HTTP 404: Not Found"), GhError::NotFound));
        assert!(matches!(classify("could not resolve repository"), GhError::NotFound));
    }

    #[test]
    fn classify_other_truncates_to_200_chars() {
        let big = "x".repeat(500);
        let err = classify(&big);
        match err {
            GhError::Other(msg) => assert_eq!(msg.len(), 200),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn is_auth_or_not_found_only_fires_for_those_kinds() {
        assert!(GhError::NotAuthenticated.is_auth_or_not_found());
        assert!(GhError::NotFound.is_auth_or_not_found());
        assert!(!GhError::Other("nope".into()).is_auth_or_not_found());
    }
}
