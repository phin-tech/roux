use anyhow::{anyhow, Result};
use roux_gh::{GhCli, GhError};
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    /// Aggregate check status — feeds the status-bar checks icon.
    /// `None` when the lookup didn't (or couldn't) include the rollup.
    pub checks: Option<PrChecksSummary>,
    /// Individual checks from GitHub's `statusCheckRollup` for the
    /// status-bar hover popover.
    pub check_runs: Vec<PrCheckDetails>,
    /// GitHub's `reviewDecision` enum — `"APPROVED"` |
    /// `"CHANGES_REQUESTED"` | `"REVIEW_REQUIRED"`. Mapped 1:1 from gh.
    pub review_decision: Option<String>,
    /// Latest review per reviewer from GitHub's `latestReviews`; feeds the
    /// status-bar review hover popover.
    pub review_details: Vec<PrReviewDetails>,
}

/// Aggregate of a PR's check runs, derived from gh's
/// `statusCheckRollup`. We collapse to a single "worst-of" state plus
/// counts so the status bar can render a tiny icon without re-deriving
/// the rollup on every render.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrChecksSummary {
    /// `"passing"` | `"failing"` | `"pending"` | `"none"`.
    /// `"none"` means there are no check runs at all (empty rollup).
    pub state: PrChecksState,
    pub passing: u32,
    pub failing: u32,
    pub pending: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PrChecksState {
    Passing,
    Failing,
    Pending,
    None,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrCheckDetails {
    pub name: String,
    pub status: PrCheckStatus,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PrCheckStatus {
    Passing,
    Failing,
    Pending,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrReviewDetails {
    pub reviewer: String,
    pub state: String,
    pub url: Option<String>,
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
    let without_scheme =
        input.strip_prefix("https://").or_else(|| input.strip_prefix("http://"))?;
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
    Some(PrRef { owner: owner.to_string(), repo: repo.to_string(), number })
}

fn parse_shortform(input: &str) -> Option<PrRef> {
    // owner/repo#N
    let (slug, num) = input.split_once('#')?;
    let number: u32 = num.trim().parse().ok()?;
    let (owner, repo) = slug.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(PrRef { owner: owner.to_string(), repo: repo.to_string(), number })
}

fn gh_cli() -> GhCli {
    GhCli::new(crate::services::setup::gh_command())
}

fn nonempty_path(repo_path: Option<&str>) -> Option<&Path> {
    repo_path.filter(|p| !p.is_empty()).map(Path::new)
}

/// Call `gh pr view --json ...` for the given PR and parse the result.
/// Runs in `repo_path` so gh's auth/config context matches the user's
/// normal workflow (gh reads `~/.config/gh/hosts.yml` globally, but some
/// env-based configs are cwd-sensitive).
pub(crate) async fn lookup_pr(repo_path: Option<&str>, input: &str) -> Result<PrInfo> {
    let pr_ref =
        parse_pr_ref(input).ok_or_else(|| anyhow!("Not a valid GitHub PR URL or shortform"))?;
    let repo_slug = format!("{}/{}", pr_ref.owner, pr_ref.repo);
    let stdout = gh_cli()
        .pr_view(&repo_slug, pr_ref.number, PR_JSON_FIELDS, nonempty_path(repo_path))
        .await
        .map_err(gh_to_anyhow_pr)?;
    let raw: RawPr =
        serde_json::from_str(&stdout).map_err(|e| anyhow!("Failed to parse gh output: {}", e))?;
    Ok(raw.into_pr_info(&repo_slug))
}

/// JSON fields requested from `gh pr view` / `gh pr list`. Centralized so
/// the two call sites stay in sync. `statusCheckRollup` and
/// `reviewDecision`/`latestReviews` are the new additions; all are absent on older gh
/// versions, but `serde(default)` on `RawPr` keeps the parse alive in
/// that case (the chip just won't render the new icons/details).
const PR_JSON_FIELDS: &str = "number,title,headRefName,headRepositoryOwner,isCrossRepository,url,statusCheckRollup,reviewDecision,latestReviews";

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
    #[serde(default, rename = "statusCheckRollup")]
    status_check_rollup: Option<Vec<RawCheckRun>>,
    #[serde(default, rename = "reviewDecision")]
    review_decision: Option<String>,
    #[serde(default, rename = "latestReviews")]
    latest_reviews: Option<Vec<RawReview>>,
}

#[derive(Deserialize)]
struct RawOwner {
    login: String,
}

#[derive(Deserialize, Default)]
struct RawReview {
    #[serde(default)]
    author: Option<RawOwner>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// Subset of gh's check-rollup row we care about. gh emits two shapes
/// here — workflow check-runs (with `name`, `status` + `conclusion`)
/// and status-context rows (with `context` + `state`). All fields are
/// optional so summary/detail computation can fall back gracefully.
#[derive(Deserialize, Default)]
struct RawCheckRun {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "workflowName")]
    workflow_name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default, rename = "detailsUrl")]
    details_url: Option<String>,
    #[serde(default, rename = "targetUrl")]
    target_url: Option<String>,
}

