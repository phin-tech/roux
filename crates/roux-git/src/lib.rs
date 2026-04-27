//! Shell-out wrapper around the user's native `git` CLI.
//!
//! Roux intentionally shells out instead of embedding a Git implementation so
//! clone/fetch/pull honor the user's SSH config, credential helpers, and
//! corporate Git setup.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct GitCli {
    git_bin: PathBuf,
}

impl Default for GitCli {
    fn default() -> Self {
        Self::new(resolve_git_bin())
    }
}

impl GitCli {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { git_bin: path.into() }
    }

    pub fn clone_repo(
        &self,
        url: &str,
        branch: Option<&str>,
        target: &Path,
    ) -> Result<(), GitError> {
        if target.exists() {
            if self.is_repo(target) {
                self.validate_existing_checkout(url, branch, target)?;
                return Ok(());
            }
            return Err(GitError::TargetExists { path: target.to_path_buf() });
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| GitError::CreateDir { path: parent.to_path_buf(), source })?;
        }
        let mut args = vec!["clone".to_string()];
        if let Some(branch) = branch.filter(|branch| !branch.trim().is_empty()) {
            args.push("--branch".to_string());
            args.push(branch.to_string());
            args.push("--single-branch".to_string());
        }
        args.push(url.to_string());
        args.push(target.to_string_lossy().into_owned());
        match self.run(None, &args) {
            Ok(_) => Ok(()),
            Err(err) => {
                let _ = std::fs::remove_dir_all(target);
                Err(err)
            }
        }
    }

    pub fn fetch_origin(&self, repo: &Path) -> Result<(), GitError> {
        self.run(Some(repo), &["fetch".into(), "--prune".into(), "origin".into()]).map(|_| ())
    }

    pub fn checkout_branch(&self, repo: &Path, branch: &str) -> Result<(), GitError> {
        self.run(Some(repo), &["checkout".into(), branch.to_string()]).map(|_| ())
    }

    pub fn pull_ff_only(&self, repo: &Path, branch: Option<&str>) -> Result<(), GitError> {
        let mut args = vec!["pull".to_string(), "--ff-only".to_string()];
        if let Some(branch) = branch.filter(|branch| !branch.trim().is_empty()) {
            args.push("origin".to_string());
            args.push(branch.to_string());
        }
        self.run(Some(repo), &args).map(|_| ())
    }

    pub fn sync_branch(&self, repo: &Path, branch: Option<&str>) -> Result<(), GitError> {
        self.fetch_origin(repo)?;
        if let Some(branch) = branch.filter(|branch| !branch.trim().is_empty()) {
            self.checkout_branch(repo, branch)?;
        }
        self.pull_ff_only(repo, branch)
    }

    pub fn status(&self, repo: &Path) -> Result<GitRepoStatus, GitError> {
        if !self.is_repo(repo) {
            return Err(GitError::NotRepo { path: repo.to_path_buf() });
        }
        let branch =
            self.stdout(repo, &["branch", "--show-current"]).ok().filter(|s| !s.is_empty());
        let dirty =
            self.stdout(repo, &["status", "--porcelain"]).map(|out| !out.trim().is_empty())?;
        let tracking_branch = self
            .stdout(repo, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .ok()
            .filter(|s| !s.is_empty());
        let (ahead, behind) = if tracking_branch.is_some() {
            self.ahead_behind(repo, "HEAD...@{u}").unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        let remote_state = RemoteState::from_counts(tracking_branch.is_some(), ahead, behind);
        let default_branch = self
            .stdout(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
            .ok()
            .map(|name| name.strip_prefix("origin/").unwrap_or(&name).to_string())
            .filter(|s| !s.is_empty());
        let behind_default = match (&branch, &default_branch) {
            (Some(branch), Some(default)) if branch != default => self
                .stdout(repo, &["rev-list", "--count", &format!("HEAD..origin/{default}")])
                .ok()
                .and_then(|count| count.parse::<u32>().ok()),
            _ => None,
        };

        Ok(GitRepoStatus {
            branch,
            tracking_branch,
            default_branch,
            dirty,
            ahead,
            behind,
            behind_default,
            remote_state,
        })
    }

    pub fn is_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
            && self.run(Some(path), &["rev-parse".into(), "--is-inside-work-tree".into()]).is_ok()
    }

    fn validate_existing_checkout(
        &self,
        url: &str,
        branch: Option<&str>,
        target: &Path,
    ) -> Result<(), GitError> {
        let existing_url = self.stdout(target, &["remote", "get-url", "origin"])?;
        let requested_url = url.trim();
        if existing_url.trim() != requested_url {
            return Err(GitError::TargetMismatch {
                path: target.to_path_buf(),
                expected: requested_url.to_string(),
                actual: existing_url,
            });
        }
        if let Some(branch) = branch.filter(|branch| !branch.trim().is_empty()) {
            let current = self.stdout(target, &["branch", "--show-current"])?;
            if current.trim() != branch.trim() {
                return Err(GitError::TargetMismatch {
                    path: target.to_path_buf(),
                    expected: branch.trim().to_string(),
                    actual: current,
                });
            }
        }
        Ok(())
    }

    fn ahead_behind(&self, repo: &Path, range: &str) -> Result<(u32, u32), GitError> {
        parse_ahead_behind(&self.stdout(repo, &["rev-list", "--left-right", "--count", range])?)
    }

    fn stdout(&self, repo: &Path, args: &[&str]) -> Result<String, GitError> {
        let args = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
        self.run(Some(repo), &args)
    }

    fn run(&self, cwd: Option<&Path>, args: &[String]) -> Result<String, GitError> {
        run_git(&self.git_bin, cwd, args)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoStatus {
    pub branch: Option<String>,
    pub tracking_branch: Option<String>,
    pub default_branch: Option<String>,
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    pub behind_default: Option<u32>,
    pub remote_state: RemoteState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteState {
    UpToDate,
    Ahead,
    Behind,
    Diverged,
    Unknown,
}

impl RemoteState {
    fn from_counts(has_tracking: bool, ahead: u32, behind: u32) -> Self {
        match (has_tracking, ahead, behind) {
            (false, _, _) => Self::Unknown,
            (true, 0, 0) => Self::UpToDate,
            (true, 0, _) => Self::Behind,
            (true, _, 0) => Self::Ahead,
            (true, _, _) => Self::Diverged,
        }
    }
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to create {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to run git: {source}")]
    Spawn {
        #[source]
        source: std::io::Error,
    },
    #[error("git exited with status {status}: {stderr}")]
    NonZeroExit { status: i32, stderr: String },
    #[error("target path already exists and is not a git repo: {path}")]
    TargetExists { path: PathBuf },
    #[error("target git checkout does not match requested source at {path}: expected {expected}, found {actual}")]
    TargetMismatch { path: PathBuf, expected: String, actual: String },
    #[error("not a git repo: {path}")]
    NotRepo { path: PathBuf },
    #[error("failed to parse git output: {message}")]
    Parse { message: String },
}

fn run_git(git_bin: &Path, cwd: Option<&Path>, args: &[String]) -> Result<String, GitError> {
    let mut command = duct::cmd(git_bin, args);
    if let Some(cwd) = cwd {
        command = command.dir(cwd);
    }
    let output = command
        .unchecked()
        .stderr_capture()
        .stdout_capture()
        .run()
        .map_err(|source| GitError::Spawn { source })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(GitError::NonZeroExit {
        status: output.status.code().unwrap_or(-1),
        stderr: truncate(stderr.trim(), 240),
    })
}

fn resolve_git_bin() -> PathBuf {
    if let Some(path) = std::env::var_os("ROUX_GIT").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    resolve_git_bin_with_path(std::env::var_os("PATH").as_deref(), &common_git_candidates())
}

fn resolve_git_bin_with_path(path_env: Option<&OsStr>, candidates: &[PathBuf]) -> PathBuf {
    let executable = git_executable_name();
    if let Some(path_env) = path_env {
        for dir in std::env::split_paths(path_env) {
            let candidate = dir.join(executable);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }
    PathBuf::from(executable)
}

fn common_git_candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        Vec::new()
    } else {
        vec![
            PathBuf::from("/usr/bin/git"),
            PathBuf::from("/opt/homebrew/bin/git"),
            PathBuf::from("/usr/local/bin/git"),
        ]
    }
}

fn git_executable_name() -> &'static str {
    if cfg!(windows) {
        "git.exe"
    } else {
        "git"
    }
}

fn parse_ahead_behind(output: &str) -> Result<(u32, u32), GitError> {
    let mut parts = output.split_whitespace();
    let ahead = parts
        .next()
        .ok_or_else(|| GitError::Parse { message: "missing ahead count".into() })?
        .parse::<u32>()
        .map_err(|_| GitError::Parse { message: format!("invalid ahead count: {output}") })?;
    let behind = parts
        .next()
        .ok_or_else(|| GitError::Parse { message: "missing behind count".into() })?
        .parse::<u32>()
        .map_err(|_| GitError::Parse { message: format!("invalid behind count: {output}") })?;
    Ok((ahead, behind))
}

fn truncate(input: &str, limit: usize) -> String {
    input.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_state_from_counts() {
        assert_eq!(RemoteState::from_counts(false, 0, 0), RemoteState::Unknown);
        assert_eq!(RemoteState::from_counts(true, 0, 0), RemoteState::UpToDate);
        assert_eq!(RemoteState::from_counts(true, 2, 0), RemoteState::Ahead);
        assert_eq!(RemoteState::from_counts(true, 0, 2), RemoteState::Behind);
        assert_eq!(RemoteState::from_counts(true, 1, 2), RemoteState::Diverged);
    }

    #[test]
    fn parses_ahead_behind_counts() {
        assert_eq!(parse_ahead_behind("3\t7\n").unwrap(), (3, 7));
        assert_eq!(parse_ahead_behind("0 12").unwrap(), (0, 12));
    }

    #[test]
    fn invalid_ahead_behind_counts_error() {
        assert!(parse_ahead_behind("x 1").is_err());
        assert!(parse_ahead_behind("1").is_err());
    }

    #[test]
    fn truncates_by_chars() {
        assert_eq!(truncate("abcdef", 3), "abc");
    }

    #[test]
    fn resolves_git_from_candidates_when_path_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let candidate = tmp.path().join(git_executable_name());
        std::fs::write(&candidate, "").unwrap();

        assert_eq!(resolve_git_bin_with_path(None, std::slice::from_ref(&candidate)), candidate);
    }

    #[test]
    fn existing_checkout_must_match_requested_source() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = GitCli::default();
        git.run(Some(&repo), &["init".into()]).unwrap();
        git.run(
            Some(&repo),
            &["remote".into(), "add".into(), "origin".into(), "https://example.com/old.git".into()],
        )
        .unwrap();

        let err = git.clone_repo("https://example.com/new.git", None, &repo).unwrap_err();
        assert!(matches!(err, GitError::TargetMismatch { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn failed_clone_removes_new_checkout_directory() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let fake_git = tmp.path().join("git");
        std::fs::write(
            &fake_git,
            "#!/bin/sh\nif [ \"$1\" = \"clone\" ]; then\n  mkdir -p \"$3\"\n  exit 42\nfi\nexit 42\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();

        let target = tmp.path().join("checkout");
        let git = GitCli::new(&fake_git);
        let err = git.clone_repo("https://example.com/repo.git", None, &target).unwrap_err();

        assert!(matches!(err, GitError::NonZeroExit { status: 42, .. }));
        assert!(!target.exists());
    }
}
