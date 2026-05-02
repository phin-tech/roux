use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrInfo {
    pub number: u32,
    pub title: String,
    pub head_ref: String,
    pub head_owner: String,
    pub is_cross_repository: bool,
    pub url: String,
    pub repo_slug: String,
}

/// Parse either a full GitHub PR URL or a shortform `owner/repo#NNN`.
/// Returns `None` if the input doesn't match a recognizable form.
pub(crate) fn parse_pr_ref(input: &str) -> Option<PrRef> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(r) = parse_url_form(trimmed) {
        return Some(r);
    }
    parse_shortform(trimmed)
}

fn parse_url_form(input: &str) -> Option<PrRef> {
    let without_scheme = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))?;
    let host_end = without_scheme.find('/')?;
    let host = &without_scheme[..host_end];
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let path = &without_scheme[host_end + 1..];
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let path = path.trim_end_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 {
        return None;
    }
    if parts[2] != "pull" && parts[2] != "pulls" {
        return None;
    }
    let number: u32 = parts[3].parse().ok()?;
    let owner = parts[0];
    let repo = parts[1];
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(PrRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

fn parse_shortform(input: &str) -> Option<PrRef> {
    // owner/repo#N
    let (slug, num) = input.split_once('#')?;
    let number: u32 = num.trim().parse().ok()?;
    let (owner, repo) = slug.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(PrRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

/// Call `gh pr view --json ...` for the given PR and parse the result.
/// Runs in `repo_path` so gh's auth/config context matches the user's
/// normal workflow (gh reads `~/.config/gh/hosts.yml` globally, but some
/// env-based configs are cwd-sensitive).
pub(crate) fn lookup_pr(repo_path: Option<&str>, input: &str) -> Result<PrInfo> {
    let pr_ref = parse_pr_ref(input)
        .ok_or_else(|| anyhow!("Not a valid GitHub PR URL or shortform"))?;

    let repo_slug = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    let mut cmd = Command::new(crate::services::setup::gh_command());
    cmd.args([
        "pr",
        "view",
        &pr_ref.number.to_string(),
        "--repo",
        &repo_slug,
        "--json",
        "number,title,headRefName,headRepositoryOwner,isCrossRepository,url",
    ]);
    // cwd only matters for gh's repo auto-detection — irrelevant here since
    // we pass --repo explicitly. Still, prefer a real dir when available so
    // gh doesn't barf on a dangling cwd.
    if let Some(path) = repo_path {
        if !path.is_empty() {
            cmd.current_dir(path);
        }
    }
    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_gh_error(&stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw: RawPr = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("Failed to parse gh output: {}", e))?;

    Ok(raw.into_pr_info(&repo_slug))
}

#[derive(Deserialize)]
struct RawPr {
    number: u32,
    title: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "headRepositoryOwner")]
    head_repository_owner: RawOwner,
    #[serde(rename = "isCrossRepository")]
    is_cross_repository: bool,
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct RawOwner {
    login: String,
}

impl RawPr {
    fn into_pr_info(self, repo_slug: &str) -> PrInfo {
        PrInfo {
            number: self.number,
            title: self.title,
            head_ref: self.head_ref_name,
            head_owner: self.head_repository_owner.login,
            is_cross_repository: self.is_cross_repository,
            url: self.url,
            repo_slug: repo_slug.to_string(),
        }
    }
}

/// Look up the open PR whose head branch matches `branch` in the repo at
/// `repo_path`. Returns `Ok(None)` when no such PR exists (the empty case
/// is normal — not every branch has a PR yet). Cross-repo PRs whose local
/// branch was renamed by `fetch_pr_branch` to `pr-<N>` are recognized via
/// the `pr-<N>` shape and resolved through `lookup_pr` against the repo's
/// own slug, since `gh pr list --head` does not accept `<owner>:<branch>`
/// syntax (verified via `gh pr list --help`).
pub(crate) fn lookup_pr_for_branch(
    repo_path: &str,
    branch: &str,
) -> Result<Option<PrInfo>> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let repo_slug = resolve_repo_slug(repo_path)?;

    if let Some(num_str) = trimmed.strip_prefix("pr-") {
        if let Ok(num) = num_str.parse::<u32>() {
            // Cross-repo PR fetched via fetch_pr_branch — we know the repo
            // slug from `gh repo view`, so resolve directly.
            let shortform = format!("{}#{}", repo_slug, num);
            return match lookup_pr(Some(repo_path), &shortform) {
                Ok(info) => Ok(Some(info)),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("PR not found") {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            };
        }
    }

    let mut cmd = Command::new(crate::services::setup::gh_command());
    cmd.args([
        "pr",
        "list",
        "--head",
        trimmed,
        "--state",
        "open",
        "--limit",
        "1",
        "--json",
        "number,title,headRefName,headRepositoryOwner,isCrossRepository,url",
    ]);
    cmd.current_dir(repo_path);

    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to run gh: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_gh_error(&stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raws: Vec<RawPr> = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("Failed to parse gh output: {}", e))?;
    Ok(raws.into_iter().next().map(|r| r.into_pr_info(&repo_slug)))
}

/// Resolve `<owner>/<repo>` for the repo at `repo_path` using `gh repo view`.
/// gh reads the repo from cwd's git remotes, so this is a per-path call.
fn resolve_repo_slug(repo_path: &str) -> Result<String> {
    let mut cmd = Command::new(crate::services::setup::gh_command());
    cmd.args(["repo", "view", "--json", "nameWithOwner"]);
    cmd.current_dir(repo_path);
    let output = cmd
        .output()
        .map_err(|e| anyhow!("Failed to run gh: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_gh_error(&stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }
    let raw: Raw = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("Failed to parse gh repo view output: {}", e))?;
    Ok(raw.name_with_owner)
}

fn classify_gh_error(stderr: &str) -> anyhow::Error {
    let s = stderr.to_lowercase();
    if s.contains("authentication") || s.contains("gh auth") || s.contains("not logged in") {
        anyhow!("gh is not authenticated — run 'gh auth login' and retry")
    } else if s.contains("could not resolve") || s.contains("not found") || s.contains("404") {
        anyhow!("PR not found")
    } else {
        let trimmed = stderr.trim();
        let snippet: String = trimmed.chars().take(200).collect();
        anyhow!("Failed to fetch PR: {}", snippet)
    }
}

/// Fetch the PR's head commit into a local branch in `repo_path`, without
/// moving HEAD. Uses `refs/pull/<N>/head` which GitHub exposes for every PR
/// (same-repo and fork alike). Returns the local branch name.
///
/// For same-repo PRs the target branch name matches the PR's head ref, so
/// existing worktree detection finds the branch naturally. For cross-repo
/// PRs the target is `pr-<N>` — the fork's branch name isn't guaranteed to
/// be a valid or meaningful local ref.
pub(crate) fn fetch_pr_branch(
    repo_path: &str,
    number: u32,
    head_ref: &str,
    is_cross_repository: bool,
) -> Result<String> {
    let local_branch = if is_cross_repository {
        format!("pr-{}", number)
    } else {
        head_ref.to_string()
    };
    let refspec = format!("+refs/pull/{}/head:refs/heads/{}", number, local_branch);
    let output = Command::new("git")
        .args(["fetch", "origin", &refspec])
        .current_dir(repo_path)
        .output()
        .map_err(|e| anyhow!("Failed to run git: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        let snippet: String = trimmed.chars().take(200).collect();
        return Err(anyhow!("Failed to fetch PR branch: {}", snippet));
    }
    Ok(local_branch)
}

/// Clone `owner/repo` from GitHub into `target_dir` using `gh repo clone`.
/// The caller is responsible for ensuring `target_dir`'s parent exists and
/// `target_dir` itself does not already exist. Returns `target_dir` on
/// success (the caller already knows the path, but returning it keeps the
/// command symmetrical with fetch_pr_branch).
pub(crate) fn clone_repo(owner: &str, repo: &str, target_dir: &str) -> Result<String> {
    if std::path::Path::new(target_dir).exists() {
        return Err(anyhow!("Target path already exists: {}", target_dir));
    }
    if let Some(parent) = std::path::Path::new(target_dir).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("Failed to create parent dir: {}", e))?;
        }
    }
    let slug = format!("{}/{}", owner, repo);
    let output = Command::new(crate::services::setup::gh_command())
        .args(["repo", "clone", &slug, target_dir])
        .output()
        .map_err(|e| anyhow!("Failed to run gh: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_gh_error(&stderr));
    }
    Ok(target_dir.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_url() {
        let r = parse_pr_ref("https://github.com/phin-tech/roux/pull/142").unwrap();
        assert_eq!(r.owner, "phin-tech");
        assert_eq!(r.repo, "roux");
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_url_with_trailing_slash() {
        let r = parse_pr_ref("https://github.com/phin-tech/roux/pull/142/").unwrap();
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_url_with_query_string() {
        let r = parse_pr_ref("https://github.com/phin-tech/roux/pull/142?foo=bar").unwrap();
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_url_with_fragment() {
        let r =
            parse_pr_ref("https://github.com/phin-tech/roux/pull/142#issuecomment-123").unwrap();
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_url_with_files_tab() {
        // We deliberately accept any path ending after the PR number; /files
        // and /commits are common tabs users paste from.
        let r = parse_pr_ref("https://github.com/phin-tech/roux/pull/142/files").unwrap();
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_shortform() {
        let r = parse_pr_ref("phin-tech/roux#142").unwrap();
        assert_eq!(r.owner, "phin-tech");
        assert_eq!(r.repo, "roux");
        assert_eq!(r.number, 142);
    }

    #[test]
    fn parses_shortform_with_whitespace() {
        let r = parse_pr_ref("  phin-tech/roux#142  ").unwrap();
        assert_eq!(r.number, 142);
    }

    #[test]
    fn rejects_non_github_host() {
        assert!(parse_pr_ref("https://gitlab.com/foo/bar/pull/1").is_none());
    }

    #[test]
    fn rejects_non_pr_path() {
        assert!(parse_pr_ref("https://github.com/foo/bar/issues/1").is_none());
    }

    #[test]
    fn rejects_missing_number() {
        assert!(parse_pr_ref("https://github.com/foo/bar/pull/").is_none());
        assert!(parse_pr_ref("https://github.com/foo/bar/pull").is_none());
    }

    #[test]
    fn rejects_non_numeric_pr_id() {
        assert!(parse_pr_ref("https://github.com/foo/bar/pull/abc").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_pr_ref("").is_none());
        assert!(parse_pr_ref("not a url").is_none());
        assert!(parse_pr_ref("foo#1").is_none());
    }
}
