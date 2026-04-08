use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
