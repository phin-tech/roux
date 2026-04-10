use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use tauri_plugin_notification::NotificationExt;
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

// Re-export core types
pub use roux_core::{
    CreateWatchConfig, GithubJob, NotifyConfig, PrCheckRun, PrReview, RuntimeState, Watch,
    WatchKind, WatchMode, WatchOutcome, WatchResult, WatchScope, WatchUpdateEvent,
};

/// Tracks recent outcome transitions for flap debouncing.
pub struct FlapTracker {
    last_outcomes: Vec<(WatchOutcome, u64)>,
}

impl FlapTracker {
    pub fn new() -> Self {
        Self { last_outcomes: Vec::new() }
    }

    pub fn record(&mut self, outcome: WatchOutcome, now_ms: u64) {
        self.last_outcomes.push((outcome, now_ms));
        if self.last_outcomes.len() > 3 {
            self.last_outcomes.remove(0);
        }
    }

    pub fn is_flapping(&self) -> bool {
        if self.last_outcomes.len() < 3 {
            return false;
        }
        let recent = &self.last_outcomes;
        let window = recent.last().unwrap().1 - recent[recent.len() - 3].1;
        if window > 60_000 {
            return false;
        }
        let last = &recent[recent.len() - 1].0;
        let prev = &recent[recent.len() - 2].0;
        if last == prev {
            return false;
        }
        true
    }
}

// ── Persistence ─────────────────────────────────────────────

pub struct WatchStore {
    watches: Arc<Mutex<Vec<Watch>>>,
    dirty: Arc<AtomicBool>,
}

impl WatchStore {
    pub fn load_persisted() -> Self {
        let path = Self::persistence_path();
        let watches = if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            let mut watches: Vec<Watch> = serde_json::from_str(&content).unwrap_or_default();
            for w in &mut watches {
                match &w.runtime_state {
                    RuntimeState::Stopped | RuntimeState::Paused => {}
                    _ => w.runtime_state = RuntimeState::Pending,
                }
            }
            watches
        } else {
            Vec::new()
        };
        let store = Self {
            watches: Arc::new(Mutex::new(watches)),
            dirty: Arc::new(AtomicBool::new(false)),
        };
        store.start_persist_thread();
        store
    }

    fn start_persist_thread(&self) {
        let watches = Arc::clone(&self.watches);
        let dirty = Arc::clone(&self.dirty);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if dirty.swap(false, Ordering::AcqRel) {
                let snapshot = {
                    let guard = watches.lock().unwrap();
                    guard.clone()
                };
                Self::write_to_disk(&snapshot);
            }
        });
    }

    pub fn add(&self, watch: Watch) {
        let mut watches = self.watches.lock().unwrap();
        watches.push(watch);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn remove(&self, id: &str) {
        let mut watches = self.watches.lock().unwrap();
        watches.retain(|w| w.id != id);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn get(&self, id: &str) -> Option<Watch> {
        self.watches.lock().unwrap().iter().find(|w| w.id == id).cloned()
    }

    pub fn list(&self) -> Vec<Watch> {
        self.watches.lock().unwrap().clone()
    }

    pub fn update(&self, id: &str, f: impl FnOnce(&mut Watch)) {
        let mut watches = self.watches.lock().unwrap();
        if let Some(w) = watches.iter_mut().find(|w| w.id == id) {
            f(w);
        }
        self.dirty.store(true, Ordering::Release);
    }

    pub fn cleanup_orphans(&self, session_ids: &[String], project_ids: &[String]) {
        let mut watches = self.watches.lock().unwrap();
        let before = watches.len();
        watches.retain(|w| match &w.scope {
            WatchScope::Global => true,
            WatchScope::Session { session_id } => session_ids.contains(session_id),
            WatchScope::Project { project_id } => project_ids.contains(project_id),
        });
        if watches.len() != before {
            self.dirty.store(true, Ordering::Release);
        }
    }

    fn persistence_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("roux").join("watches.json")
    }

    fn write_to_disk(watches: &[Watch]) {
        let path = Self::persistence_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(watches) {
            let _ = fs::write(&path, json);
        }
    }
}

// ── WatchManager ───────────────────────────────────────────

const MAX_OUTPUT_BYTES: usize = 64 * 1024; // 64KB

