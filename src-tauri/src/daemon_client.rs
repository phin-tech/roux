use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::ipc::{Channel, Response as IpcResponse};
use tauri::{AppHandle, Emitter};

use crate::commands::notes::{NotesRead, NotesSearchQuery, NotesTarget};
use roux_core::{
    AgentAlias, BusSubscription, CreateWatchConfig, Event, Project, ProjectUpdate, ReadState,
    Session, SessionExitPayload, SessionExitReason, Watch, Worktree,
};
use roux_runtime::automation_hooks::{
    HookListItem, HookLogEntry, HookPreviewItem, HookRunRequest, HookRunSummary,
};
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};
use roux_runtime::terminal_env::NotesEnvInputs;
use roux_sdk::{
    AliasEventStreamFrame, MailboxEventStreamFrame, PtyAttachFrame, PtyRecord, PtySnapshot,
    SubscriptionEventStreamFrame, WatchEventStreamFrame, WorkItemEventStreamFrame,
};

use crate::platform;
use crate::watches::WatchManager;

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);
const EVENT_BRIDGE_RECONNECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonStatus {
    pub(crate) kind: String,
    pub(crate) pid: u32,
    pub(crate) socket: String,
    #[serde(default)]
    pub(crate) log_path: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) uptime_ms: u64,
    pub(crate) session_count: usize,
    pub(crate) project_count: usize,
    #[serde(default)]
    pub(crate) watch_count: usize,
    #[serde(default)]
    pub(crate) process_count: usize,
    #[serde(default)]
    pub(crate) pty_count: usize,
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonClient {
    status: DaemonStatus,
    sdk: roux_sdk::Roux,
}

