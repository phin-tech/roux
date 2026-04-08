use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use tauri_plugin_notification::NotificationExt;
use tokio::process::Command as TokioCommand;
use tokio::time::{timeout, sleep};
use tokio_util::sync::CancellationToken;

// ── Core Types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Watch {
    pub id: String,
    pub name: String,
    pub kind: WatchKind,
    pub mode: WatchMode,
    pub scope: WatchScope,
    pub runtime_state: RuntimeState,
    pub last_result: Option<WatchResult>,
    pub last_checked: Option<u64>,
    pub notify: NotifyConfig,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchScope {
    Global,
    Session { session_id: String },
    Project { project_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RuntimeState {
    Pending,
    Active,
    Paused,
    Stopped,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchKind {
    GithubAction {
        repo: String,
        run_id: Option<u64>,
        workflow: Option<String>,
        branch: Option<String>,
    },
    HttpHealth {
        url: String,
        expected_status: u16,
    },
    ShellCommand {
        command: String,
        working_dir: Option<String>,
        success_exit_code: i32,
    },
    Task {
        task_id: String,
        command: String,
        working_dir: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchMode {
    Recurring { interval_secs: u64 },
    OneShot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchResult {
    GithubRun {
        run_id: u64,
        status: String,
        conclusion: Option<String>,
        url: String,
        jobs: Vec<GithubJob>,
        outcome: WatchOutcome,
    },
    HttpCheck {
        status_code: u16,
        response_time_ms: u64,
        outcome: WatchOutcome,
    },
    CommandRun {
        exit_code: i32,
        stdout: String,
        stderr: String,
        outcome: WatchOutcome,
    },
}

impl WatchResult {
    pub fn outcome(&self) -> &WatchOutcome {
        match self {
            WatchResult::GithubRun { outcome, .. } => outcome,
            WatchResult::HttpCheck { outcome, .. } => outcome,
            WatchResult::CommandRun { outcome, .. } => outcome,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchOutcome {
    Success,
    Failure,
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubJob {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub failed_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyConfig {
    pub desktop_notification: bool,
    pub on_failure: bool,
    pub on_success: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            desktop_notification: true,
            on_failure: true,
            on_success: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchUpdateEvent {
    pub watch: Watch,
    pub changed: bool,
    pub previous_outcome: Option<WatchOutcome>,
}

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
                    RuntimeState::Stopped => {}
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
            let event = WatchUpdateEvent {
                watch,
                changed: true,
                previous_outcome: None,
            };
            let _ = app.emit("watch-update", &event);
        }
    }

    fn cancel_watch(&self, id: &str) {
        let mut handles = self.handles.lock().unwrap();
        if let Some(handle) = handles.remove(id) {
            handle.cancel.cancel();
        }
    }

    fn spawn_watch(&self, watch_id: String, initial_delay: Option<Duration>, app: tauri::AppHandle) {
        let store = Arc::clone(&self.store);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handles = Arc::clone(&self.handles);
        let flap_trackers = Arc::clone(&self.flap_trackers);
        let watch_id_for_handles = watch_id.clone();

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
                    None => return,
                };

                let previous_outcome = watch.last_result.as_ref().map(|r| r.outcome().clone());

                let check_timeout = match &watch.kind {
                    WatchKind::HttpHealth { .. } => Duration::from_secs(10),
                    _ => Duration::from_secs(30),
                };

                // IMPORTANT: Use select! with timeout to support cancellation
                let result = tokio::select! {
                    r = timeout(check_timeout, execute_check(&watch.kind)) => {
                        match r {
                            Ok(result) => result,
                            Err(_) => WatchResult::CommandRun {
                                exit_code: -1,
                                stdout: String::new(),
                                stderr: "(timed out)".to_string(),
                                outcome: WatchOutcome::Failure,
                            },
                        }
                    }
                    _ = cancel_clone.cancelled() => return,
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
                    let event = WatchUpdateEvent {
                        watch: updated_watch,
                        changed,
                        previous_outcome,
                    };
                    let _ = app.emit("watch-update", &event);
                }

                // Send desktop notification if configured (with flap debouncing)
                if changed {
                    if let Some(ref updated) = store.get(&watch_id) {
                        let outcome = updated.last_result.as_ref().map(|r| r.outcome());

                        // Update flap tracker and check if flapping
                        let suppress = {
                            let mut trackers = flap_trackers.lock().unwrap();
                            let tracker = trackers.entry(watch_id.clone()).or_insert_with(FlapTracker::new);
                            if let Some(ref o) = outcome {
                                tracker.record((*o).clone(), now);
                            }
                            tracker.is_flapping()
                        };

                        let should_notify = !suppress && match outcome {
                            Some(WatchOutcome::Failure) => updated.notify.desktop_notification && updated.notify.on_failure,
                            Some(WatchOutcome::Success) => updated.notify.desktop_notification && updated.notify.on_success,
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
                                    format!("{} — {}", conclusion.as_deref().unwrap_or("unknown"), url)
                                }
                                Some(WatchResult::HttpCheck { status_code, response_time_ms, .. }) => {
                                    format!("HTTP {} ({}ms)", status_code, response_time_ms)
                                }
                                Some(WatchResult::CommandRun { exit_code, .. }) => {
                                    format!("Exit code: {}", exit_code)
                                }
                                None => String::new(),
                            };
                            let _ = app.notification()
                                .builder()
                                .title(&title)
                                .body(&body)
                                .show();
                        }
                    }
                }

                if matches!(watch.mode, WatchMode::OneShot) {
                    store.update(&watch_id, |w| {
                        w.runtime_state = RuntimeState::Stopped;
                    });
                    return;
                }

                let interval = match &watch.mode {
                    WatchMode::Recurring { interval_secs } => Duration::from_secs(*interval_secs),
                    WatchMode::OneShot => return,
                };

                tokio::select! {
                    _ = sleep(interval) => {}
                    _ = cancel_clone.cancelled() => return,
                }
            }
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
    }
}

async fn execute_github_check(
    repo: &str,
    run_id: Option<u64>,
    workflow: Option<&str>,
    branch: Option<&str>,
) -> WatchResult {
    let target_run_id = if let Some(id) = run_id {
        id
    } else {
        let mut cmd = TokioCommand::new("gh");
        cmd.args(["run", "list", "--repo", repo, "--json", "databaseId,status,conclusion", "--limit", "1"]);
        if let Some(w) = workflow {
            cmd.args(["--workflow", w]);
        }
        if let Some(b) = branch {
            cmd.args(["--branch", b]);
        }
        match cmd.output().await {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let runs: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
                match runs.first().and_then(|r| r["databaseId"].as_u64()) {
                    Some(id) => id,
                    None => return WatchResult::GithubRun {
                        run_id: 0, status: "unknown".into(), conclusion: None,
                        url: String::new(), jobs: vec![], outcome: WatchOutcome::Failure,
                    },
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return WatchResult::CommandRun {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout: String::new(), stderr: truncate_output(stderr),
                    outcome: WatchOutcome::Failure,
                };
            }
            Err(e) => {
                return WatchResult::CommandRun {
                    exit_code: -1, stdout: String::new(),
                    stderr: format!("Failed to run gh: {}", e),
                    outcome: WatchOutcome::Failure,
                };
            }
        }
    };

    let output = TokioCommand::new("gh")
        .args(["run", "view", &target_run_id.to_string(), "--repo", repo,
               "--json", "databaseId,status,conclusion,url,jobs"])
        .output().await;

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let run: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
            let status = run["status"].as_str().unwrap_or("unknown").to_string();
            let conclusion = run["conclusion"].as_str().map(|s| s.to_string());
            let url = run["url"].as_str().unwrap_or("").to_string();
            let jobs: Vec<GithubJob> = run["jobs"].as_array().unwrap_or(&vec![]).iter().map(|j| {
                let job_conclusion = j["conclusion"].as_str().map(|s| s.to_string());
                let failed_step = if job_conclusion.as_deref() == Some("failure") {
                    j["steps"].as_array().and_then(|steps| {
                        steps.iter()
                            .find(|s| s["conclusion"].as_str() == Some("failure"))
                            .and_then(|s| s["name"].as_str().map(|n| n.to_string()))
                    })
                } else { None };
                GithubJob {
                    name: j["name"].as_str().unwrap_or("").to_string(),
                    status: j["status"].as_str().unwrap_or("").to_string(),
                    conclusion: job_conclusion, failed_step,
                }
            }).collect();
            let outcome = match (status.as_str(), conclusion.as_deref()) {
                ("completed", Some("success")) => WatchOutcome::Success,
                ("completed", _) => WatchOutcome::Failure,
                _ => WatchOutcome::InProgress,
            };
            WatchResult::GithubRun { run_id: target_run_id, status, conclusion, url, jobs, outcome }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            WatchResult::CommandRun {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::new(), stderr: truncate_output(stderr),
                outcome: WatchOutcome::Failure,
            }
        }
        Err(e) => WatchResult::CommandRun {
            exit_code: -1, stdout: String::new(),
            stderr: format!("Failed to run gh: {}", e),
            outcome: WatchOutcome::Failure,
        },
    }
}

async fn execute_http_check(url: &str, expected_status: u16) -> WatchResult {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let start = std::time::Instant::now();
    match client.get(url).send().await {
        Ok(resp) => {
            let status_code = resp.status().as_u16();
            let response_time_ms = start.elapsed().as_millis() as u64;
            let outcome = if status_code == expected_status { WatchOutcome::Success } else { WatchOutcome::Failure };
            WatchResult::HttpCheck { status_code, response_time_ms, outcome }
        }
        Err(_e) => WatchResult::HttpCheck {
            status_code: 0, response_time_ms: start.elapsed().as_millis() as u64,
            outcome: WatchOutcome::Failure,
        },
    }
}

// ── Config for creating a watch (no runtime fields) ─────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWatchConfig {
    pub name: String,
    pub kind: WatchKind,
    pub mode: WatchMode,
    pub scope: WatchScope,
    pub notify: Option<NotifyConfig>,
}

// ── Tauri Commands ──────────────────────────────────────────

use crate::AppState;

#[tauri::command]
pub fn cmd_create_watch(
    config: CreateWatchConfig,
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Watch {
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
    state.watch_manager.create_watch(watch.clone(), app);
    watch
}

#[tauri::command]
pub fn cmd_remove_watch(id: String, state: tauri::State<AppState>) {
    state.watch_manager.remove_watch(&id);
}

#[tauri::command]
pub fn cmd_list_watches(state: tauri::State<AppState>) -> Vec<Watch> {
    state.watch_manager.store().list()
}

#[tauri::command]
pub fn cmd_pause_watch(id: String, state: tauri::State<AppState>, app: tauri::AppHandle) {
    state.watch_manager.pause_watch(&id, &app);
}

#[tauri::command]
pub fn cmd_resume_watch(id: String, state: tauri::State<AppState>, app: tauri::AppHandle) {
    state.watch_manager.resume_watch(&id, app);
}

async fn execute_shell_check(command: &str, working_dir: Option<&str>, success_exit_code: i32) -> WatchResult {
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
            let outcome = if exit_code == success_exit_code { WatchOutcome::Success } else { WatchOutcome::Failure };
            WatchResult::CommandRun { exit_code, stdout, stderr, outcome }
        }
        Err(e) => WatchResult::CommandRun {
            exit_code: -1, stdout: String::new(),
            stderr: format!("Failed to execute: {}", e),
            outcome: WatchOutcome::Failure,
        },
    }
}
