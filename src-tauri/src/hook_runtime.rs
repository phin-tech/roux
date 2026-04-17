use std::path::{Path, PathBuf};

use roux_core::{
    merge_hook_configs, parse_hooks_kdl, HookConfig, HookDefinition, HookEventKind, HookFilter,
    RouxEvent, RouxEventOrigin, RouxEventSession, RouxEventWorkspace, Watch, WatchKind,
    WatchMode, WatchOutcome, WatchResult, WatchScope,
};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Default)]
pub struct LoadedHooks {
    pub config: HookConfig,
    pub errors: Vec<HookLoadError>,
}

#[derive(Debug)]
pub struct HookLoadError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug)]
pub struct HookDispatchResult {
    pub matched_hook_ids: Vec<String>,
    pub runs: Vec<HookRunResult>,
}

#[derive(Debug)]
pub struct HookRunResult {
    pub hook_id: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn load_hooks_for_event_in(
    config_dir: &Path,
    settings: &crate::settings::RouxSettings,
    event: &RouxEvent,
) -> LoadedHooks {
    let mut out = LoadedHooks::default();

    let global = load_optional_hook_file(&config_dir.join("hooks.kdl"));
    out.errors.extend(global.errors);
    out.config = global.config;

    if let Some(repo_root) = event.workspace.as_ref().and_then(|w| w.repo_root.as_deref()) {
        let trusted = settings.trusted_workspaces.iter().any(|root| root == repo_root);
        if trusted {
            let repo = load_optional_hook_file(Path::new(repo_root).join(".roux").join("hooks.kdl").as_path());
            out.errors.extend(repo.errors);
            out.config = merge_hook_configs(&out.config, &repo.config);
        }
    }

    out
}

pub fn matching_hooks<'a>(config: &'a HookConfig, event: &RouxEvent) -> Vec<&'a HookDefinition> {
    let event_value = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);

    config
        .hooks
        .iter()
        .filter(|hook| hook.enabled)
        .filter(|hook| hook.on.iter().any(|kind| *kind == event.kind))
        .filter(|hook| hook.filters.iter().all(|filter| filter_matches(filter, &event_value)))
        .collect()
}

pub async fn dispatch_event_in(
    config_dir: &Path,
    settings: &crate::settings::RouxSettings,
    event: &RouxEvent,
) -> HookDispatchResult {
    let loaded = load_hooks_for_event_in(config_dir, settings, event);
    let matched = matching_hooks(&loaded.config, event);
    let matched_hook_ids = matched.iter().map(|hook| hook.id.clone()).collect::<Vec<_>>();

    let mut runs = Vec::new();
    for hook in matched {
        runs.push(run_hook(hook, event).await);
    }

    HookDispatchResult { matched_hook_ids, runs }
}

pub fn session_created_event(session: &crate::session::Session, profile: Option<&str>) -> RouxEvent {
    RouxEvent {
        id: uuid::Uuid::new_v4().to_string(),
        kind: HookEventKind::SessionCreated,
        timestamp: session.created_at.to_string(),
        origin: RouxEventOrigin {
            kind: "roux".into(),
            causation_id: None,
            triggered_by_hook_id: None,
        },
        session: Some(RouxEventSession {
            roux_session_id: Some(session.id.clone()),
            pane_id: Some(format!("{}-main", session.id)),
            profile: profile.map(str::to_string),
        }),
        workspace: Some(RouxEventWorkspace {
            repo_root: Some(session.repo_root.clone()),
            worktree_path: Some(session.worktree_path.clone()),
        }),
        payload: serde_json::json!({
            "sessionId": session.id,
            "name": session.name,
            "branch": session.branch,
            "isWorktree": session.is_worktree,
        }),
    }
}

