use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command as TokioCommand;

use roux_core::{GithubJob, PrCheckRun, PrReview, WatchKind, WatchOutcome, WatchResult};

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

fn truncate_output(s: String) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn timeout_result(kind: &WatchKind) -> WatchResult {
    match kind {
        WatchKind::HttpHealth { .. } => WatchResult::HttpCheck {
            status_code: 0,
            response_time_ms: 0,
            outcome: WatchOutcome::Failure,
        },
        WatchKind::GithubAction { repo, run_id, .. } => WatchResult::GithubRun {
            run_id: run_id.unwrap_or(0),
            status: "timeout".into(),
            conclusion: None,
            url: format!("https://github.com/{}", repo),
            jobs: vec![],
            outcome: WatchOutcome::Failure,
        },
        WatchKind::GithubPr { repo, pr_number } => WatchResult::GithubPr {
            pr_number: *pr_number,
            state: "unknown".into(),
            title: String::new(),
            url: format!("https://github.com/{}/pull/{}", repo, pr_number),
            head_sha: String::new(),
            draft: false,
            reviews: vec![],
            checks: vec![],
            outcome: WatchOutcome::Failure,
        },
        _ => WatchResult::CommandRun {
            exit_code: -1,
            stdout: String::new(),
            stderr: "(timed out)".to_string(),
            outcome: WatchOutcome::Failure,
        },
    }
}

pub async fn execute_check(kind: &WatchKind) -> WatchResult {
    match kind {
        WatchKind::GithubAction { repo, run_id, workflow, branch } => {
            execute_github_check(repo, *run_id, workflow.as_deref(), branch.as_deref()).await
        }
        WatchKind::HttpHealth { url, expected_status } => {
            execute_http_check(url, *expected_status).await
        }
        WatchKind::ShellCommand { command, working_dir, success_exit_code } => {
            execute_shell_check(command, working_dir.as_deref(), *success_exit_code).await
        }
        WatchKind::Task { command, working_dir, .. } => {
            execute_shell_check(command, Some(working_dir.as_str()), 0).await
        }
        WatchKind::GithubPr { repo, pr_number } => execute_github_pr_check(repo, *pr_number).await,
    }
}

// ── GitHub API ──────────────────────────────────────────────

fn github_token() -> Option<&'static str> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            let user_path = crate::pty::get_user_path();
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
            std::process::Command::new(&shell)
                .args(["-c", "gh auth token"])
                .env("PATH", &user_path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|t| !t.is_empty())
        })
        .as_deref()
}

fn build_octocrab() -> octocrab::Octocrab {
    match github_token() {
        Some(token) => octocrab::Octocrab::builder()
            .personal_token(token.to_string())
            .build()
            .unwrap_or_else(|_| octocrab::Octocrab::default()),
        None => octocrab::Octocrab::default(),
    }
}

fn github_error_result(msg: String) -> WatchResult {
    WatchResult::CommandRun {
        exit_code: -1,
        stdout: String::new(),
        stderr: msg,
        outcome: WatchOutcome::Failure,
    }
}

