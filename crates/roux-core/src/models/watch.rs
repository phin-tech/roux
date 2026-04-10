use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchScope {
    Global,
    Session {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    Project {
        #[serde(rename = "projectId")]
        project_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RuntimeState {
    Pending,
    Active,
    Paused,
    Stopped,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchKind {
    GithubAction {
        repo: String,
        #[serde(rename = "runId")]
        run_id: Option<u64>,
        workflow: Option<String>,
        branch: Option<String>,
    },
    HttpHealth {
        url: String,
        #[serde(rename = "expectedStatus")]
        expected_status: u16,
    },
    ShellCommand {
        command: String,
        #[serde(rename = "workingDir")]
        working_dir: Option<String>,
        #[serde(rename = "successExitCode")]
        success_exit_code: i32,
    },
    Task {
        #[serde(rename = "taskId")]
        task_id: String,
        command: String,
        #[serde(rename = "workingDir")]
        working_dir: String,
    },
    GithubPr {
        repo: String,
        #[serde(rename = "prNumber")]
        pr_number: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchMode {
    Recurring {
        #[serde(rename = "intervalSecs")]
        interval_secs: u64,
    },
    OneShot,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WatchResult {
    GithubRun {
        #[serde(rename = "runId")]
        run_id: u64,
        status: String,
        conclusion: Option<String>,
        url: String,
        jobs: Vec<GithubJob>,
        outcome: WatchOutcome,
    },
    HttpCheck {
        #[serde(rename = "statusCode")]
        status_code: u16,
        #[serde(rename = "responseTimeMs")]
        response_time_ms: u64,
        outcome: WatchOutcome,
    },
    CommandRun {
        #[serde(rename = "exitCode")]
        exit_code: i32,
        stdout: String,
        stderr: String,
        outcome: WatchOutcome,
    },
    GithubPr {
        #[serde(rename = "prNumber")]
        pr_number: u64,
        state: String,
        title: String,
        url: String,
        #[serde(rename = "headSha")]
        head_sha: String,
        draft: bool,
        reviews: Vec<PrReview>,
        checks: Vec<PrCheckRun>,
        outcome: WatchOutcome,
    },
}

impl WatchResult {
    pub fn outcome(&self) -> &WatchOutcome {
        match self {
            WatchResult::GithubRun { outcome, .. } => outcome,
            WatchResult::HttpCheck { outcome, .. } => outcome,
            WatchResult::CommandRun { outcome, .. } => outcome,
            WatchResult::GithubPr { outcome, .. } => outcome,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WatchOutcome {
    Success,
    Failure,
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubJob {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub failed_step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrReview {
    pub reviewer: String,
    pub state: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PrCheckRun {
    pub name: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotifyConfig {
    pub desktop_notification: bool,
    pub on_failure: bool,
    pub on_success: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self { desktop_notification: true, on_failure: true, on_success: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchUpdateEvent {
    pub watch: Watch,
    pub changed: bool,
    pub previous_outcome: Option<WatchOutcome>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateWatchConfig {
    pub name: String,
    pub kind: WatchKind,
    pub mode: WatchMode,
    pub scope: WatchScope,
    pub notify: Option<NotifyConfig>,
}