impl RawPr {
    fn into_pr_info(self, repo_slug: &str) -> PrInfo {
        let checks = self.status_check_rollup.as_deref().map(summarize_checks);
        let check_runs = self.status_check_rollup.as_deref().map(check_details).unwrap_or_default();
        let review_details = self.latest_reviews.as_deref().map(review_details).unwrap_or_default();
        // gh returns "" when there's no decision; normalize to `None`
        // so the frontend doesn't have to special-case empty strings.
        let review_decision = self.review_decision.and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });
        PrInfo {
            number: self.number,
            title: self.title,
            head_ref: self.head_ref_name,
            head_owner: self.head_repository_owner.login,
            is_cross_repository: self.is_cross_repository,
            url: self.url,
            repo_slug: repo_slug.to_string(),
            checks,
            check_runs,
            review_decision,
            review_details,
        }
    }
}

/// Collapse gh's `statusCheckRollup` into a single summary chip.
///
/// Order of precedence (worst-of):
///   - failing (any FAILURE / TIMED_OUT / CANCELLED / ERROR)
///   - pending (any QUEUED / IN_PROGRESS / WAITING / PENDING)
///   - passing (every check is SUCCESS / NEUTRAL / SKIPPED)
///   - none (empty rollup)
///
/// gh classifies each row using either `status`+`conclusion` (workflow
/// runs) or `state` (commit-status contexts). We treat unknown values
/// as pending — better to under-promise success than to flash green
/// while a check is still running.
fn summarize_checks(rollup: &[RawCheckRun]) -> PrChecksSummary {
    if rollup.is_empty() {
        return PrChecksSummary {
            state: PrChecksState::None,
            passing: 0,
            failing: 0,
            pending: 0,
            total: 0,
        };
    }
    let mut passing = 0u32;
    let mut failing = 0u32;
    let mut pending = 0u32;
    for row in rollup {
        match classify_check(row) {
            CheckClass::Pass => passing += 1,
            CheckClass::Fail => failing += 1,
            CheckClass::Pending => pending += 1,
        }
    }
    let total = passing + failing + pending;
    let state = if failing > 0 {
        PrChecksState::Failing
    } else if pending > 0 {
        PrChecksState::Pending
    } else if passing > 0 {
        PrChecksState::Passing
    } else {
        PrChecksState::None
    };
    PrChecksSummary { state, passing, failing, pending, total }
}

fn check_details(rollup: &[RawCheckRun]) -> Vec<PrCheckDetails> {
    rollup
        .iter()
        .map(|row| PrCheckDetails {
            name: check_name(row),
            status: check_status(row),
            url: check_url(row),
        })
        .collect()
}