async fn execute_github_check(
    repo: &str,
    run_id: Option<u64>,
    _workflow: Option<&str>,
    _branch: Option<&str>,
) -> WatchResult {
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return github_error_result(format!("Invalid repo format: {}", repo));
    }
    let (owner, repo_name) = (parts[0], parts[1]);
    let crab = build_octocrab();

    let target_run_id = if let Some(id) = run_id {
        id
    } else {
        let page = crab.workflows(owner, repo_name).list_all_runs().per_page(1).send().await;
        match page {
            Ok(page) => match page.items.first() {
                Some(run) => run.id.0,
                None => {
                    return WatchResult::GithubRun {
                        run_id: 0,
                        status: "unknown".into(),
                        conclusion: None,
                        url: String::new(),
                        jobs: vec![],
                        outcome: WatchOutcome::Failure,
                    }
                }
            },
            Err(e) => return github_error_result(format!("GitHub API error: {}", e)),
        }
    };

    let run = match crab.workflows(owner, repo_name).get(target_run_id.into()).await {
        Ok(r) => r,
        Err(e) => return github_error_result(format!("GitHub API error: {}", e)),
    };

    let status = run.status;
    let conclusion = run.conclusion;
    let url = run.html_url.to_string();

    let jobs_result = crab.workflows(owner, repo_name).list_jobs(target_run_id.into()).send().await;

    let jobs: Vec<GithubJob> = match jobs_result {
        Ok(jobs_page) => jobs_page
            .items
            .iter()
            .map(|j: &octocrab::models::workflows::Job| {
                let job_conclusion = j.conclusion.as_ref().map(|c| {
                    serde_json::to_value(c)
                        .ok()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default()
                });
                let is_failure = job_conclusion.as_deref() == Some("failure");
                let failed_step = if is_failure {
                    j.steps
                        .iter()
                        .find(|s| {
                            s.conclusion
                                .as_ref()
                                .and_then(|c| serde_json::to_value(c).ok())
                                .and_then(|v| v.as_str().map(|s| s == "failure"))
                                .unwrap_or(false)
                        })
                        .map(|s| s.name.clone())
                } else {
                    None
                };
                let job_status = serde_json::to_value(&j.status)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                GithubJob {
                    name: j.name.clone(),
                    status: job_status,
                    conclusion: job_conclusion,
                    failed_step,
                }
            })
            .collect(),
        Err(_) => vec![],
    };

    let outcome = match (status.as_str(), conclusion.as_deref()) {
        ("completed", Some("success")) => WatchOutcome::Success,
        ("completed", _) => WatchOutcome::Failure,
        _ => WatchOutcome::InProgress,
    };

    WatchResult::GithubRun { run_id: target_run_id, status, conclusion, url, jobs, outcome }
}

