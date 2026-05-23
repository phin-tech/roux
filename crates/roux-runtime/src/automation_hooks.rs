use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value as MiniValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

use roux_core::{Watch, WatchOutcome, WorktreeProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum HookEvent {
    PreWatchRun,
    PostWatchRun,
    PostWatchChange,
    PostWatchFailure,
    PostWatchSuccess,
    PreWorktreeCreate,
    PostWorktreeCreate,
    PreWorktreeRemove,
    PostWorktreeRemove,
    PostSessionCreate,
    PreSessionClose,
    PostSessionClose,
    PreTaskRun,
    PostTaskRun,
    PostTaskFailure,
    PostTaskSuccess,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreWatchRun => "pre-watch-run",
            Self::PostWatchRun => "post-watch-run",
            Self::PostWatchChange => "post-watch-change",
            Self::PostWatchFailure => "post-watch-failure",
            Self::PostWatchSuccess => "post-watch-success",
            Self::PreWorktreeCreate => "pre-worktree-create",
            Self::PostWorktreeCreate => "post-worktree-create",
            Self::PreWorktreeRemove => "pre-worktree-remove",
            Self::PostWorktreeRemove => "post-worktree-remove",
            Self::PostSessionCreate => "post-session-create",
            Self::PreSessionClose => "pre-session-close",
            Self::PostSessionClose => "post-session-close",
            Self::PreTaskRun => "pre-task-run",
            Self::PostTaskRun => "post-task-run",
            Self::PostTaskFailure => "post-task-failure",
            Self::PostTaskSuccess => "post-task-success",
        }
    }

    pub fn is_blocking(self) -> bool {
        self.as_str().starts_with("pre-")
    }
}