fn check_name(row: &RawCheckRun) -> String {
    row.name
        .as_deref()
        .or(row.context.as_deref())
        .or(row.workflow_name.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Unnamed check")
        .to_string()
}

fn check_url(row: &RawCheckRun) -> Option<String> {
    row.details_url
        .as_deref()
        .or(row.target_url.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn check_status(row: &RawCheckRun) -> PrCheckStatus {
    match classify_check(row) {
        CheckClass::Pass => PrCheckStatus::Passing,
        CheckClass::Fail => PrCheckStatus::Failing,
        CheckClass::Pending => PrCheckStatus::Pending,
    }
}

fn review_details(reviews: &[RawReview]) -> Vec<PrReviewDetails> {
    reviews
        .iter()
        .filter_map(|review| {
            let state = review.state.as_deref().map(str::trim).unwrap_or("");
            if state.is_empty() {
                return None;
            }
            Some(PrReviewDetails {
                reviewer: review
                    .author
                    .as_ref()
                    .map(|a| a.login.trim())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("unknown")
                    .to_string(),
                state: state.to_string(),
                url: review
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned),
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum CheckClass {
    Pass,
    Fail,
    Pending,
}

fn classify_check(row: &RawCheckRun) -> CheckClass {
    // Workflow runs use status + conclusion. A row is settled only when
    // status == COMPLETED; while pending the conclusion is empty.
    if let Some(status) = row.status.as_deref() {
        match status.to_ascii_uppercase().as_str() {
            "COMPLETED" => {
                match row.conclusion.as_deref().map(|s| s.to_ascii_uppercase()).as_deref() {
                    Some("SUCCESS") | Some("NEUTRAL") | Some("SKIPPED") => CheckClass::Pass,
                    Some("FAILURE")
                    | Some("TIMED_OUT")
                    | Some("CANCELLED")
                    | Some("ACTION_REQUIRED")
                    | Some("STARTUP_FAILURE") => CheckClass::Fail,
                    _ => CheckClass::Pending,
                }
            }
            // QUEUED / IN_PROGRESS / WAITING / PENDING / REQUESTED, etc.
            _ => CheckClass::Pending,
        }
    } else if let Some(state) = row.state.as_deref() {
        // Commit-status contexts: state is the conclusion directly.
        match state.to_ascii_uppercase().as_str() {
            "SUCCESS" => CheckClass::Pass,
            "FAILURE" | "ERROR" => CheckClass::Fail,
            _ => CheckClass::Pending,
        }
    } else {
        CheckClass::Pending
    }
}

/// Look up the open PR whose head branch matches `branch` in the repo at
/// `repo_path`. Returns `Ok(None)` when no such PR exists (the empty case
/// is normal — not every branch has a PR yet). Cross-repo PRs whose local
/// branch was renamed by `fetch_pr_branch` to `pr-<N>` are recognized via
/// the `pr-<N>` shape and resolved through `lookup_pr` against the repo's
/// own slug, since `gh pr list --head` does not accept `<owner>:<branch>`
/// syntax (verified via `gh pr list --help`).
pub(crate) async fn lookup_pr_for_branch(repo_path: &str, branch: &str) -> Result<Option<PrInfo>> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let repo_slug = resolve_repo_slug(repo_path).await?;

    if let Some(num_str) = trimmed.strip_prefix("pr-") {
        if let Ok(num) = num_str.parse::<u32>() {
            // Cross-repo PR fetched via fetch_pr_branch — we know the repo
            // slug from `gh repo view`, so resolve directly.
            let shortform = format!("{}#{}", repo_slug, num);
            return match lookup_pr(Some(repo_path), &shortform).await {
                Ok(info) => Ok(Some(info)),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("PR not found") || msg.contains("not found") {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            };
        }
    }

    if let Some(info) = gh_pr_list_by_head(repo_path, trimmed, &repo_slug).await? {
        return Ok(Some(info));
    }

    // Fork fallback: `gh pr list --head <branch>` only matches PRs whose
    // head branch lives in this repo. A PR opened from a fork with the
    // same branch name is invisible to that query, so fall back to GitHub
    // Search syntax. `head:<branch> repo:<slug>` finds the fork PR by
    // matching the head ref name across forks.
    gh_pr_search_by_head(repo_path, trimmed, &repo_slug).await
}

async fn gh_pr_list_by_head(
    repo_path: &str,
    branch: &str,
    repo_slug: &str,
) -> Result<Option<PrInfo>> {
    let stdout = gh_cli()
        .pr_list_by_head(branch, PR_JSON_FIELDS, Path::new(repo_path))
        .await
        .map_err(gh_to_anyhow_pr)?;
    let raws: Vec<RawPr> =
        serde_json::from_str(&stdout).map_err(|e| anyhow!("Failed to parse gh output: {}", e))?;
    Ok(raws.into_iter().next().map(|r| r.into_pr_info(repo_slug)))
}

async fn gh_pr_search_by_head(
    repo_path: &str,
    branch: &str,
    repo_slug: &str,
) -> Result<Option<PrInfo>> {
    // GitHub's search index is eventually consistent — freshly-opened PRs
    // can take a few seconds to show up here. That's fine for our use:
    // by the time the user is back in the app, the index has caught up.
    match gh_cli().pr_search_by_head(branch, repo_slug, PR_JSON_FIELDS, Path::new(repo_path)).await
    {
        Ok(stdout) => {
            let raws: Vec<RawPr> = serde_json::from_str(&stdout)
                .map_err(|e| anyhow!("Failed to parse gh output: {}", e))?;
            Ok(raws.into_iter().next().map(|r| r.into_pr_info(repo_slug)))
        }
        // Auth failures should bubble up — the user needs to know they're
        // logged out. Anything else is non-fatal: we already returned None
        // from the primary query, so let the status bar stay quiet.
        Err(e @ GhError::NotAuthenticated) => Err(gh_to_anyhow(e)),
        Err(_) => Ok(None),
    }
}

/// Resolve `<owner>/<repo>` for the repo at `repo_path` using `gh repo view`.
/// gh reads the repo from cwd's git remotes, so this is a per-path call.
async fn resolve_repo_slug(repo_path: &str) -> Result<String> {
    let stdout =
        gh_cli().repo_view_name_with_owner(Path::new(repo_path)).await.map_err(gh_to_anyhow)?;
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }
    let raw: Raw = serde_json::from_str(&stdout)
        .map_err(|e| anyhow!("Failed to parse gh repo view output: {}", e))?;
    Ok(raw.name_with_owner)
}

/// Map a `GhError` into an anyhow error, rewording the generic
/// `NotFound` to `"PR not found"` so the existing string-matching in
/// `lookup_pr_for_branch` (which downgrades that case to `Ok(None)`)
/// still works.
fn gh_to_anyhow_pr(err: GhError) -> anyhow::Error {
    match err {
        GhError::NotFound => anyhow!("PR not found"),
        other => anyhow!(other),
    }
}

/// Map a `GhError` from a non-PR operation. Reports the underlying gh
/// error verbatim — no PR-specific wording.
fn gh_to_anyhow(err: GhError) -> anyhow::Error {
    anyhow!(err)
}

/// Fetch the PR's head commit into a local branch in `repo_path`, without
/// moving HEAD. Uses `refs/pull/<N>/head` which GitHub exposes for every PR
/// (same-repo and fork alike). Returns the local branch name.
///
/// For same-repo PRs the target branch name matches the PR's head ref, so
/// existing worktree detection finds the branch naturally. For cross-repo
/// PRs the target is `pr-<N>` — the fork's branch name isn't guaranteed to
/// be a valid or meaningful local ref.
pub(crate) async fn fetch_pr_branch(
    repo_path: &str,
    number: u32,
    head_ref: &str,
    is_cross_repository: bool,
) -> Result<String> {
    let local_branch =
        if is_cross_repository { format!("pr-{}", number) } else { head_ref.to_string() };
    let refspec = format!("+refs/pull/{}/head:refs/heads/{}", number, local_branch);
    let git = crate::services::setup::git_cli().into_async();
    git.fetch_refspec(Path::new(repo_path), &refspec)
        .await
        .map_err(|e| anyhow!("Failed to fetch PR branch: {}", e))?;
    Ok(local_branch)
}

/// Clone `owner/repo` from GitHub into `target_dir` using `gh repo clone`.
/// The caller is responsible for ensuring `target_dir`'s parent exists and
/// `target_dir` itself does not already exist. Returns `target_dir` on
/// success (the caller already knows the path, but returning it keeps the
/// command symmetrical with fetch_pr_branch).
pub(crate) async fn clone_repo(owner: &str, repo: &str, target_dir: &str) -> Result<String> {
    let target = std::path::PathBuf::from(target_dir);
    if target.exists() {
        return Err(anyhow!("Target path already exists: {}", target_dir));
    }
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| anyhow!("Failed to create parent dir: {}", e))?;
        }
    }
    let slug = format!("{}/{}", owner, repo);
    gh_cli().repo_clone(&slug, &target).await.map_err(|e| match e {
        GhError::NotFound => anyhow!("Repository not found: {slug}"),
        other => anyhow!(other),
    })?;
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

    fn workflow_run(conclusion: &str) -> RawCheckRun {
        RawCheckRun {
            name: Some("workflow".into()),
            status: Some("COMPLETED".into()),
            conclusion: Some(conclusion.into()),
            ..Default::default()
        }
    }

    fn workflow_pending() -> RawCheckRun {
        RawCheckRun {
            name: Some("workflow".into()),
            status: Some("IN_PROGRESS".into()),
            ..Default::default()
        }
    }

    fn status_context(state: &str) -> RawCheckRun {
        RawCheckRun {
            context: Some("context".into()),
            state: Some(state.into()),
            ..Default::default()
        }
    }

    #[test]
    fn summarize_checks_empty_is_none() {
        let s = summarize_checks(&[]);
        assert!(matches!(s.state, PrChecksState::None));
        assert_eq!(s.total, 0);
    }

    #[test]
    fn summarize_checks_failing_wins_over_pending_and_passing() {
        let s = summarize_checks(&[
            workflow_run("SUCCESS"),
            workflow_pending(),
            workflow_run("FAILURE"),
        ]);
        assert!(matches!(s.state, PrChecksState::Failing));
        assert_eq!(s.passing, 1);
        assert_eq!(s.failing, 1);
        assert_eq!(s.pending, 1);
        assert_eq!(s.total, 3);
    }

    #[test]
    fn summarize_checks_pending_wins_over_passing() {
        let s = summarize_checks(&[workflow_run("SUCCESS"), workflow_pending()]);
        assert!(matches!(s.state, PrChecksState::Pending));
    }

    #[test]
    fn summarize_checks_neutral_and_skipped_count_as_passing() {
        let s = summarize_checks(&[workflow_run("NEUTRAL"), workflow_run("SKIPPED")]);
        assert!(matches!(s.state, PrChecksState::Passing));
        assert_eq!(s.passing, 2);
    }

    #[test]
    fn summarize_checks_supports_status_context_state_field() {
        let s = summarize_checks(&[status_context("SUCCESS"), status_context("FAILURE")]);
        assert!(matches!(s.state, PrChecksState::Failing));
        assert_eq!(s.passing, 1);
        assert_eq!(s.failing, 1);
    }

    #[test]
    fn check_details_normalizes_names_statuses_and_urls() {
        let rows = [
            RawCheckRun {
                name: Some("cargo test".into()),
                status: Some("COMPLETED".into()),
                conclusion: Some("SUCCESS".into()),
                details_url: Some("https://example.test/checks/1".into()),
                ..Default::default()
            },
            RawCheckRun {
                context: Some("lint".into()),
                state: Some("FAILURE".into()),
                target_url: Some("https://example.test/checks/2".into()),
                ..Default::default()
            },
            RawCheckRun {
                workflow_name: Some("preview".into()),
                status: Some("IN_PROGRESS".into()),
                ..Default::default()
            },
        ];

        let details = check_details(&rows);

        assert_eq!(details[0].name, "cargo test");
        assert!(matches!(details[0].status, PrCheckStatus::Passing));
        assert_eq!(details[0].url.as_deref(), Some("https://example.test/checks/1"));
        assert_eq!(details[1].name, "lint");
        assert!(matches!(details[1].status, PrCheckStatus::Failing));
        assert_eq!(details[1].url.as_deref(), Some("https://example.test/checks/2"));
        assert_eq!(details[2].name, "preview");
        assert!(matches!(details[2].status, PrCheckStatus::Pending));
        assert!(details[2].url.is_none());
    }

    #[test]
    fn review_details_normalizes_latest_reviews() {
        let rows = [
            RawReview {
                author: Some(RawOwner { login: "alice".into() }),
                state: Some("APPROVED".into()),
                url: Some("https://example.test/reviews/1".into()),
            },
            RawReview {
                author: Some(RawOwner { login: " ".into() }),
                state: Some("CHANGES_REQUESTED".into()),
                url: Some(" ".into()),
            },
            RawReview {
                author: Some(RawOwner { login: "ignored".into() }),
                state: Some(" ".into()),
                url: None,
            },
        ];

        let details = review_details(&rows);

        assert_eq!(details.len(), 2);
        assert_eq!(details[0].reviewer, "alice");
        assert_eq!(details[0].state, "APPROVED");
        assert_eq!(details[0].url.as_deref(), Some("https://example.test/reviews/1"));
        assert_eq!(details[1].reviewer, "unknown");
        assert_eq!(details[1].state, "CHANGES_REQUESTED");
        assert!(details[1].url.is_none());
    }
}