async fn execute_github_pr_check(repo: &str, pr_number: u64) -> WatchResult {
    let parts: Vec<&str> = repo.splitn(2, '/').collect();
    if parts.len() != 2 {
        return github_error_result(format!("Invalid repo format: {}", repo));
    }
    let (owner, repo_name) = (parts[0], parts[1]);
    let crab = build_octocrab();

    let pr = match crab.pulls(owner, repo_name).get(pr_number).await {
        Ok(pr) => pr,
        Err(e) => return github_error_result(format!("GitHub API error: {}", e)),
    };

    let state = if pr.merged.unwrap_or(false) {
        "merged".to_string()
    } else {
        match pr.state {
            Some(octocrab::models::IssueState::Open) => "open".to_string(),
            Some(octocrab::models::IssueState::Closed) => "closed".to_string(),
            _ => "unknown".to_string(),
        }
    };
    let title = pr.title.unwrap_or_default();
    let url = pr.html_url.map(|u| u.to_string()).unwrap_or_default();
    let head_sha = pr.head.sha.clone();
    let draft = pr.draft.unwrap_or(false);

    let reviews_result =
        crab.pulls(owner, repo_name).list_reviews(pr_number).per_page(100).send().await;

    let reviews: Vec<PrReview> = match reviews_result {
        Ok(page) => {
            let mut latest: std::collections::HashMap<String, PrReview> =
                std::collections::HashMap::new();
            for review in &page.items {
                let reviewer = review
                    .user
                    .as_ref()
                    .map(|u| u.login.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let state_str = match review.state {
                    Some(octocrab::models::pulls::ReviewState::Approved) => "approved",
                    Some(octocrab::models::pulls::ReviewState::ChangesRequested) => {
                        "changes_requested"
                    }
                    Some(octocrab::models::pulls::ReviewState::Commented) => "commented",
                    Some(octocrab::models::pulls::ReviewState::Dismissed) => "dismissed",
                    Some(octocrab::models::pulls::ReviewState::Pending) => "pending",
                    _ => "unknown",
                };
                let review_url = Some(review.html_url.to_string());
                if state_str == "commented" || state_str == "pending" {
                    if !latest.contains_key(&reviewer) {
                        latest.insert(
                            reviewer.clone(),
                            PrReview { reviewer, state: state_str.to_string(), url: review_url },
                        );
                    }
                    continue;
                }
                latest.insert(
                    reviewer.clone(),
                    PrReview { reviewer, state: state_str.to_string(), url: review_url },
                );
            }
            latest.into_values().collect()
        }
        Err(_) => vec![],
    };

    let checks_result = crab
        .checks(owner, repo_name)
        .list_check_runs_for_git_ref(octocrab::params::repos::Commitish(head_sha.clone()))
        .per_page(100)
        .send()
        .await;

    let checks: Vec<PrCheckRun> = match checks_result {
        Ok(list) => list
            .check_runs
            .iter()
            .map(|cr| PrCheckRun {
                name: cr.name.clone(),
                conclusion: cr.conclusion.clone(),
                url: cr.html_url.clone(),
            })
            .collect(),
        Err(_) => vec![],
    };

    let outcome = compute_pr_outcome(&state, &reviews, &checks);

    WatchResult::GithubPr {
        pr_number,
        state,
        title,
        url,
        head_sha,
        draft,
        reviews,
        checks,
        outcome,
    }
}

fn compute_pr_outcome(state: &str, reviews: &[PrReview], checks: &[PrCheckRun]) -> WatchOutcome {
    if state == "merged" {
        return WatchOutcome::Success;
    }
    if state == "closed" {
        return WatchOutcome::Failure;
    }

    let any_check_running = checks.iter().any(|c| c.conclusion.is_none());
    let any_check_failed = checks.iter().any(|c| c.conclusion.as_deref() == Some("failure"));
    let changes_requested = reviews.iter().any(|r| r.state == "changes_requested");
    let has_approval = reviews.iter().any(|r| r.state == "approved");

    if any_check_failed || changes_requested {
        WatchOutcome::Failure
    } else if any_check_running || checks.is_empty() {
        WatchOutcome::InProgress
    } else if has_approval {
        WatchOutcome::Success
    } else {
        WatchOutcome::InProgress
    }
}

// ── HTTP check ──────────────────────────────────────────────

async fn execute_http_check(url: &str, expected_status: u16) -> WatchResult {
    let client =
        reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap_or_default();
    let start = std::time::Instant::now();
    match client.get(url).send().await {
        Ok(resp) => {
            let status_code = resp.status().as_u16();
            let response_time_ms = start.elapsed().as_millis() as u64;
            let outcome = if status_code == expected_status {
                WatchOutcome::Success
            } else {
                WatchOutcome::Failure
            };
            WatchResult::HttpCheck { status_code, response_time_ms, outcome }
        }
        Err(_e) => WatchResult::HttpCheck {
            status_code: 0,
            response_time_ms: start.elapsed().as_millis() as u64,
            outcome: WatchOutcome::Failure,
        },
    }
}

// ── Shell check ─────────────────────────────────────────────

pub async fn execute_shell_check(
    command: &str,
    working_dir: Option<&str>,
    success_exit_code: i32,
) -> WatchResult {
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
    let mut cmd = TokioCommand::new(shell);
    cmd.arg(flag).arg(command);
    cmd.kill_on_drop(true);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    match cmd.output().await {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = truncate_output(String::from_utf8_lossy(&output.stdout).to_string());
            let stderr = truncate_output(String::from_utf8_lossy(&output.stderr).to_string());
            let outcome = if exit_code == success_exit_code {
                WatchOutcome::Success
            } else {
                WatchOutcome::Failure
            };
            WatchResult::CommandRun { exit_code, stdout, stderr, outcome }
        }
        Err(e) => WatchResult::CommandRun {
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("Failed to execute: {}", e),
            outcome: WatchOutcome::Failure,
        },
    }
}