pub fn watch_events(
    watch: &Watch,
    previous_outcome: Option<&WatchOutcome>,
    session: Option<&crate::session::Session>,
) -> Vec<RouxEvent> {
    let Some(result) = watch.last_result.as_ref() else {
        return Vec::new();
    };

    let outcome = result.outcome().clone();
    let timestamp =
        watch.last_checked.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        });
    let session_payload = session.map(|session| RouxEventSession {
        roux_session_id: Some(session.id.clone()),
        pane_id: None,
        profile: None,
    });
    let workspace_payload = session.map(|session| RouxEventWorkspace {
        repo_root: Some(session.repo_root.clone()),
        worktree_path: Some(session.worktree_path.clone()),
    });

    let payload = watch_payload(watch, result, previous_outcome);

    let mut events = vec![RouxEvent {
        id: uuid::Uuid::new_v4().to_string(),
        kind: HookEventKind::WatchCompleted,
        timestamp: timestamp.to_string(),
        origin: RouxEventOrigin {
            kind: "watch".into(),
            causation_id: None,
            triggered_by_hook_id: None,
        },
        session: session_payload.clone(),
        workspace: workspace_payload.clone(),
        payload: payload.clone(),
    }];

    if previous_outcome != Some(&outcome) {
        events.push(RouxEvent {
            id: uuid::Uuid::new_v4().to_string(),
            kind: HookEventKind::WatchOutcomeChanged,
            timestamp: timestamp.to_string(),
            origin: RouxEventOrigin {
                kind: "watch".into(),
                causation_id: None,
                triggered_by_hook_id: None,
            },
            session: session_payload,
            workspace: workspace_payload,
            payload,
        });
    }

    events
}

fn load_optional_hook_file(path: &Path) -> LoadedHooks {
    let mut out = LoadedHooks::default();
    if !path.exists() {
        return out;
    }

    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            out.errors.push(HookLoadError {
                path: path.to_path_buf(),
                message: format!("failed to read hook file: {e}"),
            });
            return out;
        }
    };

    match parse_hooks_kdl(&src) {
        Ok(config) => out.config = config,
        Err(e) => out.errors.push(HookLoadError { path: path.to_path_buf(), message: e.to_string() }),
    }
    out
}