fn truncate_output(s: String) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    // Find a valid UTF-8 boundary at or before MAX_OUTPUT_BYTES
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[allow(dead_code)]
pub struct WatchHandle {
    pub cancel: CancellationToken,
    pub join: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
pub struct WatchManager {
    store: Arc<WatchStore>,
    handles: Arc<Mutex<HashMap<String, WatchHandle>>>,
    flap_trackers: Arc<Mutex<HashMap<String, FlapTracker>>>,
}

impl WatchManager {
    pub fn new(store: Arc<WatchStore>) -> Self {
        Self {
            store,
            handles: Arc::new(Mutex::new(HashMap::new())),
            flap_trackers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn store(&self) -> &Arc<WatchStore> {
        &self.store
    }

    pub fn start_all(&self, app: tauri::AppHandle) {
        let watches = self.store.list();
        for (i, watch) in watches.iter().enumerate() {
            if matches!(watch.runtime_state, RuntimeState::Stopped | RuntimeState::Paused) {
                continue;
            }
            let jitter = Duration::from_millis((i as u64) * 500 + rand_jitter());
            self.spawn_watch(watch.id.clone(), Some(jitter), app.clone());
        }
    }

    pub fn create_watch(&self, mut watch: Watch, app: tauri::AppHandle) -> Watch {
        watch.runtime_state = RuntimeState::Active;
        self.store.add(watch.clone());
        self.spawn_watch(watch.id.clone(), None, app);
        watch
    }

    pub fn remove_watch(&self, id: &str) {
        self.cancel_watch(id);
        self.store.remove(id);
    }

    pub fn pause_watch(&self, id: &str, app: &tauri::AppHandle) {
        self.cancel_watch(id);
        self.store.update(id, |w| {
            w.runtime_state = RuntimeState::Paused;
        });
        self.emit_watch_update(id, app);
    }

    pub fn resume_watch(&self, id: &str, app: tauri::AppHandle) {
        self.store.update(id, |w| {
            w.runtime_state = RuntimeState::Active;
        });
        self.emit_watch_update(id, &app);
        self.spawn_watch(id.to_string(), None, app);
    }

    fn emit_watch_update(&self, id: &str, app: &tauri::AppHandle) {
        if let Some(watch) = self.store.get(id) {
            let event = WatchUpdateEvent { watch, changed: false, previous_outcome: None };
            let _ = app.emit("watch-update", &event);
        }
    }

    fn cancel_watch(&self, id: &str) {
        let mut handles = self.handles.lock().unwrap();
        if let Some(handle) = handles.remove(id) {
            handle.cancel.cancel();
        }
    }

    fn spawn_watch(
        &self,
        watch_id: String,
        initial_delay: Option<Duration>,
        app: tauri::AppHandle,
    ) {
        // Cancel any existing task for this watch before spawning a new one
        self.cancel_watch(&watch_id);

        let store = Arc::clone(&self.store);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handles = Arc::clone(&self.handles);
        let flap_trackers = Arc::clone(&self.flap_trackers);
        let watch_id_for_handles = watch_id.clone();
        let watch_id_for_cleanup = watch_id.clone();
        let handles_for_cleanup = Arc::clone(&self.handles);

        let join = tokio::spawn(async move {
            if let Some(delay) = initial_delay {
                tokio::select! {
                    _ = sleep(delay) => {}
                    _ = cancel_clone.cancelled() => return,
                }
            }

            loop {
                let watch = match store.get(&watch_id) {
                    Some(w) => w,
                    None => break,
                };

                let previous_outcome = watch.last_result.as_ref().map(|r| r.outcome().clone());

                let check_timeout = match &watch.kind {
                    WatchKind::HttpHealth { .. } => Duration::from_secs(10),
                    _ => Duration::from_secs(30),
                };

                // Use select! with timeout to support cancellation.
                // Return kind-appropriate failure on timeout.
                let result = tokio::select! {
                    r = timeout(check_timeout, execute_check(&watch.kind)) => {
                        match r {
                            Ok(result) => result,
                            Err(_) => timeout_result(&watch.kind),
                        }
                    }
                    _ = cancel_clone.cancelled() => break,
                };

                let new_outcome = result.outcome().clone();
                let changed = previous_outcome.as_ref() != Some(&new_outcome);

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                store.update(&watch_id, |w| {
                    w.last_result = Some(result.clone());
                    w.last_checked = Some(now);
                    w.runtime_state = RuntimeState::Active;
                });

                if let Some(updated_watch) = store.get(&watch_id) {
                    let event =
                        WatchUpdateEvent { watch: updated_watch, changed, previous_outcome };
                    let _ = app.emit("watch-update", &event);
                }

                // Send desktop notification if configured (with flap debouncing)
                if changed {
                    if let Some(ref updated) = store.get(&watch_id) {
                        let outcome = updated.last_result.as_ref().map(|r| r.outcome());

                        // Update flap tracker and check if flapping
                        let suppress = {
                            let mut trackers = flap_trackers.lock().unwrap();
                            let tracker =
                                trackers.entry(watch_id.clone()).or_insert_with(FlapTracker::new);
                            if let Some(ref o) = outcome {
                                tracker.record((*o).clone(), now);
                            }
                            tracker.is_flapping()
                        };

                        let should_notify = !suppress
                            && match outcome {
                                Some(WatchOutcome::Failure) => {
                                    updated.notify.desktop_notification && updated.notify.on_failure
                                }
                                Some(WatchOutcome::Success) => {
                                    updated.notify.desktop_notification && updated.notify.on_success
                                }
                                _ => false,
                            };
                        if should_notify {
                            let title = match outcome {
                                Some(WatchOutcome::Failure) => format!("❌ {}", updated.name),
                                Some(WatchOutcome::Success) => format!("✅ {}", updated.name),
                                _ => updated.name.clone(),
                            };
                            let body = match &updated.last_result {
                                Some(WatchResult::GithubRun { conclusion, url, .. }) => {
                                    format!(
                                        "{} — {}",
                                        conclusion.as_deref().unwrap_or("unknown"),
                                        url
                                    )
                                }
                                Some(WatchResult::HttpCheck {
                                    status_code,
                                    response_time_ms,
                                    ..
                                }) => {
                                    format!("HTTP {} ({}ms)", status_code, response_time_ms)
                                }
                                Some(WatchResult::CommandRun { exit_code, .. }) => {
                                    format!("Exit code: {}", exit_code)
                                }
                                Some(WatchResult::GithubPr { state, checks, reviews, .. }) => {
                                    let passed = checks
                                        .iter()
                                        .filter(|c| c.conclusion.as_deref() == Some("success"))
                                        .count();
                                    let approvals =
                                        reviews.iter().filter(|r| r.state == "approved").count();
                                    format!(
                                        "{} — {}/{} checks passed, {} approval(s)",
                                        state,
                                        passed,
                                        checks.len(),
                                        approvals
                                    )
                                }
                                None => String::new(),
                            };
                            let _ = app.notification().builder().title(&title).body(&body).show();
                        }
                    }
                }

                // Auto-stop: one-shot watches, completed GitHub runs, or merged/closed PRs
                let should_stop = matches!(watch.mode, WatchMode::OneShot)
                    || matches!(
                        (&watch.kind, &new_outcome),
                        (
                            WatchKind::GithubAction { .. },
                            WatchOutcome::Success | WatchOutcome::Failure
                        )
                    )
                    || matches!(
                        (&watch.kind, &result),
                        (WatchKind::GithubPr { .. }, WatchResult::GithubPr { ref state, .. })
                        if state == "merged" || state == "closed"
                    );
                if should_stop {
                    store.update(&watch_id, |w| {
                        w.runtime_state = RuntimeState::Stopped;
                    });
                    break;
                }

                let interval = match &watch.mode {
                    WatchMode::Recurring { interval_secs } => Duration::from_secs(*interval_secs),
                    WatchMode::OneShot => break,
                };

                tokio::select! {
                    _ = sleep(interval) => {}
                    _ = cancel_clone.cancelled() => break,
                }
            }

            // Clean up handle entry when task exits
            let mut handles_guard = handles_for_cleanup.lock().unwrap();
            handles_guard.remove(&watch_id_for_cleanup);
        });

        let mut handles_guard = handles.lock().unwrap();
        handles_guard.insert(watch_id_for_handles, WatchHandle { cancel, join });
    }
}

fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 5000) as u64
}

fn timeout_result(kind: &WatchKind) -> WatchResult {
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

async fn execute_check(kind: &WatchKind) -> WatchResult {
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

// ── GitHub API (octocrab) ──────────────────────────────────

/// Try to get a GitHub token from `gh auth token`, cached for the process lifetime.
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
        // List the most recent run
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

    // Get the run details
    let run = crab.workflows(owner, repo_name).get(target_run_id.into()).await;

    let run = match run {
        Ok(r) => r,
        Err(e) => return github_error_result(format!("GitHub API error: {}", e)),
    };

    let status = run.status;
    let conclusion = run.conclusion;
    let url = run.html_url.to_string();

    // Get jobs for the run
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

    // 1. Get PR details
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

    // 2. Get reviews (deduplicate to latest per reviewer)
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
                // Only keep actionable states; skip "commented"/"pending" if an actionable review exists
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

    // 3. Get check runs for the head commit
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

    // 4. Compute outcome
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

// ── Tauri Commands ──────────────────────────────────────────

use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn cmd_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Watch, String> {
    let watch = Watch {
        id: uuid::Uuid::new_v4().to_string(),
        name: config.name,
        kind: config.kind,
        mode: config.mode,
        scope: config.scope,
        runtime_state: RuntimeState::Pending,
        last_result: None,
        last_checked: None,
        notify: config.notify.unwrap_or_default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    };
    Ok(state.watch_manager.create_watch(watch, app))
}

#[tauri::command]
#[specta::specta]
pub fn cmd_remove_watch(id: String, state: tauri::State<AppState>) {
    state.watch_manager.remove_watch(&id);
}

#[tauri::command]
#[specta::specta]
pub fn cmd_list_watches(state: tauri::State<AppState>) -> Vec<Watch> {
    state.watch_manager.store().list()
}

#[tauri::command]
#[specta::specta]
pub fn cmd_pause_watch(id: String, state: tauri::State<AppState>, app: tauri::AppHandle) {
    state.watch_manager.pause_watch(&id, &app);
}

#[tauri::command]
#[specta::specta]
pub async fn cmd_resume_watch(
    id: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.watch_manager.resume_watch(&id, app);
    Ok(())
}

async fn execute_shell_check(
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