#[derive(Debug, Clone)]
pub(crate) enum DaemonStartup {
    Connected(Box<DaemonClient>),
    LocalFallbackDisabled(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonCreateSessionShellRequest {
    pub(crate) id: String,
    pub(crate) repo_path: String,
    pub(crate) name: String,
    pub(crate) worktree_path: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) base: Option<String>,
    pub(crate) fetch_first: bool,
    pub(crate) profile: Option<String>,
    pub(crate) nono_profile: Option<String>,
    pub(crate) nono_allow_dirs: Vec<String>,
    pub(crate) initial_size: Option<(u16, u16)>,
    pub(crate) project_id: Option<String>,
    pub(crate) blueprint_id: Option<String>,
    pub(crate) smol_machine_name: Option<String>,
    pub(crate) notes: Option<NotesEnvInputs>,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonReconnectSessionShellRequest {
    pub(crate) id: String,
    pub(crate) profile: Option<String>,
    pub(crate) nono_profile: Option<String>,
    pub(crate) nono_allow_dirs: Vec<String>,
    pub(crate) initial_size: Option<(u16, u16)>,
    pub(crate) notes: Option<NotesEnvInputs>,
}

pub(crate) type DaemonMailboxPostRequest = roux_sdk::MailboxPost;

type DaemonClientResult<T> = std::result::Result<T, DaemonClientError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonClientError {
    #[error(transparent)]
    Sdk(#[from] roux_sdk::RouxError),
    #[error(transparent)]
    Decode(#[from] serde_json::Error),
    #[error("unexpected daemon kind: {0}")]
    UnexpectedDaemonKind(String),
    #[error("daemon pty not found")]
    DaemonPtyNotFound,
    #[error("{0}")]
    Adapter(String),
}

impl DaemonClientError {
    fn adapter(message: impl Into<String>) -> Self {
        Self::Adapter(message.into())
    }
}

impl From<DaemonClientError> for String {
    fn from(error: DaemonClientError) -> Self {
        error.to_string()
    }
}

impl DaemonClient {
    pub(crate) fn detect() -> Option<Self> {
        let sdk = roux_sdk::Roux::builder().timeout(PROBE_TIMEOUT).connect().ok()?;
        Self::detect_from_probe_sdk(sdk)
    }

    fn detect_from_probe_sdk(sdk: roux_sdk::Roux) -> Option<Self> {
        let data = sdk.status_blocking().ok()?;
        let status: DaemonStatus = serde_json::from_value(data).ok()?;
        Self::from_detected_status(status, sdk)
    }

    fn from_detected_status(status: DaemonStatus, sdk: roux_sdk::Roux) -> Option<Self> {
        if status.kind == "roux-daemon" {
            Some(Self { status, sdk: sdk.with_default_timeout() })
        } else {
            None
        }
    }

    pub(crate) fn ensure_local() -> DaemonStartup {
        if let Some(client) = Self::detect() {
            return DaemonStartup::Connected(Box::new(client));
        }

        if let Some(endpoint) = configured_socket_endpoint_that_blocks_autostart() {
            return DaemonStartup::Failed(format!(
                "ROUX_SOCKET is set to {endpoint}, but no daemon responded"
            ));
        }

        if let Some(reason) = daemon_autostart_disabled_reason() {
            return DaemonStartup::LocalFallbackDisabled(reason);
        }

        match launch_local_daemon() {
            Ok(started) => {
                rlog!("Started roux daemon pid={} from {}", started.pid, started.binary.display());
            }
            Err(err) => {
                return DaemonStartup::Failed(format!("unable to start roux daemon: {err}"));
            }
        }

        match wait_for_daemon(STARTUP_TIMEOUT, STARTUP_POLL_INTERVAL) {
            Some(client) => DaemonStartup::Connected(Box::new(client)),
            None => DaemonStartup::Failed(format!(
                "started roux daemon but it did not become ready within {}ms",
                STARTUP_TIMEOUT.as_millis()
            )),
        }
    }

    pub(crate) fn status(&self) -> &DaemonStatus {
        &self.status
    }

    pub(crate) fn supports(&self, capability: &str) -> bool {
        self.status.capabilities.iter().any(|candidate| candidate == capability)
    }

    pub(crate) async fn refresh_status(&self) -> DaemonClientResult<DaemonStatus> {
        let status = self.sdk.status_value().await.map_err(DaemonClientError::from)?;
        let status: DaemonStatus =
            serde_json::from_value(status).map_err(DaemonClientError::from)?;
        if status.kind == "roux-daemon" {
            Ok(status)
        } else {
            Err(DaemonClientError::UnexpectedDaemonKind(status.kind))
        }
    }

    pub(crate) async fn list_sessions(&self) -> DaemonClientResult<Vec<Session>> {
        self.sdk.sessions().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn get_session(&self, id: String) -> DaemonClientResult<Session> {
        self.sdk.get_session(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_projects(&self) -> DaemonClientResult<Vec<Project>> {
        self.sdk.projects().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn create_project(&self, name: String) -> DaemonClientResult<Project> {
        self.sdk.create_project(name).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn remove_project(&self, id: String) -> DaemonClientResult<()> {
        self.sdk.remove_project(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn rename_project(&self, id: String, name: String) -> DaemonClientResult<()> {
        self.sdk.rename_project(id, name).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn update_project(
        &self,
        id: String,
        patch: ProjectUpdate,
    ) -> DaemonClientResult<Project> {
        self.sdk.update_project(id, patch).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn work_item_list(
        &self,
        project_id: Option<String>,
    ) -> Result<Vec<roux_core::WorkItem>, String> {
        self.sdk.work_item_list(project_id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_create(
        &self,
        input: roux_core::WorkItemInput,
    ) -> Result<roux_core::WorkItem, String> {
        self.sdk.work_item_create(input).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_update(
        &self,
        id: String,
        input: roux_core::WorkItemInput,
    ) -> Result<roux_core::WorkItem, String> {
        self.sdk.work_item_update(id, input).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_move(
        &self,
        id: String,
        status: roux_core::WorkItemStatus,
        sort_order: f64,
    ) -> Result<roux_core::WorkItem, String> {
        self.sdk.work_item_move(id, status, sort_order).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_delete(&self, id: String) -> Result<String, String> {
        self.sdk.work_item_delete(id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_dispatch(
        &self,
        id: String,
        profile: Option<String>,
        repo_path: Option<String>,
        name: Option<String>,
        worktree_path: Option<String>,
        branch: Option<String>,
        base: Option<String>,
        fetch_first: Option<bool>,
    ) -> Result<String, String> {
        self.sdk
            .work_item_dispatch(
                id,
                profile,
                repo_path,
                name,
                worktree_path,
                branch,
                base,
                fetch_first,
            )
            .await
            .map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_run_dispatch(
        &self,
        id: String,
        profile: Option<String>,
        repo_path: Option<String>,
        name: Option<String>,
        worktree_path: Option<String>,
        branch: Option<String>,
        base: Option<String>,
        fetch_first: Option<bool>,
    ) -> Result<roux_core::WorkItemRun, String> {
        self.sdk
            .work_item_run_dispatch(
                id,
                profile,
                repo_path,
                name,
                worktree_path,
                branch,
                base,
                fetch_first,
            )
            .await
            .map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_runs_list(
        &self,
        work_item_id: Option<String>,
    ) -> Result<Vec<roux_core::WorkItemRun>, String> {
        self.sdk.work_item_runs_list(work_item_id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_run_events(
        &self,
        run_id: String,
    ) -> Result<Vec<roux_core::WorkItemRunEvent>, String> {
        self.sdk.work_item_run_events(run_id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_run_stop(
        &self,
        run_id: String,
    ) -> Result<roux_core::WorkItemRun, String> {
        self.sdk.work_item_run_stop(run_id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_decision_create(
        &self,
        run_id: String,
        question: String,
        options: Vec<roux_core::WorkItemDecisionOption>,
        default_value: Option<String>,
        timeout_at: Option<u64>,
    ) -> Result<roux_core::WorkItemDecision, String> {
        self.sdk
            .work_item_decision_create(run_id, question, options, default_value, timeout_at)
            .await
            .map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_decisions_list(
        &self,
        work_item_id: Option<String>,
    ) -> Result<Vec<roux_core::WorkItemDecision>, String> {
        self.sdk.work_item_decisions_list(work_item_id).await.map_err(|err| err.to_string())
    }

    pub(crate) async fn work_item_decision_resolve(
        &self,
        id: String,
        value: String,
        resolved_by: Option<String>,
    ) -> Result<roux_core::WorkItemDecision, String> {
        self.sdk
            .work_item_decision_resolve(id, value, resolved_by)
            .await
            .map_err(|err| err.to_string())
    }

    pub(crate) async fn list_aliases(
        &self,
        project_id: Option<String>,
        global: bool,
        only_unbound: bool,
    ) -> DaemonClientResult<Vec<AgentAlias>> {
        self.sdk
            .list_aliases(project_id, global, only_unbound)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn get_alias(
        &self,
        alias: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<Option<AgentAlias>> {
        self.sdk.get_alias(alias, project_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn whoami_aliases(
        &self,
        session_id: String,
    ) -> DaemonClientResult<Vec<AgentAlias>> {
        self.sdk.whoami_aliases(session_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn add_alias_member(
        &self,
        alias: String,
        pane_id: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<AgentAlias> {
        self.sdk.add_alias_member(alias, pane_id, project_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn remove_alias_member(
        &self,
        alias: String,
        pane_id: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<bool> {
        self.sdk
            .remove_alias_member(alias, pane_id, project_id)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn set_alias_mode(
        &self,
        alias: String,
        mode: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<AgentAlias> {
        self.sdk.set_alias_mode(alias, mode, project_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_subscriptions(
        &self,
        alias: Option<String>,
        project_id: Option<String>,
        global: bool,
    ) -> DaemonClientResult<Vec<BusSubscription>> {
        self.sdk
            .list_subscriptions(alias, project_id, global)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn create_subscription(
        &self,
        alias: String,
        pattern: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<BusSubscription> {
        self.sdk
            .create_subscription(alias, pattern, project_id)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn delete_subscription(&self, id: String) -> DaemonClientResult<bool> {
        self.sdk.delete_subscription(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_list_for_recipient(
        &self,
        alias: String,
        unread_only: bool,
        project_id: Option<String>,
        global: bool,
    ) -> DaemonClientResult<Vec<Event>> {
        self.sdk
            .mailbox_list_for_recipient(alias, unread_only, project_id, global)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_list_for_topic(
        &self,
        topic: String,
        project_id: Option<String>,
        global: bool,
    ) -> DaemonClientResult<Vec<Event>> {
        self.sdk
            .mailbox_list_for_topic(topic, project_id, global)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_list_all(
        &self,
        project_id: Option<String>,
        global: bool,
        limit: Option<u32>,
    ) -> DaemonClientResult<Vec<Event>> {
        self.sdk.mailbox_list_all(project_id, global, limit).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_unread_count(
        &self,
        alias: String,
        project_id: Option<String>,
        global: bool,
    ) -> DaemonClientResult<u32> {
        self.sdk
            .mailbox_unread_count(alias, project_id, global)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_get_event(
        &self,
        event_id: String,
    ) -> DaemonClientResult<Option<Event>> {
        self.sdk.mailbox_get_event(event_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_read_state(
        &self,
        event_id: String,
        recipient: String,
    ) -> DaemonClientResult<Option<ReadState>> {
        self.sdk.mailbox_read_state(event_id, recipient).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_post(
        &self,
        request: DaemonMailboxPostRequest,
    ) -> DaemonClientResult<Event> {
        self.sdk.mailbox_post(request).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_mark_read(
        &self,
        event_id: String,
        recipient: String,
    ) -> DaemonClientResult<bool> {
        self.sdk.mailbox_mark_read(event_id, recipient).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_ack(
        &self,
        event_id: String,
        recipient: String,
        result: Option<String>,
    ) -> DaemonClientResult<bool> {
        self.sdk.mailbox_ack(event_id, recipient, result).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_clear_read(
        &self,
        recipient: String,
        project_id: Option<String>,
        global: bool,
    ) -> DaemonClientResult<u32> {
        self.sdk
            .mailbox_clear_read(recipient, project_id, global)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_retract(
        &self,
        event_id: String,
        sender: String,
    ) -> DaemonClientResult<Event> {
        self.sdk.mailbox_retract(event_id, sender).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn mailbox_dismiss(
        &self,
        event_id: String,
        recipient: String,
    ) -> DaemonClientResult<bool> {
        self.sdk.mailbox_dismiss(event_id, recipient).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn read_notes(&self, target: NotesTarget) -> DaemonClientResult<NotesRead> {
        self.sdk.read_notes(target).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn write_notes(
        &self,
        target: NotesTarget,
        content: String,
        tags: Vec<String>,
    ) -> DaemonClientResult<()> {
        self.sdk.write_notes(target, content, tags).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn append_notes(
        &self,
        target: NotesTarget,
        content: String,
        timestamped: bool,
        tags: Vec<String>,
    ) -> DaemonClientResult<()> {
        self.sdk
            .append_notes(target, content, timestamped, tags)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn notes_path(
        &self,
        target: NotesTarget,
        dir: bool,
    ) -> DaemonClientResult<String> {
        self.sdk.notes_path(target, dir).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn search_notes(
        &self,
        query: NotesSearchQuery,
    ) -> DaemonClientResult<Vec<String>> {
        self.sdk.search_notes(query).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn notes_vault_root(&self) -> DaemonClientResult<String> {
        self.sdk.notes_vault_root().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_automation_hooks(
        &self,
        repo_path: Option<String>,
    ) -> DaemonClientResult<Vec<HookListItem>> {
        self.sdk.list_automation_hooks(repo_path).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn preview_automation_hooks(
        &self,
        request: HookRunRequest,
    ) -> DaemonClientResult<Vec<HookPreviewItem>> {
        self.sdk.preview_automation_hooks(request).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn run_automation_hook(
        &self,
        request: HookRunRequest,
    ) -> DaemonClientResult<HookRunSummary> {
        self.sdk.run_automation_hook(request).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn approve_automation_hook(
        &self,
        approval_id: String,
    ) -> DaemonClientResult<()> {
        self.sdk.approve_automation_hook(approval_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn clear_automation_hook_approvals(&self) -> DaemonClientResult<()> {
        self.sdk.clear_automation_hook_approvals().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_automation_hook_logs(&self) -> DaemonClientResult<Vec<HookLogEntry>> {
        self.sdk.list_automation_hook_logs().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn read_automation_hook_log(
        &self,
        path: String,
    ) -> DaemonClientResult<String> {
        self.sdk.read_automation_hook_log(path).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn set_session_name_override(
        &self,
        session_id: String,
        name_override: Option<String>,
    ) -> DaemonClientResult<()> {
        self.sdk
            .set_session_name_override(session_id, name_override)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn set_session_project(
        &self,
        session_id: String,
        project_id: Option<String>,
    ) -> DaemonClientResult<()> {
        self.sdk.set_session_project(session_id, project_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn set_session_pinned_pr_url(
        &self,
        session_id: String,
        url: Option<String>,
    ) -> DaemonClientResult<()> {
        self.sdk.set_session_pinned_pr_url(session_id, url).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn set_session_smol_machine(
        &self,
        session_id: String,
        machine_name: Option<String>,
    ) -> DaemonClientResult<()> {
        self.sdk
            .set_session_smol_machine(session_id, machine_name)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn create_session_shell(
        &self,
        request: DaemonCreateSessionShellRequest,
    ) -> DaemonClientResult<Session> {
        self.sdk
            .create_session_shell(roux_sdk::CreateSessionShell {
                id: request.id,
                repo_path: request.repo_path,
                name: request.name,
                worktree_path: request.worktree_path,
                branch: request.branch,
                base: request.base,
                fetch_first: request.fetch_first,
                profile: request.profile,
                nono_profile: request.nono_profile,
                nono_allow_dirs: request.nono_allow_dirs,
                initial_size: request.initial_size,
                project_id: request.project_id,
                blueprint_id: request.blueprint_id,
                smol_machine_name: request.smol_machine_name,
                notes: request.notes.map(sdk_notes_env),
            })
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn reconnect_session_shell(
        &self,
        request: DaemonReconnectSessionShellRequest,
    ) -> DaemonClientResult<Session> {
        self.sdk
            .reconnect_session_shell(roux_sdk::ReconnectSessionShell {
                id: request.id,
                profile: request.profile,
                nono_profile: request.nono_profile,
                nono_allow_dirs: request.nono_allow_dirs,
                initial_size: request.initial_size,
                notes: request.notes.map(sdk_notes_env),
            })
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn archive_session(&self, id: String) -> DaemonClientResult<Session> {
        self.sdk.archive_session(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn restore_session(&self, id: String) -> DaemonClientResult<Session> {
        self.sdk.restore_session(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn delete_session(&self, id: String) -> DaemonClientResult<()> {
        self.sdk.delete_session(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn session_worktree_exists(&self, id: String) -> DaemonClientResult<bool> {
        self.sdk.session_worktree_exists(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn refresh_session_branch(
        &self,
        id: String,
    ) -> DaemonClientResult<Option<String>> {
        self.sdk.refresh_session_branch(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_worktrees(
        &self,
        repo_path: String,
    ) -> DaemonClientResult<Vec<Worktree>> {
        self.sdk.list_worktrees(repo_path).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn create_worktree(
        &self,
        repo_path: String,
        branch: String,
        start_point: Option<String>,
        fetch_first: bool,
    ) -> DaemonClientResult<String> {
        self.sdk
            .create_worktree(repo_path, branch, start_point, fetch_first)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn remove_worktree(
        &self,
        repo_path: String,
        worktree_path: String,
        also_branch: bool,
        force: bool,
    ) -> DaemonClientResult<()> {
        self.sdk
            .remove_worktree(repo_path, worktree_path, also_branch, force)
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_branches(&self, repo_path: String) -> DaemonClientResult<Vec<String>> {
        self.sdk.list_branches(repo_path).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn git_init(&self, path: String) -> DaemonClientResult<()> {
        self.sdk.git_init(path).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_watches(&self) -> DaemonClientResult<Vec<Watch>> {
        self.sdk.watches().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn create_watch(
        &self,
        config: CreateWatchConfig,
    ) -> DaemonClientResult<Watch> {
        self.sdk.create_watch(config).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn find_or_create_watch(
        &self,
        config: CreateWatchConfig,
    ) -> DaemonClientResult<Watch> {
        self.sdk.find_or_create_watch(config).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn remove_watch(&self, id: String) -> DaemonClientResult<()> {
        self.sdk.remove_watch(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn pause_watch(&self, id: String) -> DaemonClientResult<Watch> {
        self.sdk.pause_watch(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn resume_watch(&self, id: String) -> DaemonClientResult<Watch> {
        self.sdk.resume_watch(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn replace_watch(&self, watch: Watch) -> DaemonClientResult<()> {
        self.sdk.replace_watch(watch).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn remove_watches_for_session(
        &self,
        session_id: String,
    ) -> DaemonClientResult<()> {
        self.sdk.remove_watches_for_session(session_id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn cleanup_watch_orphans(&self) -> DaemonClientResult<()> {
        self.sdk.cleanup_watch_orphans().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn start_daemon_process(
        &self,
        command: String,
        working_dir: Option<String>,
    ) -> DaemonClientResult<ProcessRecord> {
        self.sdk.start_daemon_process(command, working_dir).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn daemon_process_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> DaemonClientResult<ProcessSnapshot> {
        self.sdk.daemon_process_output(id, max_bytes).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_daemon_processes(&self) -> DaemonClientResult<Vec<ProcessRecord>> {
        self.sdk.list_daemon_processes().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn kill_daemon_process(
        &self,
        id: String,
    ) -> DaemonClientResult<ProcessRecord> {
        self.sdk.kill_daemon_process(id).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn spawn_daemon_pty_shell(
        &self,
        id: Option<String>,
        working_dir: Option<String>,
        session_id: Option<String>,
        pane_id: Option<String>,
        profile: Option<String>,
        nono_profile: Option<String>,
        nono_allow_dirs: Vec<String>,
        initial_size: Option<(u16, u16)>,
    ) -> DaemonClientResult<PtyRecord> {
        let mut spawn = self.sdk.spawn_shell();
        if let Some(id) = id {
            spawn = spawn.id(id);
        }
        if let Some(working_dir) = working_dir {
            spawn = spawn.working_dir(working_dir);
        }
        if let Some(session_id) = session_id {
            spawn = spawn.session_id(session_id);
        }
        if let Some(pane_id) = pane_id {
            spawn = spawn.pane_id(pane_id);
        }
        if let Some(profile) = profile {
            spawn = spawn.profile(profile);
        }
        if let Some(nono_profile) = nono_profile {
            spawn = spawn.nono_profile(nono_profile);
        }
        if !nono_allow_dirs.is_empty() {
            spawn = spawn.nono_allow_dirs(nono_allow_dirs);
        }
        if let Some((cols, rows)) = initial_size {
            spawn = spawn.initial_size(cols, rows);
        }
        spawn.spawn_record().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn spawn_daemon_pty_task(
        &self,
        command: String,
        id: Option<String>,
        working_dir: Option<String>,
        session_id: Option<String>,
        pane_id: Option<String>,
        profile: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> DaemonClientResult<PtyRecord> {
        let mut spawn = self.sdk.spawn_task(command);
        if let Some(id) = id {
            spawn = spawn.id(id);
        }
        if let Some(working_dir) = working_dir {
            spawn = spawn.working_dir(working_dir);
        }
        if let Some(session_id) = session_id {
            spawn = spawn.session_id(session_id);
        }
        if let Some(pane_id) = pane_id {
            spawn = spawn.pane_id(pane_id);
        }
        if let Some(profile) = profile {
            spawn = spawn.profile(profile);
        }
        if let Some((cols, rows)) = initial_size {
            spawn = spawn.initial_size(cols, rows);
        }
        spawn.spawn_record().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn daemon_pty_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> DaemonClientResult<PtySnapshot> {
        self.sdk
            .pty(id)
            .snapshot(max_bytes.unwrap_or(roux_runtime::pty_service::PTY_OUTPUT_DEFAULT_POLL_BYTES))
            .await
            .map_err(DaemonClientError::from)
    }

    pub(crate) async fn list_daemon_ptys(&self) -> DaemonClientResult<Vec<PtyRecord>> {
        self.sdk.ptys().await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn write_daemon_pty(
        &self,
        id: String,
        data: String,
    ) -> DaemonClientResult<()> {
        self.sdk.pty(id).write(data).await.map(|_| ()).map_err(DaemonClientError::from)
    }

    pub(crate) async fn resize_daemon_pty(
        &self,
        id: String,
        cols: u16,
        rows: u16,
    ) -> DaemonClientResult<PtyRecord> {
        self.sdk.pty(id).resize(cols, rows).await.map_err(DaemonClientError::from)
    }

    pub(crate) async fn kill_daemon_pty(&self, id: String) -> DaemonClientResult<PtyRecord> {
        self.sdk
            .pty(id)
            .kill()
            .await
            .map_err(DaemonClientError::from)?
            .ok_or(DaemonClientError::DaemonPtyNotFound)
    }

    pub(crate) async fn detach_daemon_pty(&self, id: String) -> DaemonClientResult<PtyRecord> {
        self.sdk
            .pty(id)
            .detach()
            .await
            .map_err(DaemonClientError::from)?
            .ok_or(DaemonClientError::DaemonPtyNotFound)
    }

    pub(crate) async fn attach_daemon_pty_to_pane(
        &self,
        id: String,
        pane_id: String,
    ) -> DaemonClientResult<PtyRecord> {
        self.sdk
            .pty(id)
            .attach_to_pane(pane_id)
            .await
            .map_err(DaemonClientError::from)?
            .ok_or(DaemonClientError::DaemonPtyNotFound)
    }

    pub(crate) async fn mark_daemon_pty_read(&self, id: String) -> DaemonClientResult<PtyRecord> {
        self.sdk
            .pty(id)
            .mark_read()
            .await
            .map_err(DaemonClientError::from)?
            .ok_or(DaemonClientError::DaemonPtyNotFound)
    }

    pub(crate) async fn set_daemon_pty_name(
        &self,
        id: String,
        name: Option<String>,
    ) -> DaemonClientResult<PtyRecord> {
        self.sdk
            .pty(id)
            .set_name(name)
            .await
            .map_err(DaemonClientError::from)?
            .ok_or(DaemonClientError::DaemonPtyNotFound)
    }

    pub(crate) fn spawn_daemon_pty_output_bridge(
        &self,
        id: String,
        channel: Channel<IpcResponse>,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        let pty = self.sdk.pty(id.clone());
        tauri::async_runtime::spawn(async move {
            let log_id = id.clone();
            let mut sent_until = 0_u64;
            let result = pty
                .attach(roux_runtime::pty_service::PTY_OUTPUT_LIMIT_BYTES, move |frame| {
                    match handle_sdk_pty_attach_frame(&id, frame, &channel, &app, &mut sent_until) {
                        Ok(keep_reading) => keep_reading,
                        Err(err) => {
                            rlog!("Daemon PTY output bridge for {id} stopped: {err}");
                            false
                        }
                    }
                })
                .await;
            if let Err(err) = result {
                rlog!("Daemon PTY output bridge for {log_id} stopped: {err}");
            }
        })
    }

    pub(crate) fn spawn_watch_event_bridge(
        &self,
        app: AppHandle,
        watch_manager: WatchManager,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            run_reconnecting_resolved_event_bridge(
                "watch",
                connect_current_sdk,
                move |sdk| read_watch_events_blocking(&sdk, app.clone(), watch_manager.clone()),
                std::thread::sleep,
                None,
            );
        })
    }

    pub(crate) fn spawn_mailbox_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            run_reconnecting_resolved_event_bridge(
                "mailbox",
                connect_current_sdk,
                move |sdk| read_mailbox_events_blocking(&sdk, app.clone()),
                std::thread::sleep,
                None,
            );
        })
    }

    pub(crate) fn spawn_alias_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            run_reconnecting_resolved_event_bridge(
                "alias",
                connect_current_sdk,
                move |sdk| read_alias_events_blocking(&sdk, app.clone()),
                std::thread::sleep,
                None,
            );
        })
    }

    pub(crate) fn spawn_subscription_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            run_reconnecting_resolved_event_bridge(
                "subscription",
                connect_current_sdk,
                move |sdk| read_subscription_events_blocking(&sdk, app.clone()),
                std::thread::sleep,
                None,
            );
        })
    }

    pub(crate) fn spawn_work_item_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            run_reconnecting_resolved_event_bridge(
                "work-item",
                connect_current_sdk,
                move |sdk| read_work_item_events_blocking(&sdk, app.clone()),
                std::thread::sleep,
                None,
            );
        })
    }
}

fn connect_current_sdk() -> DaemonClientResult<roux_sdk::Roux> {
    roux_sdk::Roux::connect().map_err(DaemonClientError::from)
}

fn run_reconnecting_resolved_event_bridge<C, T, F, S>(
    label: &'static str,
    mut resolve: C,
    mut read_once: F,
    mut sleep: S,
    max_attempts: Option<usize>,
) where
    C: FnMut() -> DaemonClientResult<T>,
    F: FnMut(T) -> DaemonClientResult<()>,
    S: FnMut(Duration),
{
    let mut attempts = 0_usize;
    loop {
        attempts += 1;
        match resolve().and_then(&mut read_once) {
            Ok(()) => rlog!("Daemon {label} event bridge disconnected; reconnecting"),
            Err(err) => rlog!("Daemon {label} event bridge stopped: {err}; reconnecting"),
        }
        if max_attempts.is_some_and(|max| attempts >= max) {
            break;
        }
        sleep(EVENT_BRIDGE_RECONNECT_DELAY);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartedDaemon {
    binary: PathBuf,
    pid: u32,
}

fn launch_local_daemon() -> DaemonClientResult<StartedDaemon> {
    let binary = resolve_daemon_binary()?;
    let mut child = daemon_spawn_command(&binary).spawn().map_err(|err| {
        DaemonClientError::adapter(format!("spawn {} daemon: {err}", binary.display()))
    })?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(StartedDaemon { binary, pid })
}

fn wait_for_daemon(timeout: Duration, interval: Duration) -> Option<DaemonClient> {
    let started = std::time::Instant::now();
    loop {
        if let Some(client) = DaemonClient::detect() {
            return Some(client);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(interval);
    }
}

fn sdk_notes_env(notes: NotesEnvInputs) -> roux_sdk::NotesEnv {
    roux_sdk::NotesEnv {
        vault_root: notes.vault_root,
        session_slug: notes.session_slug,
        repo_slug: notes.repo_slug,
        project_slug: notes.project_slug,
        context_paths: notes.context_paths,
        project_prompt: notes.project_prompt,
    }
}

fn daemon_autostart_disabled_reason() -> Option<String> {
    daemon_autostart_disabled_reason_for(std::env::var("ROUX_DAEMON_AUTOSTART").ok().as_deref())
}

fn daemon_autostart_disabled_reason_for(autostart: Option<&str>) -> Option<String> {
    if autostart.and_then(parse_env_enabled) == Some(false) {
        return Some("ROUX_DAEMON_AUTOSTART disabled local daemon startup".to_string());
    }
    None
}

fn configured_socket_endpoint_that_blocks_autostart() -> Option<String> {
    configured_socket_endpoint_that_blocks_autostart_for(
        std::env::var("ROUX_SOCKET").ok().as_deref(),
    )
}

fn configured_socket_endpoint_that_blocks_autostart_for(raw: Option<&str>) -> Option<String> {
    let endpoint = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(platform::parse_socket_endpoint)?;
    match &endpoint {
        platform::SocketEndpoint::Unix(path) if path == &platform::socket_path() => None,
        _ => Some(endpoint.display_value()),
    }
}

fn parse_env_enabled(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

fn resolve_daemon_binary() -> DaemonClientResult<PathBuf> {
    let current_exe = std::env::current_exe().ok();
    resolve_daemon_binary_from(current_exe.as_deref()).ok_or_else(|| {
        DaemonClientError::adapter(format!(
            "{} not found next to the desktop binary or on PATH",
            platform::roux_cli_file_name()
        ))
    })
}

fn resolve_daemon_binary_from(current_exe: Option<&Path>) -> Option<PathBuf> {
    current_exe
        .and_then(platform::sibling_roux_cli_path)
        .filter(|path| path.is_file())
        .or_else(|| platform::find_executable_on_path(platform::roux_cli_file_name()))
}

fn daemon_spawn_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
    #[cfg(not(windows))]
    {
        command.env("PATH", crate::pty::get_user_path());
    }

    command
}

fn read_watch_events_blocking(
    sdk: &roux_sdk::Roux,
    app: AppHandle,
    watch_manager: WatchManager,
) -> DaemonClientResult<()> {
    let mut stream_error = None;
    let result = sdk.watch_events_blocking(true, |frame| {
        match handle_watch_event_frame(frame, &app, &watch_manager) {
            Ok(()) => true,
            Err(err) => {
                stream_error = Some(err);
                false
            }
        }
    });
    stream_error.map_or_else(|| result.map_err(DaemonClientError::from), Err)
}

fn handle_watch_event_frame(
    frame: WatchEventStreamFrame,
    app: &AppHandle,
    watch_manager: &WatchManager,
) -> DaemonClientResult<()> {
    match frame {
        WatchEventStreamFrame::Ready => Ok(()),
        WatchEventStreamFrame::Update { event } => {
            let app = app.clone();
            let watch_manager = watch_manager.clone();
            tauri::async_runtime::spawn(async move {
                watch_manager.apply_daemon_watch_update(*event, app).await;
            });
            Ok(())
        }
        WatchEventStreamFrame::Warning { message } => {
            rlog!("Daemon watch event stream warning: {message}");
            Ok(())
        }
        WatchEventStreamFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

fn read_mailbox_events_blocking(sdk: &roux_sdk::Roux, app: AppHandle) -> DaemonClientResult<()> {
    let mut stream_error = None;
    let result =
        sdk.mailbox_events_blocking(|frame| match handle_mailbox_event_frame(frame, &app) {
            Ok(()) => true,
            Err(err) => {
                stream_error = Some(err);
                false
            }
        });
    stream_error.map_or_else(|| result.map_err(DaemonClientError::from), Err)
}

fn handle_mailbox_event_frame(
    frame: MailboxEventStreamFrame,
    app: &AppHandle,
) -> DaemonClientResult<()> {
    match frame {
        MailboxEventStreamFrame::Ready => Ok(()),
        MailboxEventStreamFrame::Event { event } => app
            .emit(roux_lib::mailbox::MAILBOX_EVENT, event.as_ref())
            .map_err(|err| DaemonClientError::adapter(format!("emit daemon mailbox event: {err}"))),
        MailboxEventStreamFrame::Warning { message } => {
            rlog!("Daemon mailbox event stream warning: {message}");
            Ok(())
        }
        MailboxEventStreamFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

fn read_alias_events_blocking(sdk: &roux_sdk::Roux, app: AppHandle) -> DaemonClientResult<()> {
    let mut stream_error = None;
    let result = sdk.alias_events_blocking(|frame| match handle_alias_event_frame(frame, &app) {
        Ok(()) => true,
        Err(err) => {
            stream_error = Some(err);
            false
        }
    });
    stream_error.map_or_else(|| result.map_err(DaemonClientError::from), Err)
}

fn handle_alias_event_frame(
    frame: AliasEventStreamFrame,
    app: &AppHandle,
) -> DaemonClientResult<()> {
    match frame {
        AliasEventStreamFrame::Ready => Ok(()),
        AliasEventStreamFrame::Event { event } => app
            .emit(roux_lib::aliases::ALIAS_EVENT, &event)
            .map_err(|err| DaemonClientError::adapter(format!("emit daemon alias event: {err}"))),
        AliasEventStreamFrame::Warning { message } => {
            rlog!("Daemon alias event stream warning: {message}");
            Ok(())
        }
        AliasEventStreamFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

fn read_subscription_events_blocking(
    sdk: &roux_sdk::Roux,
    app: AppHandle,
) -> DaemonClientResult<()> {
    let mut stream_error = None;
    let result = sdk.subscription_events_blocking(|frame| {
        match handle_subscription_event_frame(frame, &app) {
            Ok(()) => true,
            Err(err) => {
                stream_error = Some(err);
                false
            }
        }
    });
    stream_error.map_or_else(|| result.map_err(DaemonClientError::from), Err)
}

fn handle_subscription_event_frame(
    frame: SubscriptionEventStreamFrame,
    app: &AppHandle,
) -> DaemonClientResult<()> {
    match frame {
        SubscriptionEventStreamFrame::Ready => Ok(()),
        SubscriptionEventStreamFrame::Event { event } => {
            app.emit(roux_lib::subscriptions::SUBSCRIPTION_EVENT, &event).map_err(|err| {
                DaemonClientError::adapter(format!("emit daemon subscription event: {err}"))
            })
        }
        SubscriptionEventStreamFrame::Warning { message } => {
            rlog!("Daemon subscription event stream warning: {message}");
            Ok(())
        }
        SubscriptionEventStreamFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

pub(crate) const WORK_ITEM_EVENT: &str = "work-item-event";

fn read_work_item_events_blocking(sdk: &roux_sdk::Roux, app: AppHandle) -> DaemonClientResult<()> {
    let mut stream_error = None;
    let result =
        sdk.work_item_events_blocking(|frame| match handle_work_item_event_frame(frame, &app) {
            Ok(()) => true,
            Err(err) => {
                stream_error = Some(err);
                false
            }
        });
    stream_error.map_or_else(|| result.map_err(DaemonClientError::from), Err)
}

fn handle_work_item_event_frame(
    frame: WorkItemEventStreamFrame,
    app: &AppHandle,
) -> DaemonClientResult<()> {
    match frame {
        WorkItemEventStreamFrame::Ready => Ok(()),
        WorkItemEventStreamFrame::Event { event } => {
            app.emit(WORK_ITEM_EVENT, &event).map_err(|err| {
                DaemonClientError::adapter(format!("emit daemon work-item event: {err}"))
            })
        }
        WorkItemEventStreamFrame::Warning { message } => {
            rlog!("Daemon work-item event stream warning: {message}");
            Ok(())
        }
        WorkItemEventStreamFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

fn handle_sdk_pty_attach_frame(
    id: &str,
    frame: PtyAttachFrame,
    channel: &Channel<IpcResponse>,
    app: &AppHandle,
    sent_until: &mut u64,
) -> DaemonClientResult<bool> {
    match frame {
        PtyAttachFrame::Ready { replay_offset, replay_bytes, .. } => {
            let replay_end = replay_offset.saturating_add(replay_bytes.len() as u64);
            if !replay_bytes.is_empty() {
                channel.send(IpcResponse::new(replay_bytes)).map_err(|err| {
                    DaemonClientError::adapter(format!("send daemon pty replay to frontend: {err}"))
                })?;
            }
            *sent_until = (*sent_until).max(replay_end);
            Ok(true)
        }
        PtyAttachFrame::Output { offset, bytes } => {
            let frame_end = offset.saturating_add(bytes.len() as u64);
            if frame_end <= *sent_until {
                return Ok(true);
            }
            let start = if offset < *sent_until { (*sent_until - offset) as usize } else { 0 };
            let bytes = bytes[start..].to_vec();
            if !bytes.is_empty() {
                channel.send(IpcResponse::new(bytes)).map_err(|err| {
                    DaemonClientError::adapter(format!("send daemon pty output to frontend: {err}"))
                })?;
            }
            *sent_until = (*sent_until).max(frame_end);
            Ok(true)
        }
        PtyAttachFrame::Exit { code, generation } => {
            emit_daemon_pty_exit(app, id, code, generation);
            Ok(false)
        }
        PtyAttachFrame::Error { error } => Err(DaemonClientError::adapter(error)),
    }
}

fn emit_daemon_pty_exit(app: &AppHandle, id: &str, code: Option<i32>, generation: u64) {
    let code = code.and_then(|code| u32::try_from(code).ok());
    let _ = app.emit(
        &format!("session-exit:{id}"),
        &SessionExitPayload { code, generation, reason: SessionExitReason::Exit },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn daemon_status() -> DaemonStatus {
        DaemonStatus {
            kind: "roux-daemon".to_string(),
            pid: 1,
            socket: "unix:///tmp/roux.sock".to_string(),
            log_path: None,
            started_at_ms: 1,
            uptime_ms: 2,
            session_count: 0,
            project_count: 0,
            watch_count: 0,
            process_count: 0,
            pty_count: 0,
            capabilities: vec!["daemon-status".to_string()],
        }
    }

    #[test]
    fn event_bridge_reconnects_after_eof_and_error() {
        let calls = Cell::new(0);
        let sleeps = Cell::new(0);
        let mut results =
            vec![Ok(()), Err(DaemonClientError::adapter("socket closed")), Ok(())].into_iter();

        run_reconnecting_resolved_event_bridge(
            "test",
            || Ok(()),
            |()| {
                calls.set(calls.get() + 1);
                results.next().unwrap()
            },
            |_| sleeps.set(sleeps.get() + 1),
            Some(3),
        );

        assert_eq!(calls.get(), 3);
        assert_eq!(sleeps.get(), 2);
    }

    #[test]
    fn event_bridge_resolves_sdk_for_each_reconnect_attempt() {
        let resolves = Cell::new(0);
        let reads = RefCell::new(Vec::new());
        let sleeps = Cell::new(0);
        let mut results =
            vec![Ok(()), Err(DaemonClientError::adapter("socket closed")), Ok(())].into_iter();

        run_reconnecting_resolved_event_bridge(
            "test",
            || {
                let attempt = resolves.get() + 1;
                resolves.set(attempt);
                Ok(attempt)
            },
            |attempt| {
                reads.borrow_mut().push(attempt);
                results.next().unwrap()
            },
            |_| sleeps.set(sleeps.get() + 1),
            Some(3),
        );

        assert_eq!(resolves.get(), 3);
        assert_eq!(*reads.borrow(), vec![1, 2, 3]);
        assert_eq!(sleeps.get(), 2);
    }

    #[test]
    fn detected_client_uses_default_timeout_after_probe() {
        let probe_sdk = roux_sdk::Roux::builder()
            .endpoint(roux_sdk::SocketEndpoint::Unix(std::path::PathBuf::from(
                "/tmp/roux-probe.sock",
            )))
            .timeout(PROBE_TIMEOUT)
            .connect()
            .unwrap();
        let client = DaemonClient::from_detected_status(daemon_status(), probe_sdk).unwrap();

        assert_eq!(client.sdk.request_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn daemon_client_error_stringification_preserves_boundary_messages() {
        assert_eq!(
            DaemonClientError::UnexpectedDaemonKind("other".to_string()).to_string(),
            "unexpected daemon kind: other"
        );
        assert_eq!(String::from(DaemonClientError::DaemonPtyNotFound), "daemon pty not found");
        assert_eq!(DaemonClientError::adapter("socket closed").to_string(), "socket closed");
        assert_eq!(
            DaemonClientError::from(roux_sdk::RouxError::NotRunning).to_string(),
            "Roux is not running"
        );
    }

    #[test]
    fn daemon_autostart_policy_requires_explicit_opt_out() {
        assert_eq!(daemon_autostart_disabled_reason_for(None), None);
        assert_eq!(daemon_autostart_disabled_reason_for(Some("1")), None);

        assert!(daemon_autostart_disabled_reason_for(Some("0"))
            .unwrap()
            .contains("ROUX_DAEMON_AUTOSTART"));
    }

    #[test]
    fn daemon_autostart_allows_default_socket_env() {
        let default_socket = platform::socket_path().to_string_lossy().into_owned();
        assert_eq!(configured_socket_endpoint_that_blocks_autostart_for(None), None);
        assert_eq!(
            configured_socket_endpoint_that_blocks_autostart_for(Some(&default_socket)),
            None
        );
        assert_eq!(
            configured_socket_endpoint_that_blocks_autostart_for(Some(&format!(
                "unix://{default_socket}"
            ))),
            None
        );

        assert_eq!(
            configured_socket_endpoint_that_blocks_autostart_for(Some("tcp://127.0.0.1:7777")),
            Some("tcp://127.0.0.1:7777".to_string())
        );
        assert_eq!(
            configured_socket_endpoint_that_blocks_autostart_for(Some("/tmp/roux-other.sock")),
            Some("/tmp/roux-other.sock".to_string())
        );
    }

    #[test]
    fn parse_env_enabled_accepts_common_false_values() {
        assert_eq!(parse_env_enabled("0"), Some(false));
        assert_eq!(parse_env_enabled("false"), Some(false));
        assert_eq!(parse_env_enabled("off"), Some(false));
        assert_eq!(parse_env_enabled("yes"), Some(true));
        assert_eq!(parse_env_enabled(""), None);
    }

    #[test]
    fn resolve_daemon_binary_prefers_sibling_cli() {
        let dir = tempfile::tempdir().unwrap();
        let desktop = dir.path().join("roux-desktop");
        let cli = dir.path().join(platform::roux_cli_file_name());
        std::fs::write(&desktop, "").unwrap();
        std::fs::write(&cli, "").unwrap();

        assert_eq!(resolve_daemon_binary_from(Some(&desktop)), Some(cli));
    }
}