async fn run_hook(hook: &HookDefinition, event: &RouxEvent) -> HookRunResult {
    let mut cmd = tokio::process::Command::new(&hook.run.command);
    cmd.args(&hook.run.args);
    if let Some(cwd) = &hook.run.cwd {
        let resolved = resolve_cwd(cwd, event);
        cmd.current_dir(resolved);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    cmd.env("ROUX_HOOK_ID", &hook.id);
    cmd.env("ROUX_EVENT_KIND", event.kind.as_str());
    cmd.env("ROUX_EVENT_ID", &event.id);
    if let Some(workspace) = &event.workspace {
        if let Some(repo_root) = &workspace.repo_root {
            cmd.env("ROUX_REPO_ROOT", repo_root);
        }
        if let Some(worktree_path) = &workspace.worktree_path {
            cmd.env("ROUX_WORKTREE_PATH", worktree_path);
        }
    }
    if let Some(session) = &event.session {
        if let Some(session_id) = &session.roux_session_id {
            cmd.env("ROUX_SESSION_ID", session_id);
        }
        if let Some(pane_id) = &session.pane_id {
            cmd.env("ROUX_PANE_ID", pane_id);
        }
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return HookRunResult {
                hook_id: hook.id.clone(),
                exit_code: None,
                timed_out: false,
                stdout: String::new(),
                stderr: format!("failed to spawn hook: {e}"),
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(event).unwrap_or_default();
        if let Err(e) = stdin.write_all(&payload).await {
            return HookRunResult {
                hook_id: hook.id.clone(),
                exit_code: None,
                timed_out: false,
                stdout: String::new(),
                stderr: format!("failed to write hook stdin: {e}"),
            };
        }
    }

    let wait = child.wait_with_output();
    let output = if let Some(timeout_ms) = hook.run.timeout_ms {
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), wait).await {
            Ok(result) => match result {
                Ok(output) => {
                    return HookRunResult {
                        hook_id: hook.id.clone(),
                        exit_code: output.status.code(),
                        timed_out: false,
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    };
                }
                Err(e) => {
                    return HookRunResult {
                        hook_id: hook.id.clone(),
                        exit_code: None,
                        timed_out: false,
                        stdout: String::new(),
                        stderr: format!("failed to wait on hook: {e}"),
                    };
                }
            },
            Err(_) => {
                return HookRunResult {
                    hook_id: hook.id.clone(),
                    exit_code: None,
                    timed_out: true,
                    stdout: String::new(),
                    stderr: "hook timed out".into(),
                };
            }
        }
    } else {
        match wait.await {
            Ok(output) => output,
            Err(e) => {
                return HookRunResult {
                    hook_id: hook.id.clone(),
                    exit_code: None,
                    timed_out: false,
                    stdout: String::new(),
                    stderr: format!("failed to wait on hook: {e}"),
                };
            }
        }
    };

    HookRunResult {
        hook_id: hook.id.clone(),
        exit_code: output.status.code(),
        timed_out: false,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn resolve_cwd(cwd: &str, event: &RouxEvent) -> PathBuf {
    match cwd {
        "worktree" => event
            .workspace
            .as_ref()
            .and_then(|w| w.worktree_path.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        "repo" => event
            .workspace
            .as_ref()
            .and_then(|w| w.repo_root.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        other => PathBuf::from(other),
    }
}

fn watch_payload(
    watch: &Watch,
    result: &WatchResult,
    previous_outcome: Option<&WatchOutcome>,
) -> serde_json::Value {
    let (scope_type, session_id, project_id) = match &watch.scope {
        WatchScope::Global => ("global", None, None),
        WatchScope::Session { session_id } => ("session", Some(session_id.clone()), None),
        WatchScope::Project { project_id } => ("project", None, Some(project_id.clone())),
    };

    let mut payload = serde_json::json!({
        "watchId": watch.id,
        "watchName": watch.name,
        "watchType": watch_type_name(&watch.kind),
        "scopeType": scope_type,
        "outcome": watch_outcome_name(result.outcome()),
        "previousOutcome": previous_outcome.map(watch_outcome_name),
        "result": result,
        "watch": watch,
    });

    if let Some(session_id) = session_id {
        payload["sessionId"] = serde_json::Value::String(session_id);
    }
    if let Some(project_id) = project_id {
        payload["projectId"] = serde_json::Value::String(project_id);
    }

    match &watch.kind {
        WatchKind::GithubAction { repo, run_id, workflow, branch } => {
            payload["repo"] = serde_json::Value::String(repo.clone());
            if let Some(run_id) = run_id {
                payload["runId"] = serde_json::json!(run_id);
            }
            if let Some(workflow) = workflow {
                payload["workflow"] = serde_json::Value::String(workflow.clone());
            }
            if let Some(branch) = branch {
                payload["branch"] = serde_json::Value::String(branch.clone());
            }
        }
        WatchKind::HttpHealth { url, expected_status } => {
            payload["url"] = serde_json::Value::String(url.clone());
            payload["expectedStatus"] = serde_json::json!(expected_status);
        }
        WatchKind::ShellCommand { command, working_dir, success_exit_code } => {
            payload["command"] = serde_json::Value::String(command.clone());
            if let Some(working_dir) = working_dir {
                payload["workingDir"] = serde_json::Value::String(working_dir.clone());
            }
            payload["successExitCode"] = serde_json::json!(success_exit_code);
        }
        WatchKind::Task { task_id, command, working_dir } => {
            payload["taskId"] = serde_json::Value::String(task_id.clone());
            payload["command"] = serde_json::Value::String(command.clone());
            payload["workingDir"] = serde_json::Value::String(working_dir.clone());
        }
        WatchKind::GithubPr { repo, pr_number } => {
            payload["repo"] = serde_json::Value::String(repo.clone());
            payload["prNumber"] = serde_json::json!(pr_number);
        }
    }

    match result {
        WatchResult::GithubRun { run_id, status, conclusion, url, jobs, .. } => {
            payload["runId"] = serde_json::json!(run_id);
            payload["status"] = serde_json::Value::String(status.clone());
            payload["url"] = serde_json::Value::String(url.clone());
            if let Some(conclusion) = conclusion {
                payload["conclusion"] = serde_json::Value::String(conclusion.clone());
            }
            let failing_jobs = jobs
                .iter()
                .filter(|job| job.conclusion.as_deref() == Some("failure"))
                .map(|job| job.name.clone())
                .collect::<Vec<_>>();
            payload["hasCiFailures"] = serde_json::Value::Bool(!failing_jobs.is_empty());
            payload["failingJobs"] = serde_json::json!(failing_jobs);
        }
        WatchResult::HttpCheck { status_code, response_time_ms, .. } => {
            payload["statusCode"] = serde_json::json!(status_code);
            payload["responseTimeMs"] = serde_json::json!(response_time_ms);
        }
        WatchResult::CommandRun { exit_code, .. } => {
            payload["exitCode"] = serde_json::json!(exit_code);
        }
        WatchResult::GithubPr { pr_number, state, title, url, draft, reviews, checks, .. } => {
            payload["prNumber"] = serde_json::json!(pr_number);
            payload["state"] = serde_json::Value::String(state.clone());
            payload["title"] = serde_json::Value::String(title.clone());
            payload["url"] = serde_json::Value::String(url.clone());
            payload["draft"] = serde_json::Value::Bool(*draft);
            let failing_checks = checks
                .iter()
                .filter(|check| {
                    matches!(
                        check.conclusion.as_deref(),
                        Some("failure") | Some("timed_out") | Some("cancelled") | Some("action_required")
                    )
                })
                .map(|check| check.name.clone())
                .collect::<Vec<_>>();
            let has_review_changes_requested = reviews
                .iter()
                .any(|review| review.state.eq_ignore_ascii_case("changes_requested"));
            payload["hasCiFailures"] = serde_json::Value::Bool(!failing_checks.is_empty());
            payload["failingChecks"] = serde_json::json!(failing_checks);
            payload["hasReviewChangesRequested"] =
                serde_json::Value::Bool(has_review_changes_requested);
            payload["reviewStates"] = serde_json::json!(
                reviews.iter().map(|review| review.state.clone()).collect::<Vec<_>>()
            );
        }
    }

    payload
}

fn watch_type_name(kind: &WatchKind) -> &'static str {
    match kind {
        WatchKind::GithubAction { .. } => "github_action",
        WatchKind::HttpHealth { .. } => "http_health",
        WatchKind::ShellCommand { .. } => "shell_command",
        WatchKind::Task { .. } => "task",
        WatchKind::GithubPr { .. } => "github_pr",
    }
}

fn watch_outcome_name(outcome: &WatchOutcome) -> &'static str {
    match outcome {
        WatchOutcome::Success => "success",
        WatchOutcome::Failure => "failure",
        WatchOutcome::InProgress => "in_progress",
    }
}

fn filter_matches(filter: &HookFilter, event: &serde_json::Value) -> bool {
    match filter {
        HookFilter::Equals { field, value } => {
            lookup_field(event, field).and_then(json_scalar_string) == Some(value.clone())
        }
        HookFilter::In { field, values } => lookup_field(event, field)
            .and_then(json_scalar_string)
            .is_some_and(|actual| values.iter().any(|expected| expected == &actual)),
        HookFilter::Exists { field } => lookup_field(event, field).is_some_and(|value| !value.is_null()),
    }
}

fn lookup_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in field.split('.').filter(|segment| !segment.is_empty()) {
        current = current.get(segment)?;
    }
    Some(current)
}

fn json_scalar_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{
        HookEventKind, HookFilter, HookRun, RouxEvent, RouxEventOrigin, RouxEventSession,
        RouxEventWorkspace, Session, SessionStatus,
    };

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn event_for_repo(repo_root: &std::path::Path) -> RouxEvent {
        RouxEvent {
            id: "evt-1".into(),
            kind: HookEventKind::WatchCompleted,
            timestamp: "now".into(),
            origin: RouxEventOrigin {
                kind: "cli".into(),
                causation_id: None,
                triggered_by_hook_id: None,
            },
            session: None,
            workspace: Some(RouxEventWorkspace {
                repo_root: Some(repo_root.display().to_string()),
                worktree_path: Some(repo_root.display().to_string()),
            }),
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn watch_events_include_completed_and_changed_payload() {
        let watch = Watch {
            id: "watch-1".into(),
            name: "PR watch".into(),
            kind: WatchKind::GithubPr { repo: "owner/repo".into(), pr_number: 42 },
            mode: WatchMode::Recurring { interval_secs: 60 },
            scope: WatchScope::Session { session_id: "sess-1".into() },
            runtime_state: roux_core::RuntimeState::Active,
            last_result: Some(WatchResult::GithubPr {
                pr_number: 42,
                state: "open".into(),
                title: "Fix hooks".into(),
                url: "https://example.test/pr/42".into(),
                head_sha: "abc123".into(),
                draft: false,
                reviews: vec![
                    roux_core::PrReview {
                        reviewer: "sam".into(),
                        state: "changes_requested".into(),
                        url: None,
                    },
                    roux_core::PrReview {
                        reviewer: "pat".into(),
                        state: "approved".into(),
                        url: None,
                    },
                ],
                checks: vec![
                    roux_core::PrCheckRun {
                        name: "ci / test".into(),
                        conclusion: Some("failure".into()),
                        url: None,
                    },
                    roux_core::PrCheckRun {
                        name: "ci / lint".into(),
                        conclusion: Some("success".into()),
                        url: None,
                    },
                ],
                outcome: WatchOutcome::Failure,
            }),
            last_checked: Some(12345),
            notify: Default::default(),
            created_at: 12000,
        };
        let session = Session {
            id: "sess-1".into(),
            name: "repo".into(),
            repo_root: "/tmp/repo".into(),
            worktree_path: "/tmp/repo/.worktrees/pr-42".into(),
            branch: "pr-42".into(),
            is_worktree: true,
            status: SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 999,
            project_id: None,
            is_git_repo: true,
            name_override: None,
        };

        let events = watch_events(&watch, Some(&WatchOutcome::InProgress), Some(&session));

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, HookEventKind::WatchCompleted);
        assert_eq!(events[1].kind, HookEventKind::WatchOutcomeChanged);
        assert_eq!(
            events[0].workspace.as_ref().and_then(|w| w.repo_root.as_deref()),
            Some("/tmp/repo")
        );
        assert_eq!(
            events[0].session.as_ref().and_then(|s| s.roux_session_id.as_deref()),
            Some("sess-1")
        );
        assert_eq!(events[0].payload["watchId"], "watch-1");
        assert_eq!(events[0].payload["watchType"], "github_pr");
        assert_eq!(events[0].payload["scopeType"], "session");
        assert_eq!(events[0].payload["outcome"], "failure");
        assert_eq!(events[0].payload["previousOutcome"], "in_progress");
        assert_eq!(events[0].payload["prNumber"], 42);
        assert_eq!(events[0].payload["repo"], "owner/repo");
        assert_eq!(events[0].payload["hasCiFailures"], true);
        assert_eq!(events[0].payload["hasReviewChangesRequested"], true);
        assert_eq!(events[0].payload["failingChecks"], serde_json::json!(["ci / test"]));
    }

    #[test]
    fn matching_hooks_applies_equals_in_and_exists_filters() {
        let event = RouxEvent {
            id: "evt-1".into(),
            kind: HookEventKind::WatchCompleted,
            timestamp: "now".into(),
            origin: RouxEventOrigin {
                kind: "cli".into(),
                causation_id: None,
                triggered_by_hook_id: None,
            },
            session: Some(RouxEventSession {
                roux_session_id: Some("sess-1".into()),
                pane_id: Some("pane-1".into()),
                profile: Some("codex".into()),
            }),
            workspace: Some(RouxEventWorkspace {
                repo_root: Some("/tmp/repo".into()),
                worktree_path: Some("/tmp/repo".into()),
            }),
            payload: serde_json::json!({
                "outcome": "failure",
                "watchId": "watch-1",
            }),
        };

        let config = HookConfig {
            hooks: vec![
                HookDefinition {
                    id: "match".into(),
                    on: vec![HookEventKind::WatchCompleted],
                    enabled: true,
                    filters: vec![
                        HookFilter::Equals {
                            field: "payload.outcome".into(),
                            value: "failure".into(),
                        },
                        HookFilter::In {
                            field: "session.profile".into(),
                            values: vec!["codex".into(), "claude".into()],
                        },
                        HookFilter::Exists { field: "workspace.repoRoot".into() },
                    ],
                    run: HookRun { command: "echo".into(), ..HookRun::default() },
                    policy: Default::default(),
                },
                HookDefinition {
                    id: "wrong-outcome".into(),
                    on: vec![HookEventKind::WatchCompleted],
                    enabled: true,
                    filters: vec![HookFilter::Equals {
                        field: "payload.outcome".into(),
                        value: "success".into(),
                    }],
                    run: HookRun { command: "echo".into(), ..HookRun::default() },
                    policy: Default::default(),
                },
                HookDefinition {
                    id: "wrong-profile".into(),
                    on: vec![HookEventKind::WatchCompleted],
                    enabled: true,
                    filters: vec![HookFilter::In {
                        field: "session.profile".into(),
                        values: vec!["plain-shell".into()],
                    }],
                    run: HookRun { command: "echo".into(), ..HookRun::default() },
                    policy: Default::default(),
                },
                HookDefinition {
                    id: "missing-field".into(),
                    on: vec![HookEventKind::WatchCompleted],
                    enabled: true,
                    filters: vec![HookFilter::Exists { field: "payload.missing".into() }],
                    run: HookRun { command: "echo".into(), ..HookRun::default() },
                    policy: Default::default(),
                },
            ],
        };

        let matched = matching_hooks(&config, &event);
        let matched_ids = matched.iter().map(|hook| hook.id.as_str()).collect::<Vec<_>>();
        assert_eq!(matched_ids, vec!["match"]);
    }

    #[test]
    fn loads_global_and_trusted_repo_hooks_and_matches_event_kind() {
        let cfg = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();

        write(
            &cfg.path().join("hooks.kdl"),
            r#"
                hooks {
                  hook "global-watch" {
                    on "watch.completed"
                    run { command "python3" }
                  }
                }
            "#,
        );
        write(
            &repo.path().join(".roux/hooks.kdl"),
            r#"
                hooks {
                  hook "repo-watch" {
                    on "watch.completed"
                    run { command "python3" }
                  }
                }
            "#,
        );

        let mut settings = crate::settings::RouxSettings::default();
        settings.trusted_workspaces = vec![repo.path().display().to_string()];
        let event = event_for_repo(repo.path());

        let loaded = load_hooks_for_event_in(cfg.path(), &settings, &event);
        assert!(loaded.errors.is_empty(), "unexpected load errors: {:?}", loaded.errors);
        assert_eq!(loaded.config.hooks.len(), 2);

        let matched = matching_hooks(&loaded.config, &event);
        let ids: Vec<&str> = matched.iter().map(|hook| hook.id.as_str()).collect();
        assert_eq!(ids, vec!["global-watch", "repo-watch"]);
    }

    #[test]
    fn builds_session_created_event_with_workspace_and_profile() {
        let session = crate::session::Session {
            id: "sess-1".into(),
            name: "Test Session".into(),
            repo_root: "/repo".into(),
            worktree_path: "/repo/.worktrees/test".into(),
            branch: "feature/x".into(),
            is_worktree: true,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 123,
            project_id: None,
            is_git_repo: true,
            name_override: None,
        };

        let event = session_created_event(&session, Some("codex"));
        assert_eq!(event.kind, HookEventKind::SessionCreated);
        assert_eq!(
            event.workspace.as_ref().and_then(|w| w.repo_root.as_deref()),
            Some("/repo")
        );
        assert_eq!(
            event.session.as_ref().and_then(|s| s.profile.as_deref()),
            Some("codex")
        );
        assert_eq!(event.payload["sessionId"], "sess-1");
        assert_eq!(event.payload["isWorktree"], true);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dispatch_runs_matching_hook_and_writes_event_to_stdin() {
        let cfg = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let out = repo.path().join("event.json");
        let script = repo.path().join("hook.sh");
        write(
            &script,
            "#!/usr/bin/env sh\ncat > \"$1\"\n",
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        write(
            &cfg.path().join("hooks.kdl"),
            &format!(
                r#"
                    hooks {{
                      hook "global-watch" {{
                        on "watch.completed"
                        run {{
                          command "sh"
                          arg "{}"
                          arg "{}"
                          timeout-ms 5000
                        }}
                      }}
                    }}
                "#,
                script.display(),
                out.display()
            ),
        );

        let settings = crate::settings::RouxSettings::default();
        let event = event_for_repo(repo.path());

        let result = dispatch_event_in(cfg.path(), &settings, &event).await;
        assert_eq!(result.matched_hook_ids, vec!["global-watch"]);
        assert_eq!(result.runs.len(), 1);
        assert_eq!(result.runs[0].exit_code, Some(0));
        assert!(!result.runs[0].timed_out);

        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.contains("\"kind\":\"watch.completed\""), "unexpected event json: {written}");
        assert!(
            written.contains(&format!("\"repoRoot\":\"{}\"", repo.path().display())),
            "repo root missing from event json: {written}"
        );
    }
}