impl std::str::FromStr for HookEvent {
    type Err = HookError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pre-watch-run" => Ok(Self::PreWatchRun),
            "post-watch-run" => Ok(Self::PostWatchRun),
            "post-watch-change" => Ok(Self::PostWatchChange),
            "post-watch-failure" => Ok(Self::PostWatchFailure),
            "post-watch-success" => Ok(Self::PostWatchSuccess),
            "pre-worktree-create" => Ok(Self::PreWorktreeCreate),
            "post-worktree-create" => Ok(Self::PostWorktreeCreate),
            "pre-worktree-remove" => Ok(Self::PreWorktreeRemove),
            "post-worktree-remove" => Ok(Self::PostWorktreeRemove),
            "post-session-create" => Ok(Self::PostSessionCreate),
            "pre-session-close" => Ok(Self::PreSessionClose),
            "post-session-close" => Ok(Self::PostSessionClose),
            "pre-task-run" => Ok(Self::PreTaskRun),
            "post-task-run" => Ok(Self::PostTaskRun),
            "post-task-failure" => Ok(Self::PostTaskFailure),
            "post-task-success" => Ok(Self::PostTaskSuccess),
            _ => Err(HookError::InvalidEvent(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HookSourceKind {
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookCondition {
    pub provider: Option<String>,
    pub worktrunk: Option<bool>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone)]
struct HookCommand {
    event: HookEvent,
    source: HookSourceKind,
    config_path: PathBuf,
    step_index: usize,
    name: String,
    command: String,
    when: Option<HookCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookContext {
    pub hook_type: String,
    pub provider: Option<String>,
    pub configured_provider: Option<String>,
    pub worktrunk: bool,
    pub provider_hooks_ran: Vec<String>,
    pub scope: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub watch: Option<Watch>,
    pub previous_outcome: Option<WatchOutcome>,
    pub outcome: Option<WatchOutcome>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

impl HookContext {
    pub fn new(event: HookEvent) -> Self {
        Self {
            hook_type: event.as_str().to_string(),
            provider: None,
            configured_provider: None,
            worktrunk: false,
            provider_hooks_ran: Vec::new(),
            scope: None,
            repo_path: None,
            worktree_path: None,
            branch: None,
            session_id: None,
            project_id: None,
            task_id: None,
            watch: None,
            previous_outcome: None,
            outcome: None,
            args: Vec::new(),
            cwd: None,
        }
    }

    pub fn for_watch(event: HookEvent, watch: &Watch) -> Self {
        let mut session_id = None;
        let mut project_id = None;
        let scope = match &watch.scope {
            roux_core::WatchScope::Global => Some("global".to_string()),
            roux_core::WatchScope::Session { session_id: sid } => {
                session_id = Some(sid.clone());
                Some("session".to_string())
            }
            roux_core::WatchScope::Project { project_id: pid } => {
                project_id = Some(pid.clone());
                Some("project".to_string())
            }
        };
        let cwd = match &watch.kind {
            roux_core::WatchKind::ShellCommand { working_dir, .. } => working_dir.clone(),
            roux_core::WatchKind::Task { working_dir, .. } => Some(working_dir.clone()),
            _ => None,
        };
        Self { scope, cwd, session_id, project_id, watch: Some(watch.clone()), ..Self::new(event) }
    }

    pub fn with_provider(mut self, configured: WorktreeProvider, wt_available: bool) -> Self {
        let configured_provider = provider_name(configured).to_string();
        let provider = match configured {
            WorktreeProvider::Git => "git",
            WorktreeProvider::Auto | WorktreeProvider::Worktrunk if wt_available => "worktrunk",
            WorktreeProvider::Auto | WorktreeProvider::Worktrunk => "git",
        };
        self.configured_provider = Some(configured_provider);
        self.provider = Some(provider.to_string());
        self.worktrunk = provider == "worktrunk";
        self
    }

    fn as_json(&self, hook_name: &str) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or_else(|_| json!({}));
        if let Value::Object(ref mut map) = value {
            map.insert("hook_name".into(), Value::String(hook_name.to_string()));
            insert_string_alias(map, "hook_type", &self.hook_type);
            insert_opt_string_alias(map, "configured_provider", self.configured_provider.as_ref());
            insert_opt_string_alias(map, "repo_path", self.repo_path.as_ref());
            insert_opt_string_alias(map, "worktree_path", self.worktree_path.as_ref());
            insert_opt_string_alias(map, "session_id", self.session_id.as_ref());
            insert_opt_string_alias(map, "project_id", self.project_id.as_ref());
            insert_opt_string_alias(map, "task_id", self.task_id.as_ref());
            if let Some(previous) = self.previous_outcome.as_ref() {
                map.insert(
                    "previous_outcome".into(),
                    serde_json::to_value(previous).unwrap_or(Value::Null),
                );
            }
            map.insert(
                "provider_hooks_ran".into(),
                serde_json::to_value(&self.provider_hooks_ran).unwrap_or(Value::Array(Vec::new())),
            );
        }
        value
    }

    fn cwd_path(&self) -> Option<PathBuf> {
        self.cwd
            .as_ref()
            .or(self.worktree_path.as_ref())
            .or(self.repo_path.as_ref())
            .map(PathBuf::from)
    }
}

fn insert_string_alias(map: &mut Map<String, Value>, key: &str, value: &str) {
    map.insert(key.into(), Value::String(value.to_string()));
}

fn insert_opt_string_alias(map: &mut Map<String, Value>, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        insert_string_alias(map, key, value);
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookListItem {
    pub event: String,
    pub source: HookSourceKind,
    pub config_path: String,
    pub name: String,
    pub command: String,
    pub approval_id: Option<String>,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookPreviewItem {
    pub event: String,
    pub source: HookSourceKind,
    pub config_path: String,
    pub name: String,
    pub command: String,
    pub rendered_command: String,
    pub approval_id: Option<String>,
    pub approved: bool,
    pub matched: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookRunRequest {
    pub event: String,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub scope: Option<String>,
    pub provider: Option<String>,
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookRunSummary {
    pub event: String,
    pub ran: usize,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookLogEntry {
    pub file: String,
    pub path: String,
    pub event: Option<String>,
    pub name: Option<String>,
    pub source: Option<HookSourceKind>,
    pub exit_code: Option<i32>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Error)]
pub enum HookError {
    #[error("invalid hook event: {0}")]
    InvalidEvent(String),
    #[error("failed to read {path}: {source}")]
    ReadConfig { path: String, source: std::io::Error },
    #[error("failed to parse {path}: {source}")]
    ParseConfig { path: String, source: toml::de::Error },
    #[error("failed to create hook log directory: {0}")]
    CreateLogDir(std::io::Error),
    #[error("failed to write hook log: {0}")]
    WriteLog(std::io::Error),
    #[error("project hook requires approval: {0}")]
    ApprovalRequired(String),
    #[error("hook `{name}` failed with exit code {code}: {stderr}")]
    CommandFailed { name: String, code: i32, stderr: String },
    #[error("failed to execute hook `{name}`: {source}")]
    Execute { name: String, source: std::io::Error },
    #[error("failed to serialize hook context: {0}")]
    SerializeContext(serde_json::Error),
    #[error("failed to render hook template `{name}`: {source}")]
    RenderTemplate { name: String, source: minijinja::Error },
    #[error("failed to write approvals: {0}")]
    WriteApprovals(std::io::Error),
    #[error("failed to read hook log: {0}")]
    ReadLog(std::io::Error),
    #[error("refusing to read hook log outside Roux hook logs")]
    LogPathOutsideRoot,
}

#[derive(Clone)]
pub struct AutomationHookManager {
    user_config_path: PathBuf,
    approval_path: PathBuf,
    log_dir: PathBuf,
}

impl AutomationHookManager {
    pub fn new() -> Self {
        Self::from_config_root(default_config_root())
    }

    pub fn from_config_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            user_config_path: root.join("hooks.toml"),
            approval_path: root.join("hook-approvals.json"),
            log_dir: root.join("logs").join("hooks"),
        }
    }

    pub async fn run_blocking(
        &self,
        event: HookEvent,
        mut context: HookContext,
    ) -> Result<usize, HookError> {
        context.hook_type = event.as_str().to_string();
        let commands = self.matching_commands(event, &context)?;
        let worktrees = precompute_worktrees(context.repo_path.as_deref()).await;
        let mut ran = 0;
        for step in group_by_step(commands) {
            for command in step {
                self.ensure_approved(&command)?;
                let rendered = render_template(
                    &command.command,
                    &context,
                    &command.name,
                    worktrees.as_deref(),
                )?;
                let result = execute_command(&command, &rendered, &context).await?;
                if let Err(e) = self.write_log(&command, &rendered, &result).await {
                    eprintln!("roux automation hook log write failed for `{}`: {e}", command.name);
                }
                ran += 1;
                if result.exit_code != 0 {
                    return Err(HookError::CommandFailed {
                        name: command.name,
                        code: result.exit_code,
                        stderr: result.stderr,
                    });
                }
            }
        }
        Ok(ran)
    }

    pub fn spawn_background(&self, event: HookEvent, mut context: HookContext) {
        context.hook_type = event.as_str().to_string();
        let manager = self.clone();
        tokio::spawn(async move {
            if let Err(e) = manager.run_background(event, context).await {
                eprintln!("roux automation hook {event:?} failed: {e}");
            }
        });
    }

    pub async fn run_background(
        &self,
        event: HookEvent,
        mut context: HookContext,
    ) -> Result<usize, HookError> {
        context.hook_type = event.as_str().to_string();
        let commands = self.matching_commands(event, &context)?;
        let worktrees = precompute_worktrees(context.repo_path.as_deref()).await;
        let mut ran = 0;
        for step in group_by_step(commands) {
            let mut joins = Vec::new();
            for command in step {
                let manager = self.clone();
                let ctx = context.clone();
                let worktrees = worktrees.clone();
                joins.push(tokio::spawn(async move {
                    if let Err(e) = manager.ensure_approved(&command) {
                        let rendered = render_template(
                            &command.command,
                            &ctx,
                            &command.name,
                            worktrees.as_deref(),
                        )
                        .unwrap_or_else(|e| format!("<template error: {e}>"));
                        let result = HookCommandResult {
                            exit_code: -1,
                            stdout: String::new(),
                            stderr: e.to_string(),
                            started_at: now_ms(),
                            finished_at: now_ms(),
                        };
                        let _ = manager.write_log(&command, &rendered, &result).await;
                        return 0;
                    }
                    match render_template(
                        &command.command,
                        &ctx,
                        &command.name,
                        worktrees.as_deref(),
                    ) {
                        Ok(rendered) => match execute_command(&command, &rendered, &ctx).await {
                            Ok(result) => {
                                let _ = manager.write_log(&command, &rendered, &result).await;
                                1
                            }
                            Err(e) => {
                                let result = HookCommandResult {
                                    exit_code: -1,
                                    stdout: String::new(),
                                    stderr: e.to_string(),
                                    started_at: now_ms(),
                                    finished_at: now_ms(),
                                };
                                let _ = manager.write_log(&command, &rendered, &result).await;
                                0
                            }
                        },
                        Err(e) => {
                            let rendered = format!("<template error: {e}>");
                            let result = HookCommandResult {
                                exit_code: -1,
                                stdout: String::new(),
                                stderr: e.to_string(),
                                started_at: now_ms(),
                                finished_at: now_ms(),
                            };
                            let _ = manager.write_log(&command, &rendered, &result).await;
                            0
                        }
                    }
                }));
            }
            for join in joins {
                ran += join.await.unwrap_or(0);
            }
        }
        Ok(ran)
    }

    pub fn list_hooks(&self, repo_path: Option<&str>) -> Result<Vec<HookListItem>, HookError> {
        let approvals = self.load_approvals();
        Ok(self
            .load_commands(repo_path)?
            .into_iter()
            .map(|command| {
                let approval_id =
                    (command.source == HookSourceKind::Project).then(|| approval_id(&command));
                let approved =
                    approval_id.as_ref().map(|id| approvals.contains(id)).unwrap_or(true);
                HookListItem {
                    event: command.event.as_str().to_string(),
                    source: command.source,
                    config_path: command.config_path.to_string_lossy().into_owned(),
                    name: command.name,
                    command: command.command,
                    approval_id,
                    approved,
                }
            })
            .collect())
    }

    pub fn preview(
        &self,
        event: HookEvent,
        context: &HookContext,
    ) -> Result<Vec<HookPreviewItem>, HookError> {
        let approvals = self.load_approvals();
        Ok(self
            .load_commands(context.repo_path.as_deref())?
            .into_iter()
            .filter(|command| command.event == event)
            .map(|command| {
                let approval_id =
                    (command.source == HookSourceKind::Project).then(|| approval_id(&command));
                let approved =
                    approval_id.as_ref().map(|id| approvals.contains(id)).unwrap_or(true);
                let matched = condition_matches(command.when.as_ref(), context);
                HookPreviewItem {
                    event: command.event.as_str().to_string(),
                    source: command.source,
                    config_path: command.config_path.to_string_lossy().into_owned(),
                    name: command.name.clone(),
                    command: command.command.clone(),
                    rendered_command: render_template(
                        &command.command,
                        context,
                        &command.name,
                        None,
                    )
                    .unwrap_or_else(|e| format!("<template error: {e}>")),
                    approval_id,
                    approved,
                    matched,
                }
            })
            .collect())
    }

    pub fn approve(&self, approval_id: &str) -> Result<(), HookError> {
        let mut approvals = self.load_approvals();
        approvals.insert(approval_id.to_string());
        self.write_approvals(&approvals)
    }

    pub fn clear_approvals(&self) -> Result<(), HookError> {
        self.write_approvals(&BTreeSet::new())
    }

    pub fn list_logs(&self) -> Vec<HookLogEntry> {
        let mut entries = Vec::new();
        let Ok(read_dir) = std::fs::read_dir(&self.log_dir) else {
            return entries;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let value: Value = serde_json::from_str(&content).unwrap_or(Value::Null);
            entries.push(HookLogEntry {
                file: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                event: value.get("event").and_then(Value::as_str).map(str::to_string),
                name: value.get("name").and_then(Value::as_str).map(str::to_string),
                source: value.get("source").and_then(Value::as_str).and_then(|s| match s {
                    "user" => Some(HookSourceKind::User),
                    "project" => Some(HookSourceKind::Project),
                    _ => None,
                }),
                exit_code: value.get("exitCode").and_then(Value::as_i64).map(|n| n as i32),
                started_at: value.get("startedAt").and_then(Value::as_u64),
                finished_at: value.get("finishedAt").and_then(Value::as_u64),
            });
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.finished_at.unwrap_or(0)));
        entries
    }

    pub fn read_log(&self, path: &str) -> Result<String, HookError> {
        let root = self.log_dir.canonicalize().map_err(HookError::ReadLog)?;
        let target = PathBuf::from(path).canonicalize().map_err(HookError::ReadLog)?;
        if !target.starts_with(root) {
            return Err(HookError::LogPathOutsideRoot);
        }
        std::fs::read_to_string(target).map_err(HookError::ReadLog)
    }

    fn matching_commands(
        &self,
        event: HookEvent,
        context: &HookContext,
    ) -> Result<Vec<HookCommand>, HookError> {
        Ok(self
            .load_commands(context.repo_path.as_deref())?
            .into_iter()
            .filter(|command| command.event == event)
            .filter(|command| condition_matches(command.when.as_ref(), context))
            .collect())
    }

    fn load_commands(&self, repo_path: Option<&str>) -> Result<Vec<HookCommand>, HookError> {
        let mut commands = Vec::new();
        commands.extend(load_config_commands(&self.user_config_path, HookSourceKind::User)?);
        if let Some(repo_path) = repo_path {
            commands.extend(load_config_commands(
                &Path::new(repo_path).join(".config").join("roux").join("hooks.toml"),
                HookSourceKind::Project,
            )?);
        }
        Ok(commands)
    }

    fn load_approvals(&self) -> BTreeSet<String> {
        let Ok(content) = std::fs::read_to_string(&self.approval_path) else {
            return BTreeSet::new();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    fn write_approvals(&self, approvals: &BTreeSet<String>) -> Result<(), HookError> {
        if let Some(parent) = self.approval_path.parent() {
            std::fs::create_dir_all(parent).map_err(HookError::WriteApprovals)?;
        }
        let json = serde_json::to_string_pretty(approvals).map_err(|e| {
            HookError::WriteApprovals(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        std::fs::write(&self.approval_path, json).map_err(HookError::WriteApprovals)
    }

    fn ensure_approved(&self, command: &HookCommand) -> Result<(), HookError> {
        if command.source == HookSourceKind::User {
            return Ok(());
        }
        let id = approval_id(command);
        if self.load_approvals().contains(&id) {
            Ok(())
        } else {
            Err(HookError::ApprovalRequired(id))
        }
    }

    async fn write_log(
        &self,
        command: &HookCommand,
        rendered: &str,
        result: &HookCommandResult,
    ) -> Result<(), HookError> {
        tokio::fs::create_dir_all(&self.log_dir).await.map_err(HookError::CreateLogDir)?;
        let source = match command.source {
            HookSourceKind::User => "user",
            HookSourceKind::Project => "project",
        };
        let payload = json!({
            "event": command.event.as_str(),
            "name": command.name,
            "source": source,
            "configPath": command.config_path,
            "command": command.command,
            "renderedCommand": rendered,
            "exitCode": result.exit_code,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "startedAt": result.started_at,
            "finishedAt": result.finished_at,
        });
        static LOG_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
        let file = format!(
            "{}-{}-{}-{}.json",
            result.finished_at,
            seq,
            command.event.as_str(),
            sanitize_file_component(&command.name)
        );
        let json = serde_json::to_string_pretty(&payload).map_err(HookError::SerializeContext)?;
        tokio::fs::write(self.log_dir.join(file), json).await.map_err(HookError::WriteLog)
    }
}

impl Default for AutomationHookManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn context_from_run_request(
    req: HookRunRequest,
    configured_provider: Option<WorktreeProvider>,
    wt_available: bool,
) -> Result<(HookEvent, HookContext), HookError> {
    let event: HookEvent = req.event.parse()?;
    let mut context = HookContext::new(event);
    context.repo_path = req.repo_path;
    context.worktree_path = req.worktree_path;
    context.branch = req.branch;
    context.session_id = req.session_id;
    context.project_id = req.project_id;
    context.task_id = req.task_id;
    context.scope = req.scope;
    context.args = req.args.unwrap_or_default();
    if let Some(provider) = req.provider {
        context.provider = Some(provider.clone());
        context.worktrunk = provider == "worktrunk";
    } else if let Some(provider) = configured_provider {
        context = context.with_provider(provider, wt_available);
    }
    if context.cwd.is_none() {
        context.cwd = context.worktree_path.clone().or(context.repo_path.clone());
    }
    Ok((event, context))
}

fn provider_name(provider: WorktreeProvider) -> &'static str {
    match provider {
        WorktreeProvider::Auto => "auto",
        WorktreeProvider::Git => "git",
        WorktreeProvider::Worktrunk => "worktrunk",
    }
}

fn load_config_commands(
    path: &Path,
    source: HookSourceKind,
) -> Result<Vec<HookCommand>, HookError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path).map_err(|source| HookError::ReadConfig {
        path: path.to_string_lossy().into_owned(),
        source,
    })?;
    let root: toml::Value = toml::from_str(&content).map_err(|source| HookError::ParseConfig {
        path: path.to_string_lossy().into_owned(),
        source,
    })?;
    parse_config_value(&root, path, source)
}

fn parse_config_value(
    root: &toml::Value,
    path: &Path,
    source: HookSourceKind,
) -> Result<Vec<HookCommand>, HookError> {
    let Some(table) = root.as_table() else {
        return Ok(Vec::new());
    };
    let mut commands = Vec::new();
    for (event_name, value) in table {
        let Ok(event) = event_name.parse::<HookEvent>() else {
            continue;
        };
        match value {
            toml::Value::String(command) => {
                commands.push(HookCommand {
                    event,
                    source,
                    config_path: path.to_path_buf(),
                    step_index: 0,
                    name: "default".into(),
                    command: command.clone(),
                    when: None,
                });
            }
            toml::Value::Table(table) => {
                let when = parse_when(table.get("when"));
                for (name, command_value) in table {
                    if name == "when" {
                        continue;
                    }
                    if let Some(command) = command_value.as_str() {
                        commands.push(HookCommand {
                            event,
                            source,
                            config_path: path.to_path_buf(),
                            step_index: 0,
                            name: name.clone(),
                            command: command.to_string(),
                            when: when.clone(),
                        });
                    }
                }
            }
            toml::Value::Array(steps) => {
                for (step_index, step) in steps.iter().enumerate() {
                    let Some(step_table) = step.as_table() else {
                        continue;
                    };
                    let when = parse_when(step_table.get("when"));
                    for (name, command_value) in step_table {
                        if name == "when" {
                            continue;
                        }
                        if let Some(command) = command_value.as_str() {
                            commands.push(HookCommand {
                                event,
                                source,
                                config_path: path.to_path_buf(),
                                step_index,
                                name: name.clone(),
                                command: command.to_string(),
                                when: when.clone(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(commands)
}

fn parse_when(value: Option<&toml::Value>) -> Option<HookCondition> {
    let table = value?.as_table()?;
    Some(HookCondition {
        provider: table.get("provider").and_then(toml::Value::as_str).map(str::to_string),
        worktrunk: table.get("worktrunk").and_then(toml::Value::as_bool),
        scope: table.get("scope").and_then(toml::Value::as_str).map(str::to_string),
    })
}

fn group_by_step(commands: Vec<HookCommand>) -> Vec<Vec<HookCommand>> {
    let mut steps: BTreeMap<usize, Vec<HookCommand>> = BTreeMap::new();
    for command in commands {
        steps.entry(command.step_index).or_default().push(command);
    }
    steps.into_values().collect()
}

fn condition_matches(condition: Option<&HookCondition>, context: &HookContext) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    if let Some(provider) = condition.provider.as_deref() {
        let matches_effective = context.provider.as_deref() == Some(provider);
        let matches_configured_auto =
            provider == "auto" && context.configured_provider.as_deref() == Some("auto");
        if !matches_effective && !matches_configured_auto {
            return false;
        }
    }
    if let Some(worktrunk) = condition.worktrunk {
        if context.worktrunk != worktrunk {
            return false;
        }
    }
    if let Some(scope) = condition.scope.as_deref() {
        if context.scope.as_deref() != Some(scope) {
            return false;
        }
    }
    true
}

fn approval_id(command: &HookCommand) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.config_path.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(command.event.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(command.name.as_bytes());
    hasher.update(b"\0");
    hasher.update(command.command.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn render_template(
    command: &str,
    context: &HookContext,
    hook_name: &str,
    worktrees: Option<&[(String, String)]>,
) -> Result<String, HookError> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_filter("sanitize", sanitize_filter);
    env.add_filter("sanitize_hash", sanitize_hash_filter);
    env.add_filter("sanitize_db", sanitize_db_filter);
    env.add_filter("hash_port", hash_port_filter);

    let repo_path = context.repo_path.clone();
    let precomputed: Option<Vec<(String, String)>> = worktrees.map(|w| w.to_vec());
    env.add_function("worktree_path_of_branch", move |branch: String| -> String {
        if let Some(map) = precomputed.as_ref() {
            return map
                .iter()
                .find(|(b, _)| b == &branch)
                .map(|(_, p)| p.clone())
                .unwrap_or_default();
        }
        repo_path
            .as_deref()
            .and_then(|repo| roux_core::list_worktrees(repo).ok())
            .and_then(|worktrees| {
                worktrees.into_iter().find(|worktree| worktree.branch == branch).map(|w| w.path)
            })
            .unwrap_or_default()
    });

    let data = context.as_json(hook_name);
    env.render_str(command, MiniValue::from_serialize(&data))
        .map_err(|source| HookError::RenderTemplate { name: hook_name.to_string(), source })
}

async fn precompute_worktrees(repo_path: Option<&str>) -> Option<Vec<(String, String)>> {
    let repo = repo_path?.to_string();
    tokio::task::spawn_blocking(move || roux_core::list_worktrees(&repo).ok())
        .await
        .ok()
        .flatten()
        .map(|ws| ws.into_iter().map(|w| (w.branch, w.path)).collect())
}

fn sanitize_filter(value: String) -> String {
    sanitize_template_token(&value)
}

fn sanitize_hash_filter(value: String) -> String {
    let sanitized = sanitize_template_token(&value);
    if sanitized == value && sanitized.len() <= 48 {
        sanitized
    } else {
        format!("{}-{}", truncate_token(&sanitized, 39), short_hash(&value))
    }
}

fn sanitize_db_filter(value: String) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' || ch.is_whitespace() || ch == '/' || ch == '.' {
            push_single_separator(&mut out, '_');
        }
    }
    let out = out.trim_matches('_').to_string();
    let mut out = if out.is_empty() { "value".to_string() } else { out };
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out = format!("x_{out}");
    }
    truncate_token(&out, 63)
}

fn hash_port_filter(value: String) -> u16 {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let raw = u16::from_be_bytes([digest[0], digest[1]]) as u32;
    (10_000 + (raw % 40_000)) as u16
}

fn sanitize_template_token(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' {
            push_single_separator(&mut out, ch);
        } else if ch.is_whitespace() || ch == '/' || ch == '.' || ch == ':' {
            push_single_separator(&mut out, '-');
        }
    }
    let out = out.trim_matches('-').trim_matches('_').to_string();
    if out.is_empty() {
        "value".to_string()
    } else {
        out
    }
}

fn push_single_separator(out: &mut String, separator: char) {
    if !out.ends_with(separator) {
        out.push(separator);
    }
}

fn truncate_token(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect::<String>().trim_matches('-').trim_matches('_').to_string()
}

fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())[..8].to_string()
}

struct HookCommandResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    started_at: u64,
    finished_at: u64,
}

async fn execute_command(
    command: &HookCommand,
    rendered: &str,
    context: &HookContext,
) -> Result<HookCommandResult, HookError> {
    let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
    let flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
    let started_at = now_ms();
    let mut child = TokioCommand::new(shell);
    child.arg(flag).arg(rendered);
    child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = context.cwd_path() {
        child.current_dir(cwd);
    }
    // Kill the hook child if the future is dropped (e.g. the watch poll
    // loop's `select!` chose the cancellation branch). Without this a
    // wedged hook can keep a removed session-scoped watch alive long
    // after the session was deleted.
    child.kill_on_drop(true);
    let mut child = child
        .spawn()
        .map_err(|source| HookError::Execute { name: command.name.clone(), source })?;
    if let Some(mut stdin) = child.stdin.take() {
        let json = serde_json::to_vec(&context.as_json(&command.name))
            .map_err(HookError::SerializeContext)?;
        stdin
            .write_all(&json)
            .await
            .map_err(|source| HookError::Execute { name: command.name.clone(), source })?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|source| HookError::Execute { name: command.name.clone(), source })?;
    Ok(HookCommandResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        started_at,
        finished_at: now_ms(),
    })
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn sanitize_file_component(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

pub fn worktree_provider_hooks(event: HookEvent, worktrunk: bool) -> Vec<String> {
    if !worktrunk {
        return Vec::new();
    }
    match event {
        HookEvent::PostWorktreeCreate => vec!["pre-start".into(), "post-start".into()],
        HookEvent::PostWorktreeRemove => vec!["pre-remove".into(), "post-remove".into()],
        _ => Vec::new(),
    }
}

pub fn request_from_socket_args(args: Value) -> Result<HookRunRequest, String> {
    serde_json::from_value(args).map_err(|e| format!("invalid hook args: {e}"))
}

pub fn hook_list_to_value(items: Vec<HookListItem>) -> Value {
    serde_json::to_value(items).unwrap_or_else(|_| Value::Array(Vec::new()))
}

pub fn hook_run_to_value(summary: HookRunSummary) -> Value {
    serde_json::to_value(summary).unwrap_or_else(|_| Value::Object(Map::new()))
}

fn default_config_root() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
        .join(".config")
        .join("roux")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn parse(content: &str) -> Vec<HookCommand> {
        let value: toml::Value = toml::from_str(content).unwrap();
        parse_config_value(
            &value,
            Path::new("/repo/.config/roux/hooks.toml"),
            HookSourceKind::Project,
        )
        .unwrap()
    }

    #[test]
    fn parses_string_table_and_pipeline_forms() {
        let commands = parse(
            r#"
pre-watch-run = "echo one"

[post-watch-run]
a = "echo a"
b = "echo b"

[[post-watch-failure]]
collect = "echo collect"

[[post-watch-failure]]
notify = "echo notify"
"#,
        );

        assert_eq!(commands.len(), 5);
        assert!(commands.iter().any(|c| c.event == HookEvent::PreWatchRun && c.name == "default"));
        assert!(commands.iter().any(|c| c.event == HookEvent::PostWatchRun && c.name == "a"));
        assert!(commands
            .iter()
            .any(|c| c.event == HookEvent::PostWatchFailure && c.step_index == 1));
    }

    #[test]
    fn condition_matching_checks_provider_worktrunk_and_scope() {
        let condition = HookCondition {
            provider: Some("worktrunk".into()),
            worktrunk: Some(true),
            scope: Some("session".into()),
        };
        let context = HookContext {
            provider: Some("worktrunk".into()),
            worktrunk: true,
            scope: Some("session".into()),
            ..HookContext::new(HookEvent::PostWatchRun)
        };

        assert!(condition_matches(Some(&condition), &context));

        let context = HookContext { provider: Some("git".into()), ..context };
        assert!(!condition_matches(Some(&condition), &context));
    }

    #[test]
    fn condition_matching_allows_configured_auto_provider() {
        let condition =
            HookCondition { provider: Some("auto".into()), worktrunk: None, scope: None };
        let context = HookContext {
            provider: Some("worktrunk".into()),
            configured_provider: Some("auto".into()),
            ..HookContext::new(HookEvent::PostWorktreeCreate)
        };

        assert!(condition_matches(Some(&condition), &context));

        let context = HookContext { configured_provider: Some("worktrunk".into()), ..context };
        assert!(!condition_matches(Some(&condition), &context));
    }

    #[test]
    fn approval_identity_changes_when_command_changes() {
        let mut commands = parse(
            r#"
[post-watch-run]
one = "echo a"
"#,
        );
        let first = approval_id(&commands.remove(0));
        let mut commands = parse(
            r#"
[post-watch-run]
one = "echo b"
"#,
        );
        let second = approval_id(&commands.remove(0));

        assert_ne!(first, second);
    }

    #[test]
    fn renders_template_from_context() {
        let context = HookContext {
            session_id: Some("s1".into()),
            branch: Some("feature/search".into()),
            ..HookContext::new(HookEvent::PostWatchSuccess)
        };

        let rendered = render_template(
            "echo {{ hook_type }} {{ branch }} {{ session_id }}",
            &context,
            "ci",
            None,
        )
        .unwrap();

        assert_eq!(rendered, "echo post-watch-success feature/search s1");
    }

    #[test]
    fn renders_template_conditionals_and_default_values() {
        let context = HookContext {
            provider: Some("worktrunk".into()),
            worktrunk: true,
            ..HookContext::new(HookEvent::PostWorktreeCreate)
        };

        let rendered = render_template(
            "{% if provider == 'worktrunk' %}wt{% endif %} {{ missing | default('fallback') }}",
            &context,
            "notify",
            None,
        )
        .unwrap();

        assert_eq!(rendered, "wt fallback");
    }

    #[test]
    fn renders_template_filters() {
        let context = HookContext {
            branch: Some("Feature/Login Flow".into()),
            ..HookContext::new(HookEvent::PostWorktreeCreate)
        };

        let rendered = render_template(
            "{{ branch | sanitize }} {{ branch | sanitize_db }} {{ branch | hash_port }}",
            &context,
            "ports",
            None,
        )
        .unwrap();
        let parts = rendered.split_whitespace().collect::<Vec<_>>();

        assert_eq!(parts[0], "feature-login-flow");
        assert_eq!(parts[1], "feature_login_flow");
        assert!(parts[2].parse::<u16>().is_ok());
    }

    #[tokio::test]
    async fn run_blocking_executes_user_hook() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("hooks.toml");
        std::fs::write(&config, r#"pre-watch-run = "cat >/dev/null""#).unwrap();
        let manager = AutomationHookManager {
            user_config_path: config,
            approval_path: temp.path().join("approvals.json"),
            log_dir: temp.path().join("logs"),
        };

        let ran = manager
            .run_blocking(HookEvent::PreWatchRun, HookContext::new(HookEvent::PreWatchRun))
            .await
            .unwrap();

        assert_eq!(ran, 1);
    }
}
