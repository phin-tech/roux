use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::ipc::{Channel, Response as IpcResponse};
use tauri::{AppHandle, Emitter};

use roux_core::{
    AgentAlias, AliasEvent, BusSubscription, BusSubscriptionEvent, CreateWatchConfig, Event,
    MailboxEvent, Project, ReadState, Session, SessionExitPayload, SessionExitReason, Watch,
    WatchUpdateEvent, Worktree,
};
use roux_runtime::process_service::{ProcessRecord, ProcessSnapshot};
use roux_runtime::pty_service::{PtyRecord, PtySnapshot};
use roux_runtime::terminal_env::NotesEnvInputs;

use crate::platform;
use crate::watches::WatchManager;

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
    pub(crate) initial_size: Option<(u16, u16)>,
    pub(crate) notes: Option<NotesEnvInputs>,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonMailboxPostRequest {
    pub(crate) to: Option<String>,
    pub(crate) topic: Option<String>,
    pub(crate) body: String,
    pub(crate) subject: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) correlation_id: Option<String>,
    pub(crate) structured: Option<Value>,
    pub(crate) from: Option<String>,
}

impl DaemonClient {
    pub(crate) fn detect() -> Option<Self> {
        let data =
            send_command_blocking(serde_json::json!({ "command": "daemon-status" }), PROBE_TIMEOUT)
                .ok()?;
        let status: DaemonStatus = serde_json::from_value(data).ok()?;
        if status.kind == "roux-daemon" {
            Some(Self { status })
        } else {
            None
        }
    }

    pub(crate) fn ensure_local() -> Option<Self> {
        if let Some(client) = Self::detect() {
            return Some(client);
        }

        match launch_local_daemon() {
            Ok(started) => {
                rlog!("Started roux daemon pid={} from {}", started.pid, started.binary.display());
            }
            Err(err) => {
                rlog!("Unable to start roux daemon; desktop will self-host runtime state: {err}");
                return None;
            }
        }

        match wait_for_daemon(STARTUP_TIMEOUT, STARTUP_POLL_INTERVAL) {
            Some(client) => Some(client),
            None => {
                rlog!(
                    "Started roux daemon but it did not become ready within {}ms; desktop will self-host runtime state",
                    STARTUP_TIMEOUT.as_millis()
                );
                None
            }
        }
    }

    pub(crate) fn status(&self) -> &DaemonStatus {
        &self.status
    }

    pub(crate) fn supports(&self, capability: &str) -> bool {
        self.status.capabilities.iter().any(|candidate| candidate == capability)
    }

    pub(crate) async fn refresh_status(&self) -> Result<DaemonStatus, String> {
        let value = send_command_async(serde_json::json!({ "command": "daemon-status" })).await?;
        let status: DaemonStatus =
            serde_json::from_value(value).map_err(|err| format!("decode daemon-status: {err}"))?;
        if status.kind == "roux-daemon" {
            Ok(status)
        } else {
            Err(format!("unexpected daemon kind: {}", status.kind))
        }
    }

    pub(crate) async fn list_sessions(&self) -> Result<Vec<Session>, String> {
        let value = send_command_async(serde_json::json!({ "command": "session-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-list: {err}"))
    }

    pub(crate) async fn get_session(&self, id: String) -> Result<Session, String> {
        let value = send_command_async(serde_json::json!({
            "command": "session-poll",
            "session_id": id,
        }))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-poll: {err}"))
    }

    pub(crate) async fn list_projects(&self) -> Result<Vec<Project>, String> {
        let value = send_command_async(serde_json::json!({ "command": "project-list" })).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon project-list: {err}"))
    }

    pub(crate) async fn list_aliases(
        &self,
        project_id: Option<String>,
        global: bool,
        only_unbound: bool,
    ) -> Result<Vec<AgentAlias>, String> {
        let value =
            send_command_async(daemon_alias_list_request(project_id, global, only_unbound)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon alias-list: {err}"))
    }

    pub(crate) async fn get_alias(
        &self,
        alias: String,
        project_id: Option<String>,
    ) -> Result<Option<AgentAlias>, String> {
        let value =
            match send_command_async(daemon_alias_get_request(alias.clone(), project_id)).await {
                Ok(value) => value,
                Err(err) if err.contains(&format!("alias '{alias}' not found")) => return Ok(None),
                Err(err) => return Err(err),
            };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|err| format!("decode daemon alias-get: {err}"))
    }

    pub(crate) async fn whoami_aliases(
        &self,
        session_id: String,
    ) -> Result<Vec<AgentAlias>, String> {
        let value = send_command_async(daemon_alias_whoami_request(session_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon alias-whoami: {err}"))
    }

    pub(crate) async fn add_alias_member(
        &self,
        alias: String,
        pane_id: String,
        project_id: Option<String>,
    ) -> Result<AgentAlias, String> {
        let value = send_command_async(daemon_alias_member_request(
            "alias-add-member",
            alias,
            pane_id,
            project_id,
        ))
        .await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon alias-add-member: {err}"))
    }

    pub(crate) async fn remove_alias_member(
        &self,
        alias: String,
        pane_id: String,
        project_id: Option<String>,
    ) -> Result<bool, String> {
        let value = send_command_async(daemon_alias_member_request(
            "alias-remove-member",
            alias,
            pane_id,
            project_id,
        ))
        .await?;
        value
            .get("removed")
            .and_then(|removed| removed.as_bool())
            .ok_or_else(|| "decode daemon alias-remove-member: missing removed".to_string())
    }

    pub(crate) async fn set_alias_mode(
        &self,
        alias: String,
        mode: String,
        project_id: Option<String>,
    ) -> Result<AgentAlias, String> {
        let value = send_command_async(daemon_alias_mode_request(alias, mode, project_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon alias-mode: {err}"))
    }

    pub(crate) async fn list_subscriptions(
        &self,
        alias: Option<String>,
        project_id: Option<String>,
        global: bool,
    ) -> Result<Vec<BusSubscription>, String> {
        let value =
            send_command_async(daemon_bus_subscriptions_request(alias, project_id, global)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon bus-subscriptions: {err}"))
    }

    pub(crate) async fn create_subscription(
        &self,
        alias: String,
        pattern: String,
        project_id: Option<String>,
    ) -> Result<BusSubscription, String> {
        let value =
            send_command_async(daemon_bus_subscribe_request(alias, pattern, project_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon bus-subscribe: {err}"))
    }

    pub(crate) async fn delete_subscription(&self, id: String) -> Result<bool, String> {
        let value = send_command_async(daemon_bus_unsubscribe_request(id)).await?;
        value
            .get("removed")
            .and_then(|removed| removed.as_bool())
            .ok_or_else(|| "decode daemon bus-unsubscribe: missing removed".to_string())
    }

    pub(crate) async fn mailbox_list_for_recipient(
        &self,
        alias: String,
        unread_only: bool,
        project_id: Option<String>,
        global: bool,
    ) -> Result<Vec<Event>, String> {
        let value = send_command_async(daemon_mailbox_peek_request(
            alias,
            unread_only,
            project_id,
            global,
            None,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon mailbox-peek: {err}"))
    }

    pub(crate) async fn mailbox_list_for_topic(
        &self,
        topic: String,
        project_id: Option<String>,
        global: bool,
    ) -> Result<Vec<Event>, String> {
        let value =
            send_command_async(daemon_bus_tail_request(Some(topic), project_id, global, None))
                .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon bus-tail: {err}"))
    }

    pub(crate) async fn mailbox_list_all(
        &self,
        project_id: Option<String>,
        global: bool,
        limit: Option<u32>,
    ) -> Result<Vec<Event>, String> {
        let value =
            send_command_async(daemon_bus_tail_request(None, project_id, global, limit)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon bus-tail: {err}"))
    }

    pub(crate) async fn mailbox_unread_count(
        &self,
        alias: String,
        project_id: Option<String>,
        global: bool,
    ) -> Result<u32, String> {
        let value =
            send_command_async(daemon_mailbox_count_request(alias, project_id, global)).await?;
        value
            .get("unread")
            .and_then(|unread| unread.as_u64())
            .and_then(|unread| u32::try_from(unread).ok())
            .ok_or_else(|| "decode daemon mailbox-count: missing unread".to_string())
    }

    pub(crate) async fn mailbox_get_event(
        &self,
        event_id: String,
    ) -> Result<Option<Event>, String> {
        let value =
            send_command_async(daemon_mailbox_event_id_request("mailbox-get", event_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon mailbox-get: {err}"))
    }

    pub(crate) async fn mailbox_read_state(
        &self,
        event_id: String,
        recipient: String,
    ) -> Result<Option<ReadState>, String> {
        let value =
            send_command_async(daemon_mailbox_read_state_request(event_id, recipient)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon mailbox-read-state: {err}"))
    }

    pub(crate) async fn mailbox_post(
        &self,
        request: DaemonMailboxPostRequest,
    ) -> Result<Event, String> {
        let value = send_command_async(daemon_mailbox_post_request(request)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon mailbox-post: {err}"))
    }

    pub(crate) async fn mailbox_mark_read(
        &self,
        event_id: String,
        recipient: String,
    ) -> Result<bool, String> {
        let value =
            send_command_async(daemon_mailbox_mark_read_request(event_id, recipient)).await?;
        value
            .get("changed")
            .and_then(|changed| changed.as_bool())
            .ok_or_else(|| "decode daemon mailbox-mark-read: missing changed".to_string())
    }

    pub(crate) async fn mailbox_ack(
        &self,
        event_id: String,
        recipient: String,
        result: Option<String>,
    ) -> Result<bool, String> {
        let value = send_command_async(daemon_mailbox_alias_event_request(
            "mailbox-ack",
            event_id,
            recipient,
            result,
        ))
        .await?;
        value
            .get("changed")
            .and_then(|changed| changed.as_bool())
            .ok_or_else(|| "decode daemon mailbox-ack: missing changed".to_string())
    }

    pub(crate) async fn mailbox_clear_read(
        &self,
        recipient: String,
        project_id: Option<String>,
        global: bool,
    ) -> Result<u32, String> {
        let value =
            send_command_async(daemon_mailbox_clear_request(recipient, project_id, global)).await?;
        value
            .get("cleared")
            .and_then(|cleared| cleared.as_u64())
            .and_then(|cleared| u32::try_from(cleared).ok())
            .ok_or_else(|| "decode daemon mailbox-clear: missing cleared".to_string())
    }

    pub(crate) async fn mailbox_retract(
        &self,
        event_id: String,
        sender: String,
    ) -> Result<Event, String> {
        let value = send_command_async(daemon_mailbox_alias_event_request(
            "mailbox-retract",
            event_id,
            sender,
            None,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon mailbox-retract: {err}"))
    }

    pub(crate) async fn mailbox_dismiss(
        &self,
        event_id: String,
        recipient: String,
    ) -> Result<bool, String> {
        let value = send_command_async(daemon_mailbox_alias_event_request(
            "mailbox-dismiss",
            event_id,
            recipient,
            None,
        ))
        .await?;
        value
            .get("changed")
            .and_then(|changed| changed.as_bool())
            .ok_or_else(|| "decode daemon mailbox-dismiss: missing changed".to_string())
    }

    pub(crate) async fn set_session_name_override(
        &self,
        session_id: String,
        name_override: Option<String>,
    ) -> Result<(), String> {
        let name = name_override.unwrap_or_default();
        let _ = send_command_async(serde_json::json!({
            "command": "session-rename",
            "session_id": session_id,
            "args": { "name": name },
        }))
        .await?;
        Ok(())
    }

    pub(crate) async fn create_session_shell(
        &self,
        request: DaemonCreateSessionShellRequest,
    ) -> Result<Session, String> {
        let value = send_command_async(daemon_session_create_shell_request(request)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon session-create-shell: {err}"))
    }

    pub(crate) async fn reconnect_session_shell(
        &self,
        request: DaemonReconnectSessionShellRequest,
    ) -> Result<Session, String> {
        let value = send_command_async(daemon_session_reconnect_shell_request(request)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon session-reconnect-shell: {err}"))
    }

    pub(crate) async fn archive_session(&self, id: String) -> Result<Session, String> {
        let value = send_command_async(daemon_session_id_request("session-archive", id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-archive: {err}"))
    }

    pub(crate) async fn restore_session(&self, id: String) -> Result<Session, String> {
        let value = send_command_async(daemon_session_id_request("session-restore", id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon session-restore: {err}"))
    }

    pub(crate) async fn delete_session(&self, id: String) -> Result<(), String> {
        let _ = send_command_async(daemon_session_id_request("session-delete", id)).await?;
        Ok(())
    }

    pub(crate) async fn session_worktree_exists(&self, id: String) -> Result<bool, String> {
        let value =
            send_command_async(daemon_session_id_request("session-worktree-exists", id)).await?;
        value
            .get("exists")
            .and_then(|exists| exists.as_bool())
            .ok_or_else(|| "decode daemon session-worktree-exists: missing exists".to_string())
    }

    pub(crate) async fn refresh_session_branch(
        &self,
        id: String,
    ) -> Result<Option<String>, String> {
        let value =
            send_command_async(daemon_session_id_request("session-refresh-branch", id)).await?;
        Ok(value.get("branch").and_then(|branch| branch.as_str()).map(str::to_string))
    }

    pub(crate) async fn list_worktrees(&self, repo_path: String) -> Result<Vec<Worktree>, String> {
        let value =
            send_command_async(daemon_repo_path_request("worktree-list", repo_path)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon worktree-list: {err}"))
    }

    pub(crate) async fn create_worktree(
        &self,
        repo_path: String,
        branch: String,
        start_point: Option<String>,
        fetch_first: bool,
    ) -> Result<String, String> {
        let value = send_command_async(daemon_worktree_create_request(
            repo_path,
            branch,
            start_point,
            fetch_first,
        ))
        .await?;
        value
            .get("path")
            .and_then(|path| path.as_str())
            .map(str::to_string)
            .ok_or_else(|| "decode daemon worktree-create: missing path".to_string())
    }

    pub(crate) async fn remove_worktree(
        &self,
        repo_path: String,
        worktree_path: String,
        also_branch: bool,
        force: bool,
    ) -> Result<(), String> {
        let _ = send_command_async(daemon_worktree_remove_request(
            repo_path,
            worktree_path,
            also_branch,
            force,
        ))
        .await?;
        Ok(())
    }

    pub(crate) async fn list_branches(&self, repo_path: String) -> Result<Vec<String>, String> {
        let value =
            send_command_async(daemon_repo_path_request("worktree-list-branches", repo_path))
                .await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon worktree-list-branches: {err}"))
    }

    pub(crate) async fn git_init(&self, path: String) -> Result<(), String> {
        let _ = send_command_async(daemon_path_request("git-init", path)).await?;
        Ok(())
    }

    pub(crate) async fn list_watches(&self) -> Result<Vec<Watch>, String> {
        let value = send_command_async(daemon_watch_list_request()).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon watch-list: {err}"))
    }

    pub(crate) async fn create_watch(&self, config: CreateWatchConfig) -> Result<Watch, String> {
        let value = send_command_async(daemon_watch_config_request("watch-create", config)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon watch-create: {err}"))
    }

    pub(crate) async fn find_or_create_watch(
        &self,
        config: CreateWatchConfig,
    ) -> Result<Watch, String> {
        let value =
            send_command_async(daemon_watch_config_request("watch-find-or-create", config)).await?;
        serde_json::from_value(value)
            .map_err(|err| format!("decode daemon watch-find-or-create: {err}"))
    }

    pub(crate) async fn remove_watch(&self, id: String) -> Result<(), String> {
        let _ = send_command_async(daemon_watch_id_request("watch-remove", id)).await?;
        Ok(())
    }

    pub(crate) async fn pause_watch(&self, id: String) -> Result<Watch, String> {
        let value = send_command_async(daemon_watch_id_request("watch-pause", id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon watch-pause: {err}"))
    }

    pub(crate) async fn resume_watch(&self, id: String) -> Result<Watch, String> {
        let value = send_command_async(daemon_watch_id_request("watch-resume", id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon watch-resume: {err}"))
    }

    pub(crate) async fn replace_watch(&self, watch: Watch) -> Result<(), String> {
        let _ = send_command_async(daemon_watch_replace_request(watch)).await?;
        Ok(())
    }

    pub(crate) async fn remove_watches_for_session(
        &self,
        session_id: String,
    ) -> Result<(), String> {
        let _ = send_command_async(daemon_watch_session_request(session_id)).await?;
        Ok(())
    }

    pub(crate) async fn cleanup_watch_orphans(&self) -> Result<(), String> {
        let _ = send_command_async(daemon_watch_cleanup_orphans_request()).await?;
        Ok(())
    }

    pub(crate) async fn start_daemon_process(
        &self,
        command: String,
        working_dir: Option<String>,
    ) -> Result<ProcessRecord, String> {
        let value = send_command_async(daemon_process_start_request(command, working_dir)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process start: {err}"))
    }

    pub(crate) async fn daemon_process_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> Result<ProcessSnapshot, String> {
        let value = send_command_async(daemon_process_output_request(id, max_bytes)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process output: {err}"))
    }

    pub(crate) async fn list_daemon_processes(&self) -> Result<Vec<ProcessRecord>, String> {
        let value = send_command_async(daemon_process_list_request()).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process list: {err}"))
    }

    pub(crate) async fn kill_daemon_process(&self, id: String) -> Result<ProcessRecord, String> {
        let value = send_command_async(daemon_process_kill_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon process kill: {err}"))
    }

    pub(crate) async fn spawn_daemon_pty_shell(
        &self,
        id: Option<String>,
        working_dir: Option<String>,
        session_id: Option<String>,
        pane_id: Option<String>,
        profile: Option<String>,
        initial_size: Option<(u16, u16)>,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_spawn_shell_request(
            id,
            working_dir,
            session_id,
            pane_id,
            profile,
            initial_size,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty spawn shell: {err}"))
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
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_spawn_task_request(
            command,
            id,
            working_dir,
            session_id,
            pane_id,
            profile,
            initial_size,
        ))
        .await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty spawn task: {err}"))
    }

    pub(crate) async fn daemon_pty_output(
        &self,
        id: String,
        max_bytes: Option<usize>,
    ) -> Result<PtySnapshot, String> {
        let value = send_command_async(daemon_pty_output_request(id, max_bytes)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty output: {err}"))
    }

    pub(crate) async fn list_daemon_ptys(&self) -> Result<Vec<PtyRecord>, String> {
        let value = send_command_async(daemon_pty_list_request()).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty list: {err}"))
    }

    pub(crate) async fn write_daemon_pty(&self, id: String, data: String) -> Result<(), String> {
        let _ = send_command_async(daemon_pty_write_request(id, data)).await?;
        Ok(())
    }

    pub(crate) async fn resize_daemon_pty(
        &self,
        id: String,
        cols: u16,
        rows: u16,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_resize_request(id, cols, rows)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty resize: {err}"))
    }

    pub(crate) async fn kill_daemon_pty(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_kill_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty kill: {err}"))
    }

    pub(crate) async fn detach_daemon_pty(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_detach_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty detach: {err}"))
    }

    pub(crate) async fn attach_daemon_pty_to_pane(
        &self,
        id: String,
        pane_id: String,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_attach_pane_request(id, pane_id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty attach pane: {err}"))
    }

    pub(crate) async fn mark_daemon_pty_read(&self, id: String) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_mark_read_request(id)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty mark read: {err}"))
    }

    pub(crate) async fn set_daemon_pty_name(
        &self,
        id: String,
        name: Option<String>,
    ) -> Result<PtyRecord, String> {
        let value = send_command_async(daemon_pty_set_name_request(id, name)).await?;
        serde_json::from_value(value).map_err(|err| format!("decode daemon pty set name: {err}"))
    }

    pub(crate) fn spawn_daemon_pty_output_bridge(
        &self,
        id: String,
        channel: Channel<IpcResponse>,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = attach_daemon_pty_output_blocking(id.clone(), channel, app) {
                rlog!("Daemon PTY output bridge for {id} stopped: {err}");
            }
        })
    }

    pub(crate) fn spawn_watch_event_bridge(
        &self,
        app: AppHandle,
        watch_manager: WatchManager,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = read_watch_events_blocking(app, watch_manager) {
                rlog!("Daemon watch event bridge stopped: {err}");
            }
        })
    }

    pub(crate) fn spawn_mailbox_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = read_mailbox_events_blocking(app) {
                rlog!("Daemon mailbox event bridge stopped: {err}");
            }
        })
    }

    pub(crate) fn spawn_alias_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = read_alias_events_blocking(app) {
                rlog!("Daemon alias event bridge stopped: {err}");
            }
        })
    }

    pub(crate) fn spawn_subscription_event_bridge(
        &self,
        app: AppHandle,
    ) -> tauri::async_runtime::JoinHandle<()> {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = read_subscription_events_blocking(app) {
                rlog!("Daemon subscription event bridge stopped: {err}");
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartedDaemon {
    binary: PathBuf,
    pid: u32,
}

fn launch_local_daemon() -> Result<StartedDaemon, String> {
    if let Some(reason) = daemon_autostart_disabled_reason() {
        return Err(reason);
    }
    let binary = resolve_daemon_binary()?;
    let mut child = daemon_spawn_command(&binary)
        .spawn()
        .map_err(|err| format!("spawn {} daemon: {err}", binary.display()))?;
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

fn daemon_autostart_disabled_reason() -> Option<String> {
    daemon_autostart_disabled_reason_for(
        std::env::var("ROUX_DAEMON_AUTOSTART").ok().as_deref(),
        std::env::var("ROUX_SOCKET").ok().as_deref(),
    )
}

fn daemon_autostart_disabled_reason_for(
    autostart: Option<&str>,
    socket: Option<&str>,
) -> Option<String> {
    if autostart.and_then(parse_env_enabled) == Some(false) {
        return Some("ROUX_DAEMON_AUTOSTART disabled local daemon startup".to_string());
    }
    if socket.map(|value| !value.trim().is_empty()).unwrap_or(false) {
        return Some("ROUX_SOCKET is set, assuming external daemon endpoint".to_string());
    }
    None
}

fn parse_env_enabled(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "0" | "false" | "no" | "off" => Some(false),
        _ => Some(true),
    }
}

fn resolve_daemon_binary() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().ok();
    resolve_daemon_binary_from(current_exe.as_deref()).ok_or_else(|| {
        format!(
            "{} not found next to the desktop binary or on PATH",
            platform::roux_cli_file_name()
        )
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

fn daemon_process_start_request(command: String, working_dir: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("command".to_string(), Value::String(command));
    if let Some(working_dir) = working_dir {
        args.insert("workingDir".to_string(), Value::String(working_dir));
    }
    serde_json::json!({
        "command": "daemon-process-start",
        "args": args,
    })
}

fn daemon_session_create_shell_request(request: DaemonCreateSessionShellRequest) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(request.id));
    args.insert("repoPath".to_string(), Value::String(request.repo_path));
    args.insert("name".to_string(), Value::String(request.name));
    if let Some(worktree_path) = request.worktree_path {
        args.insert("worktreePath".to_string(), Value::String(worktree_path));
    }
    if let Some(branch) = request.branch {
        args.insert("branch".to_string(), Value::String(branch));
    }
    if let Some(base) = request.base {
        args.insert("base".to_string(), Value::String(base));
    }
    if request.fetch_first {
        args.insert("fetchFirst".to_string(), Value::Bool(true));
    }
    if let Some(profile) = request.profile {
        args.insert("profile".to_string(), Value::String(profile));
    }
    if let Some((cols, rows)) = request.initial_size {
        args.insert("initialSize".to_string(), serde_json::json!([cols, rows]));
    }
    if let Some(project_id) = request.project_id {
        args.insert("projectId".to_string(), Value::String(project_id));
    }
    if let Some(blueprint_id) = request.blueprint_id {
        args.insert("blueprintId".to_string(), Value::String(blueprint_id));
    }
    if let Some(smol_machine_name) = request.smol_machine_name {
        args.insert("smolMachineName".to_string(), Value::String(smol_machine_name));
    }
    if let Some(notes) = request.notes {
        args.insert(
            "notesEnv".to_string(),
            serde_json::json!({
                "vaultRoot": notes.vault_root,
                "sessionSlug": notes.session_slug,
                "repoSlug": notes.repo_slug,
                "projectSlug": notes.project_slug,
                "contextPaths": notes.context_paths,
                "projectPrompt": notes.project_prompt,
            }),
        );
    }
    serde_json::json!({
        "command": "session-create-shell",
        "args": args,
    })
}

fn daemon_session_reconnect_shell_request(request: DaemonReconnectSessionShellRequest) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(profile) = request.profile {
        args.insert("profile".to_string(), Value::String(profile));
    }
    if let Some((cols, rows)) = request.initial_size {
        args.insert("initialSize".to_string(), serde_json::json!([cols, rows]));
    }
    if let Some(notes) = request.notes {
        args.insert(
            "notesEnv".to_string(),
            serde_json::json!({
                "vaultRoot": notes.vault_root,
                "sessionSlug": notes.session_slug,
                "repoSlug": notes.repo_slug,
                "projectSlug": notes.project_slug,
                "contextPaths": notes.context_paths,
                "projectPrompt": notes.project_prompt,
            }),
        );
    }
    serde_json::json!({
        "command": "session-reconnect-shell",
        "session_id": request.id,
        "args": args,
    })
}

fn daemon_session_id_request(command: &str, id: String) -> Value {
    serde_json::json!({
        "command": command,
        "session_id": id,
    })
}

fn daemon_repo_path_request(command: &str, repo_path: String) -> Value {
    serde_json::json!({
        "command": command,
        "args": { "repoPath": repo_path },
    })
}

fn daemon_path_request(command: &str, path: String) -> Value {
    serde_json::json!({
        "command": command,
        "args": { "path": path },
    })
}

fn daemon_alias_list_request(
    project_id: Option<String>,
    global: bool,
    only_unbound: bool,
) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    if global {
        args.insert("global".to_string(), Value::Bool(true));
    }
    if only_unbound {
        args.insert("only_unbound".to_string(), Value::Bool(true));
    }
    serde_json::json!({
        "command": "alias-list",
        "args": args,
    })
}

fn daemon_alias_get_request(alias: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    serde_json::json!({
        "command": "alias-get",
        "args": args,
    })
}

fn daemon_alias_whoami_request(session_id: String) -> Value {
    serde_json::json!({
        "command": "alias-whoami",
        "session_id": session_id,
        "args": {},
    })
}

fn daemon_alias_member_request(
    command: &str,
    alias: String,
    pane_id: String,
    project_id: Option<String>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    args.insert("pane_id".to_string(), Value::String(pane_id));
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    serde_json::json!({
        "command": command,
        "args": args,
    })
}

fn daemon_alias_mode_request(alias: String, mode: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    args.insert("mode".to_string(), Value::String(mode));
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    serde_json::json!({
        "command": "alias-mode",
        "args": args,
    })
}

fn daemon_bus_subscriptions_request(
    alias: Option<String>,
    project_id: Option<String>,
    global: bool,
) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(alias) = alias {
        args.insert("alias".to_string(), Value::String(alias));
    }
    insert_project_filter_args(&mut args, project_id, global);
    serde_json::json!({
        "command": "bus-subscriptions",
        "args": args,
    })
}

fn daemon_bus_subscribe_request(
    alias: String,
    pattern: String,
    project_id: Option<String>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    args.insert("pattern".to_string(), Value::String(pattern));
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    serde_json::json!({
        "command": "bus-subscribe",
        "args": args,
    })
}

fn daemon_bus_unsubscribe_request(id: String) -> Value {
    serde_json::json!({
        "command": "bus-unsubscribe",
        "args": { "id": id },
    })
}

fn daemon_mailbox_peek_request(
    alias: String,
    unread_only: bool,
    project_id: Option<String>,
    global: bool,
    limit: Option<u32>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    if unread_only {
        args.insert("unread".to_string(), Value::Bool(true));
    }
    insert_project_filter_args(&mut args, project_id, global);
    if let Some(limit) = limit {
        args.insert("limit".to_string(), serde_json::json!(limit));
    }
    serde_json::json!({
        "command": "mailbox-peek",
        "args": args,
    })
}

fn daemon_bus_tail_request(
    topic: Option<String>,
    project_id: Option<String>,
    global: bool,
    limit: Option<u32>,
) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(topic) = topic {
        args.insert("topic".to_string(), Value::String(topic));
    }
    insert_project_filter_args(&mut args, project_id, global);
    if let Some(limit) = limit {
        args.insert("limit".to_string(), serde_json::json!(limit));
    }
    serde_json::json!({
        "command": "bus-tail",
        "args": args,
    })
}

fn daemon_mailbox_count_request(alias: String, project_id: Option<String>, global: bool) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    insert_project_filter_args(&mut args, project_id, global);
    serde_json::json!({
        "command": "mailbox-count",
        "args": args,
    })
}

fn daemon_mailbox_post_request(request: DaemonMailboxPostRequest) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(to) = request.to {
        args.insert("to".to_string(), Value::String(to));
    }
    if let Some(topic) = request.topic {
        args.insert("topic".to_string(), Value::String(topic));
    }
    args.insert("body".to_string(), Value::String(request.body));
    if let Some(subject) = request.subject {
        args.insert("subject".to_string(), Value::String(subject));
    }
    if let Some(kind) = request.kind {
        args.insert("kind".to_string(), Value::String(kind));
    }
    if let Some(project_id) = request.project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    if let Some(correlation_id) = request.correlation_id {
        args.insert("correlation_id".to_string(), Value::String(correlation_id));
    }
    if let Some(structured) = request.structured {
        args.insert("structured".to_string(), structured);
    }
    if let Some(from) = request.from {
        args.insert("from".to_string(), Value::String(from));
    }
    serde_json::json!({
        "command": "mailbox-post",
        "args": args,
    })
}

fn daemon_mailbox_event_id_request(command: &str, event_id: String) -> Value {
    serde_json::json!({
        "command": command,
        "args": { "event_id": event_id },
    })
}

fn daemon_mailbox_read_state_request(event_id: String, recipient: String) -> Value {
    serde_json::json!({
        "command": "mailbox-read-state",
        "args": { "event_id": event_id, "recipient": recipient },
    })
}

fn daemon_mailbox_mark_read_request(event_id: String, recipient: String) -> Value {
    serde_json::json!({
        "command": "mailbox-mark-read",
        "args": { "event_id": event_id, "recipient": recipient },
    })
}

fn daemon_mailbox_alias_event_request(
    command: &str,
    event_id: String,
    alias: String,
    result: Option<String>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("event_id".to_string(), Value::String(event_id));
    args.insert("alias".to_string(), Value::String(alias));
    if let Some(result) = result {
        args.insert("result".to_string(), Value::String(result));
    }
    serde_json::json!({
        "command": command,
        "args": args,
    })
}

fn daemon_mailbox_clear_request(alias: String, project_id: Option<String>, global: bool) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".to_string(), Value::String(alias));
    insert_project_filter_args(&mut args, project_id, global);
    serde_json::json!({
        "command": "mailbox-clear",
        "args": args,
    })
}

fn insert_project_filter_args(
    args: &mut serde_json::Map<String, Value>,
    project_id: Option<String>,
    global: bool,
) {
    if let Some(project_id) = project_id {
        args.insert("project_id".to_string(), Value::String(project_id));
    }
    if global {
        args.insert("global".to_string(), Value::Bool(true));
    }
}

fn daemon_watch_list_request() -> Value {
    serde_json::json!({ "command": "watch-list" })
}

fn daemon_watch_config_request(command: &str, config: CreateWatchConfig) -> Value {
    serde_json::json!({
        "command": command,
        "args": { "config": config },
    })
}

fn daemon_watch_id_request(command: &str, id: String) -> Value {
    serde_json::json!({
        "command": command,
        "args": { "id": id },
    })
}

fn daemon_watch_replace_request(watch: Watch) -> Value {
    serde_json::json!({
        "command": "watch-replace",
        "args": { "watch": watch },
    })
}

fn daemon_watch_session_request(session_id: String) -> Value {
    serde_json::json!({
        "command": "watch-remove-for-session",
        "args": { "sessionId": session_id },
    })
}

fn daemon_watch_cleanup_orphans_request() -> Value {
    serde_json::json!({ "command": "watch-cleanup-orphans" })
}

fn daemon_watch_events_request(backlog: bool) -> Value {
    serde_json::json!({
        "command": "watch-events",
        "args": { "backlog": backlog },
    })
}

fn daemon_mailbox_events_request() -> Value {
    serde_json::json!({ "command": "mailbox-events" })
}

fn daemon_alias_events_request() -> Value {
    serde_json::json!({ "command": "alias-events" })
}

fn daemon_subscription_events_request() -> Value {
    serde_json::json!({ "command": "subscription-events" })
}

fn daemon_worktree_create_request(
    repo_path: String,
    branch: String,
    start_point: Option<String>,
    fetch_first: bool,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("repoPath".to_string(), Value::String(repo_path));
    args.insert("branch".to_string(), Value::String(branch));
    if let Some(start_point) = start_point {
        args.insert("startPoint".to_string(), Value::String(start_point));
    }
    if fetch_first {
        args.insert("fetchFirst".to_string(), Value::Bool(true));
    }
    serde_json::json!({
        "command": "worktree-create",
        "args": args,
    })
}

fn daemon_worktree_remove_request(
    repo_path: String,
    worktree_path: String,
    also_branch: bool,
    force: bool,
) -> Value {
    serde_json::json!({
        "command": "worktree-remove",
        "args": {
            "repoPath": repo_path,
            "worktreePath": worktree_path,
            "alsoBranch": also_branch,
            "force": force,
        },
    })
}

fn daemon_process_output_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-process-output",
        "args": args,
    })
}

fn daemon_process_list_request() -> Value {
    serde_json::json!({ "command": "daemon-process-list" })
}

fn daemon_process_kill_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-process-kill",
        "args": { "id": id },
    })
}

fn daemon_pty_spawn_shell_request(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Value {
    serde_json::json!({
        "command": "daemon-pty-spawn-shell",
        "args": daemon_pty_spawn_args(id, working_dir, session_id, pane_id, profile, initial_size),
    })
}

fn daemon_pty_spawn_task_request(
    command: String,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> Value {
    let mut args =
        daemon_pty_spawn_args(id, working_dir, session_id, pane_id, profile, initial_size);
    args.insert("command".to_string(), Value::String(command));
    serde_json::json!({
        "command": "daemon-pty-spawn-task",
        "args": args,
    })
}

fn daemon_pty_spawn_args(
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
) -> serde_json::Map<String, Value> {
    let mut args = serde_json::Map::new();
    if let Some(id) = id {
        args.insert("id".to_string(), Value::String(id));
    }
    if let Some(working_dir) = working_dir {
        args.insert("workingDir".to_string(), Value::String(working_dir));
    }
    if let Some(session_id) = session_id {
        args.insert("sessionId".to_string(), Value::String(session_id));
    }
    if let Some(pane_id) = pane_id {
        args.insert("paneId".to_string(), Value::String(pane_id));
    }
    if let Some(profile) = profile {
        args.insert("profile".to_string(), Value::String(profile));
    }
    if let Some((cols, rows)) = initial_size {
        args.insert("initialSize".to_string(), serde_json::json!([cols, rows]));
    }
    args
}

fn daemon_pty_output_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-pty-output",
        "args": args,
    })
}

fn daemon_pty_attach_request(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".to_string(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".to_string(), serde_json::json!(max_bytes));
    }
    serde_json::json!({
        "command": "daemon-pty-attach",
        "args": args,
    })
}

fn daemon_pty_list_request() -> Value {
    serde_json::json!({ "command": "daemon-pty-list" })
}

fn daemon_pty_write_request(id: String, data: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-write",
        "args": { "id": id, "data": data },
    })
}

fn daemon_pty_resize_request(id: String, cols: u16, rows: u16) -> Value {
    serde_json::json!({
        "command": "daemon-pty-resize",
        "args": { "id": id, "cols": cols, "rows": rows },
    })
}

fn daemon_pty_detach_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-detach",
        "args": { "id": id },
    })
}

fn daemon_pty_attach_pane_request(id: String, pane_id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-attach-pane",
        "args": { "id": id, "paneId": pane_id },
    })
}

fn daemon_pty_mark_read_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-mark-read",
        "args": { "id": id },
    })
}

fn daemon_pty_set_name_request(id: String, name: Option<String>) -> Value {
    serde_json::json!({
        "command": "daemon-pty-set-name",
        "args": { "id": id, "name": name },
    })
}

fn daemon_pty_kill_request(id: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-kill",
        "args": { "id": id },
    })
}

async fn send_command_async(request: Value) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || send_command_blocking(request, COMMAND_TIMEOUT))
        .await
        .map_err(|err| format!("daemon client task failed: {err}"))?
}

#[derive(Debug, Deserialize)]
struct Response {
    ok: bool,
    data: Option<Value>,
    error: Option<String>,
}

fn decode_response(raw: &str) -> Result<Value, String> {
    let response: Response = serde_json::from_str(raw.trim())
        .map_err(|err| format!("invalid daemon response: {err}"))?;
    if response.ok {
        Ok(response.data.unwrap_or(Value::Null))
    } else {
        Err(response.error.unwrap_or_else(|| "daemon command failed".to_string()))
    }
}

fn send_command_blocking(request: Value, timeout: Duration) -> Result<Value, String> {
    #[cfg(windows)]
    {
        send_tcp_command(request, timeout)
    }
    #[cfg(not(windows))]
    {
        send_unix_command(request, timeout)
    }
}

#[cfg(not(windows))]
fn send_unix_command(request: Value, timeout: Duration) -> Result<Value, String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

#[cfg(windows)]
fn send_tcp_command(mut request: Value, timeout: Duration) -> Result<Value, String> {
    use std::net::{Shutdown, TcpStream};

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("set daemon read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;

    write_request(&mut stream, request)?;
    let _ = stream.shutdown(Shutdown::Write);
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|err| format!("read daemon response: {err}"))?;
    decode_response(&raw)
}

fn write_request(stream: &mut impl Write, request: Value) -> Result<(), String> {
    let json = serde_json::to_string(&request).map_err(|err| format!("encode request: {err}"))?;
    stream
        .write_all(json.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|err| format!("write daemon request: {err}"))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum PtyAttachStreamFrame {
    #[serde(rename = "ready")]
    Ready {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        record: PtyRecord,
        #[serde(rename = "replayOffset")]
        replay_offset: u64,
        #[serde(rename = "replayBytes")]
        replay_bytes: Vec<u8>,
    },
    #[serde(rename = "output")]
    Output { offset: u64, bytes: Vec<u8> },
    #[serde(rename = "exit")]
    Exit { code: Option<i32>, generation: u64 },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WatchEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "update")]
    Update { event: WatchUpdateEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum MailboxEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: MailboxEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AliasEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: AliasEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SubscriptionEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: BusSubscriptionEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

fn read_watch_events_blocking(app: AppHandle, watch_manager: WatchManager) -> Result<(), String> {
    #[cfg(windows)]
    {
        read_watch_events_tcp(app, watch_manager)
    }
    #[cfg(not(windows))]
    {
        read_watch_events_unix(app, watch_manager)
    }
}

#[cfg(not(windows))]
fn read_watch_events_unix(app: AppHandle, watch_manager: WatchManager) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, daemon_watch_events_request(true))?;
    read_watch_event_stream(stream, app, watch_manager)
}

#[cfg(windows)]
fn read_watch_events_tcp(app: AppHandle, watch_manager: WatchManager) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_watch_events_request(true);
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_watch_event_stream(stream, app, watch_manager)
}

fn read_watch_event_stream(
    stream: impl Read,
    app: AppHandle,
    watch_manager: WatchManager,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon watch event frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: WatchEventStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon watch event frame: {err}"))?;
        handle_watch_event_frame(frame, &app, &watch_manager)?;
    }
}

fn handle_watch_event_frame(
    frame: WatchEventStreamFrame,
    app: &AppHandle,
    watch_manager: &WatchManager,
) -> Result<(), String> {
    match frame {
        WatchEventStreamFrame::Ready => Ok(()),
        WatchEventStreamFrame::Update { event } => {
            let app = app.clone();
            let watch_manager = watch_manager.clone();
            tauri::async_runtime::spawn(async move {
                watch_manager.apply_daemon_watch_update(event, app).await;
            });
            Ok(())
        }
        WatchEventStreamFrame::Warning { message } => {
            rlog!("Daemon watch event stream warning: {message}");
            Ok(())
        }
        WatchEventStreamFrame::Error { error } => Err(error),
    }
}

fn read_mailbox_events_blocking(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        read_mailbox_events_tcp(app)
    }
    #[cfg(not(windows))]
    {
        read_mailbox_events_unix(app)
    }
}

#[cfg(not(windows))]
fn read_mailbox_events_unix(app: AppHandle) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, daemon_mailbox_events_request())?;
    read_mailbox_event_stream(stream, app)
}

#[cfg(windows)]
fn read_mailbox_events_tcp(app: AppHandle) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_mailbox_events_request();
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_mailbox_event_stream(stream, app)
}

fn read_mailbox_event_stream(stream: impl Read, app: AppHandle) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon mailbox event frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: MailboxEventStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon mailbox event frame: {err}"))?;
        handle_mailbox_event_frame(frame, &app)?;
    }
}

fn handle_mailbox_event_frame(
    frame: MailboxEventStreamFrame,
    app: &AppHandle,
) -> Result<(), String> {
    match frame {
        MailboxEventStreamFrame::Ready => Ok(()),
        MailboxEventStreamFrame::Event { event } => app
            .emit(roux_lib::mailbox::MAILBOX_EVENT, &event)
            .map_err(|err| format!("emit daemon mailbox event: {err}")),
        MailboxEventStreamFrame::Warning { message } => {
            rlog!("Daemon mailbox event stream warning: {message}");
            Ok(())
        }
        MailboxEventStreamFrame::Error { error } => Err(error),
    }
}

fn read_alias_events_blocking(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        read_alias_events_tcp(app)
    }
    #[cfg(not(windows))]
    {
        read_alias_events_unix(app)
    }
}

#[cfg(not(windows))]
fn read_alias_events_unix(app: AppHandle) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, daemon_alias_events_request())?;
    read_alias_event_stream(stream, app)
}

#[cfg(windows)]
fn read_alias_events_tcp(app: AppHandle) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_alias_events_request();
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_alias_event_stream(stream, app)
}

fn read_alias_event_stream(stream: impl Read, app: AppHandle) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon alias event frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: AliasEventStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon alias event frame: {err}"))?;
        handle_alias_event_frame(frame, &app)?;
    }
}

fn handle_alias_event_frame(frame: AliasEventStreamFrame, app: &AppHandle) -> Result<(), String> {
    match frame {
        AliasEventStreamFrame::Ready => Ok(()),
        AliasEventStreamFrame::Event { event } => app
            .emit(roux_lib::aliases::ALIAS_EVENT, &event)
            .map_err(|err| format!("emit daemon alias event: {err}")),
        AliasEventStreamFrame::Warning { message } => {
            rlog!("Daemon alias event stream warning: {message}");
            Ok(())
        }
        AliasEventStreamFrame::Error { error } => Err(error),
    }
}

fn read_subscription_events_blocking(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        read_subscription_events_tcp(app)
    }
    #[cfg(not(windows))]
    {
        read_subscription_events_unix(app)
    }
}

#[cfg(not(windows))]
fn read_subscription_events_unix(app: AppHandle) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, daemon_subscription_events_request())?;
    read_subscription_event_stream(stream, app)
}

#[cfg(windows)]
fn read_subscription_events_tcp(app: AppHandle) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_subscription_events_request();
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_subscription_event_stream(stream, app)
}

fn read_subscription_event_stream(stream: impl Read, app: AppHandle) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon subscription event frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: SubscriptionEventStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon subscription event frame: {err}"))?;
        handle_subscription_event_frame(frame, &app)?;
    }
}

fn handle_subscription_event_frame(
    frame: SubscriptionEventStreamFrame,
    app: &AppHandle,
) -> Result<(), String> {
    match frame {
        SubscriptionEventStreamFrame::Ready => Ok(()),
        SubscriptionEventStreamFrame::Event { event } => app
            .emit(roux_lib::subscriptions::SUBSCRIPTION_EVENT, &event)
            .map_err(|err| format!("emit daemon subscription event: {err}")),
        SubscriptionEventStreamFrame::Warning { message } => {
            rlog!("Daemon subscription event stream warning: {message}");
            Ok(())
        }
        SubscriptionEventStreamFrame::Error { error } => Err(error),
    }
}

fn attach_daemon_pty_output_blocking(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        attach_daemon_pty_output_tcp(id, channel, app)
    }
    #[cfg(not(windows))]
    {
        attach_daemon_pty_output_unix(id, channel, app)
    }
}

#[cfg(not(windows))]
fn attach_daemon_pty_output_unix(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    use std::os::unix::net::UnixStream;

    let path = platform::resolve_socket_endpoint()
        .ok_or_else(|| "daemon socket endpoint not found".to_string())?;
    let mut stream =
        UnixStream::connect(&path).map_err(|err| format!("connect daemon socket {path}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(
        &mut stream,
        daemon_pty_attach_request(
            id.clone(),
            Some(roux_runtime::pty_service::PTY_OUTPUT_LIMIT_BYTES),
        ),
    )?;
    read_pty_attach_stream(id, stream, channel, app)
}

#[cfg(windows)]
fn attach_daemon_pty_output_tcp(
    id: String,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    use std::net::TcpStream;

    let auth_token = platform::load_socket_auth_token()
        .ok_or_else(|| "daemon socket auth token not found".to_string())?;
    let mut request = daemon_pty_attach_request(
        id.clone(),
        Some(roux_runtime::pty_service::PTY_OUTPUT_LIMIT_BYTES),
    );
    if let Some(obj) = request.as_object_mut() {
        obj.insert("auth_token".to_string(), Value::String(auth_token));
    }

    let endpoint =
        platform::resolve_socket_endpoint().ok_or_else(|| "daemon socket endpoint not found")?;
    let mut stream = TcpStream::connect(&endpoint)
        .map_err(|err| format!("connect daemon socket {endpoint}: {err}"))?;
    stream
        .set_write_timeout(Some(COMMAND_TIMEOUT))
        .map_err(|err| format!("set daemon write timeout: {err}"))?;
    write_request(&mut stream, request)?;
    read_pty_attach_stream(id, stream, channel, app)
}

fn read_pty_attach_stream(
    id: String,
    stream: impl Read,
    channel: Channel<IpcResponse>,
    app: AppHandle,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut sent_until = 0_u64;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|err| format!("read daemon pty attach frame: {err}"))?;
        if read == 0 {
            return Ok(());
        }
        let frame: PtyAttachStreamFrame = serde_json::from_str(line.trim())
            .map_err(|err| format!("decode daemon pty attach frame: {err}"))?;
        if !handle_pty_attach_frame(&id, frame, &channel, &app, &mut sent_until)? {
            return Ok(());
        }
    }
}

fn handle_pty_attach_frame(
    id: &str,
    frame: PtyAttachStreamFrame,
    channel: &Channel<IpcResponse>,
    app: &AppHandle,
    sent_until: &mut u64,
) -> Result<bool, String> {
    match frame {
        PtyAttachStreamFrame::Ready { replay_offset, replay_bytes, .. } => {
            let replay_end = replay_offset.saturating_add(replay_bytes.len() as u64);
            if !replay_bytes.is_empty() {
                channel
                    .send(IpcResponse::new(replay_bytes))
                    .map_err(|err| format!("send daemon pty replay to frontend: {err}"))?;
            }
            *sent_until = (*sent_until).max(replay_end);
            Ok(true)
        }
        PtyAttachStreamFrame::Output { offset, bytes } => {
            let frame_end = offset.saturating_add(bytes.len() as u64);
            if frame_end <= *sent_until {
                return Ok(true);
            }
            let start = if offset < *sent_until { (*sent_until - offset) as usize } else { 0 };
            let bytes = bytes[start..].to_vec();
            if !bytes.is_empty() {
                channel
                    .send(IpcResponse::new(bytes))
                    .map_err(|err| format!("send daemon pty output to frontend: {err}"))?;
            }
            *sent_until = (*sent_until).max(frame_end);
            Ok(true)
        }
        PtyAttachStreamFrame::Exit { code, generation } => {
            emit_daemon_pty_exit(app, id, code, generation);
            Ok(false)
        }
        PtyAttachStreamFrame::Error { error } => Err(error),
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

    #[test]
    fn decode_response_returns_data_on_success() {
        let data = decode_response(r#"{"ok":true,"data":{"kind":"roux-daemon"}}"#).unwrap();
        assert_eq!(data["kind"], "roux-daemon");
    }

    #[test]
    fn decode_response_returns_error_message() {
        let err = decode_response(r#"{"ok":false,"error":"nope"}"#).unwrap_err();
        assert_eq!(err, "nope");
    }

    #[test]
    fn daemon_process_start_request_uses_daemon_command_shape() {
        let request =
            daemon_process_start_request("printf hi".to_string(), Some("/tmp".to_string()));

        assert_eq!(request["command"], "daemon-process-start");
        assert_eq!(request["args"]["command"], "printf hi");
        assert_eq!(request["args"]["workingDir"], "/tmp");
    }

    #[test]
    fn daemon_session_create_shell_request_uses_daemon_command_shape() {
        let request = daemon_session_create_shell_request(DaemonCreateSessionShellRequest {
            id: "session-a".to_string(),
            repo_path: "/repo".to_string(),
            name: "Daemon Session".to_string(),
            worktree_path: None,
            branch: Some("feature/demo".to_string()),
            base: Some("origin/main".to_string()),
            fetch_first: true,
            profile: Some("plain-shell".to_string()),
            initial_size: Some((100, 30)),
            project_id: Some("project-a".to_string()),
            blueprint_id: Some("blueprint-a".to_string()),
            smol_machine_name: None,
            notes: Some(NotesEnvInputs {
                vault_root: "/vault".to_string(),
                session_slug: "feature-demo--sessio".to_string(),
                repo_slug: "repo-a".to_string(),
                project_slug: Some("project-a".to_string()),
                context_paths: vec!["/repo/docs".to_string()],
                project_prompt: "Use project notes".to_string(),
            }),
        });

        assert_eq!(request["command"], "session-create-shell");
        assert_eq!(request["args"]["id"], "session-a");
        assert_eq!(request["args"]["repoPath"], "/repo");
        assert_eq!(request["args"]["branch"], "feature/demo");
        assert_eq!(request["args"]["base"], "origin/main");
        assert_eq!(request["args"]["fetchFirst"], true);
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
        assert_eq!(request["args"]["projectId"], "project-a");
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
        assert_eq!(request["args"]["notesEnv"]["contextPaths"][0], "/repo/docs");
    }

    #[test]
    fn daemon_session_lifecycle_requests_use_daemon_command_shape() {
        let reconnect =
            daemon_session_reconnect_shell_request(DaemonReconnectSessionShellRequest {
                id: "session-a".to_string(),
                profile: Some("plain-shell".to_string()),
                initial_size: Some((120, 40)),
                notes: None,
            });
        assert_eq!(reconnect["command"], "session-reconnect-shell");
        assert_eq!(reconnect["session_id"], "session-a");
        assert_eq!(reconnect["args"]["profile"], "plain-shell");
        assert_eq!(reconnect["args"]["initialSize"], serde_json::json!([120, 40]));

        let archive = daemon_session_id_request("session-archive", "session-a".to_string());
        assert_eq!(archive["command"], "session-archive");
        assert_eq!(archive["session_id"], "session-a");

        let restore = daemon_session_id_request("session-restore", "session-a".to_string());
        assert_eq!(restore["command"], "session-restore");

        let delete = daemon_session_id_request("session-delete", "session-a".to_string());
        assert_eq!(delete["command"], "session-delete");

        let exists = daemon_session_id_request("session-worktree-exists", "session-a".to_string());
        assert_eq!(exists["command"], "session-worktree-exists");

        let refresh = daemon_session_id_request("session-refresh-branch", "session-a".to_string());
        assert_eq!(refresh["command"], "session-refresh-branch");
    }

    #[test]
    fn daemon_worktree_requests_use_daemon_command_shape() {
        let create = daemon_worktree_create_request(
            "/repo".to_string(),
            "feature/demo".to_string(),
            Some("origin/main".to_string()),
            true,
        );
        assert_eq!(create["command"], "worktree-create");
        assert_eq!(create["args"]["repoPath"], "/repo");
        assert_eq!(create["args"]["branch"], "feature/demo");
        assert_eq!(create["args"]["startPoint"], "origin/main");
        assert_eq!(create["args"]["fetchFirst"], true);

        let remove = daemon_worktree_remove_request(
            "/repo".to_string(),
            "/repo-feature".to_string(),
            true,
            false,
        );
        assert_eq!(remove["command"], "worktree-remove");
        assert_eq!(remove["args"]["repoPath"], "/repo");
        assert_eq!(remove["args"]["worktreePath"], "/repo-feature");
        assert_eq!(remove["args"]["alsoBranch"], true);
        assert_eq!(remove["args"]["force"], false);

        let list = daemon_repo_path_request("worktree-list", "/repo".to_string());
        assert_eq!(list["command"], "worktree-list");
        assert_eq!(list["args"]["repoPath"], "/repo");

        let branches = daemon_repo_path_request("worktree-list-branches", "/repo".to_string());
        assert_eq!(branches["command"], "worktree-list-branches");

        let init = daemon_path_request("git-init", "/new-repo".to_string());
        assert_eq!(init["command"], "git-init");
        assert_eq!(init["args"]["path"], "/new-repo");
    }

    #[test]
    fn daemon_alias_requests_use_daemon_command_shape() {
        let list = daemon_alias_list_request(Some("project-a".to_string()), false, true);
        assert_eq!(list["command"], "alias-list");
        assert_eq!(list["args"]["project_id"], "project-a");
        assert_eq!(list["args"]["only_unbound"], true);
        assert!(list["args"].get("global").is_none());

        let get = daemon_alias_get_request("reviewer".to_string(), None);
        assert_eq!(get["command"], "alias-get");
        assert_eq!(get["args"]["alias"], "reviewer");

        let whoami = daemon_alias_whoami_request("session-a".to_string());
        assert_eq!(whoami["command"], "alias-whoami");
        assert_eq!(whoami["session_id"], "session-a");

        let add = daemon_alias_member_request(
            "alias-add-member",
            "team".to_string(),
            "pane-a".to_string(),
            None,
        );
        assert_eq!(add["command"], "alias-add-member");
        assert_eq!(add["args"]["alias"], "team");
        assert_eq!(add["args"]["pane_id"], "pane-a");

        let mode = daemon_alias_mode_request(
            "team".to_string(),
            "broadcast".to_string(),
            Some("project-a".to_string()),
        );
        assert_eq!(mode["command"], "alias-mode");
        assert_eq!(mode["args"]["mode"], "broadcast");
        assert_eq!(mode["args"]["project_id"], "project-a");
    }

    #[test]
    fn daemon_bus_requests_use_daemon_command_shape() {
        let list = daemon_bus_subscriptions_request(
            Some("reviewer".to_string()),
            Some("project-a".to_string()),
            false,
        );
        assert_eq!(list["command"], "bus-subscriptions");
        assert_eq!(list["args"]["alias"], "reviewer");
        assert_eq!(list["args"]["project_id"], "project-a");
        assert!(list["args"].get("global").is_none());

        let global = daemon_bus_subscriptions_request(None, None, true);
        assert_eq!(global["command"], "bus-subscriptions");
        assert_eq!(global["args"]["global"], true);

        let subscribe = daemon_bus_subscribe_request(
            "reviewer".to_string(),
            "build.**".to_string(),
            Some("project-a".to_string()),
        );
        assert_eq!(subscribe["command"], "bus-subscribe");
        assert_eq!(subscribe["args"]["alias"], "reviewer");
        assert_eq!(subscribe["args"]["pattern"], "build.**");
        assert_eq!(subscribe["args"]["project_id"], "project-a");

        let unsubscribe = daemon_bus_unsubscribe_request("sub-a".to_string());
        assert_eq!(unsubscribe["command"], "bus-unsubscribe");
        assert_eq!(unsubscribe["args"]["id"], "sub-a");
    }

    #[test]
    fn daemon_mailbox_requests_use_daemon_command_shape() {
        let peek = daemon_mailbox_peek_request(
            "reviewer".to_string(),
            true,
            Some("project-a".to_string()),
            false,
            Some(20),
        );
        assert_eq!(peek["command"], "mailbox-peek");
        assert_eq!(peek["args"]["alias"], "reviewer");
        assert_eq!(peek["args"]["unread"], true);
        assert_eq!(peek["args"]["project_id"], "project-a");
        assert_eq!(peek["args"]["limit"], 20);

        let tail = daemon_bus_tail_request(Some("build.done".to_string()), None, true, Some(5));
        assert_eq!(tail["command"], "bus-tail");
        assert_eq!(tail["args"]["topic"], "build.done");
        assert_eq!(tail["args"]["global"], true);
        assert_eq!(tail["args"]["limit"], 5);

        let count = daemon_mailbox_count_request("reviewer".to_string(), None, false);
        assert_eq!(count["command"], "mailbox-count");
        assert_eq!(count["args"]["alias"], "reviewer");
        assert!(count["args"].get("project_id").is_none());

        let post = daemon_mailbox_post_request(DaemonMailboxPostRequest {
            to: Some("reviewer".to_string()),
            topic: Some("build.done".to_string()),
            body: "ready".to_string(),
            subject: Some("Build".to_string()),
            kind: Some("fyi".to_string()),
            project_id: Some("project-a".to_string()),
            correlation_id: Some("corr-a".to_string()),
            structured: Some(serde_json::json!({ "ok": true })),
            from: Some("runner".to_string()),
        });
        assert_eq!(post["command"], "mailbox-post");
        assert_eq!(post["args"]["to"], "reviewer");
        assert_eq!(post["args"]["topic"], "build.done");
        assert_eq!(post["args"]["body"], "ready");
        assert_eq!(post["args"]["subject"], "Build");
        assert_eq!(post["args"]["kind"], "fyi");
        assert_eq!(post["args"]["project_id"], "project-a");
        assert_eq!(post["args"]["correlation_id"], "corr-a");
        assert_eq!(post["args"]["structured"]["ok"], true);
        assert_eq!(post["args"]["from"], "runner");

        let get = daemon_mailbox_event_id_request("mailbox-get", "event-a".to_string());
        assert_eq!(get["command"], "mailbox-get");
        assert_eq!(get["args"]["event_id"], "event-a");

        let read_state =
            daemon_mailbox_read_state_request("event-a".to_string(), "reviewer".to_string());
        assert_eq!(read_state["command"], "mailbox-read-state");
        assert_eq!(read_state["args"]["recipient"], "reviewer");

        let mark_read =
            daemon_mailbox_mark_read_request("event-a".to_string(), "reviewer".to_string());
        assert_eq!(mark_read["command"], "mailbox-mark-read");
        assert_eq!(mark_read["args"]["event_id"], "event-a");

        let ack = daemon_mailbox_alias_event_request(
            "mailbox-ack",
            "event-a".to_string(),
            "reviewer".to_string(),
            Some("done".to_string()),
        );
        assert_eq!(ack["command"], "mailbox-ack");
        assert_eq!(ack["args"]["alias"], "reviewer");
        assert_eq!(ack["args"]["result"], "done");

        let clear = daemon_mailbox_clear_request("reviewer".to_string(), None, true);
        assert_eq!(clear["command"], "mailbox-clear");
        assert_eq!(clear["args"]["global"], true);
    }

    #[test]
    fn daemon_watch_requests_use_daemon_command_shape() {
        let config = CreateWatchConfig {
            name: "HTTP".to_string(),
            kind: roux_core::WatchKind::HttpHealth {
                url: "http://localhost".to_string(),
                expected_status: 200,
            },
            mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
            scope: roux_core::WatchScope::Global,
            notify: None,
        };

        assert_eq!(daemon_watch_list_request()["command"], "watch-list");

        let create = daemon_watch_config_request("watch-create", config.clone());
        assert_eq!(create["command"], "watch-create");
        assert_eq!(create["args"]["config"]["name"], "HTTP");
        assert_eq!(create["args"]["config"]["kind"]["type"], "httpHealth");

        let find_or_create = daemon_watch_config_request("watch-find-or-create", config);
        assert_eq!(find_or_create["command"], "watch-find-or-create");

        let remove = daemon_watch_id_request("watch-remove", "watch-a".to_string());
        assert_eq!(remove["command"], "watch-remove");
        assert_eq!(remove["args"]["id"], "watch-a");

        let watch = Watch {
            id: "watch-a".to_string(),
            name: "HTTP".to_string(),
            kind: roux_core::WatchKind::HttpHealth {
                url: "http://localhost".to_string(),
                expected_status: 200,
            },
            mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
            scope: roux_core::WatchScope::Global,
            runtime_state: roux_core::RuntimeState::Active,
            last_result: None,
            last_checked: None,
            notify: roux_core::NotifyConfig::default(),
            created_at: 0,
        };
        let replace = daemon_watch_replace_request(watch);
        assert_eq!(replace["command"], "watch-replace");
        assert_eq!(replace["args"]["watch"]["id"], "watch-a");

        let session_cleanup = daemon_watch_session_request("session-a".to_string());
        assert_eq!(session_cleanup["command"], "watch-remove-for-session");
        assert_eq!(session_cleanup["args"]["sessionId"], "session-a");

        let events = daemon_watch_events_request(true);
        assert_eq!(events["command"], "watch-events");
        assert_eq!(events["args"]["backlog"], true);

        assert_eq!(daemon_mailbox_events_request()["command"], "mailbox-events");
        assert_eq!(daemon_alias_events_request()["command"], "alias-events");
        assert_eq!(daemon_subscription_events_request()["command"], "subscription-events");

        assert_eq!(daemon_watch_cleanup_orphans_request()["command"], "watch-cleanup-orphans");
    }

    #[test]
    fn daemon_process_output_request_uses_max_bytes() {
        let request = daemon_process_output_request("daemon-process-1".to_string(), Some(42));

        assert_eq!(request["command"], "daemon-process-output");
        assert_eq!(request["args"]["id"], "daemon-process-1");
        assert_eq!(request["args"]["maxBytes"], 42);
    }

    #[test]
    fn daemon_process_list_and_kill_requests_use_daemon_commands() {
        assert_eq!(daemon_process_list_request()["command"], "daemon-process-list");

        let kill = daemon_process_kill_request("daemon-process-1".to_string());
        assert_eq!(kill["command"], "daemon-process-kill");
        assert_eq!(kill["args"]["id"], "daemon-process-1");
    }

    #[test]
    fn daemon_pty_spawn_task_request_uses_daemon_command_shape() {
        let request = daemon_pty_spawn_task_request(
            "printf hi".to_string(),
            Some("pty-a".to_string()),
            Some("/tmp".to_string()),
            Some("session-a".to_string()),
            Some("pane-a".to_string()),
            Some("task".to_string()),
            Some((120, 40)),
        );

        assert_eq!(request["command"], "daemon-pty-spawn-task");
        assert_eq!(request["args"]["command"], "printf hi");
        assert_eq!(request["args"]["id"], "pty-a");
        assert_eq!(request["args"]["workingDir"], "/tmp");
        assert_eq!(request["args"]["sessionId"], "session-a");
        assert_eq!(request["args"]["paneId"], "pane-a");
        assert_eq!(request["args"]["profile"], "task");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([120, 40]));
    }

    #[test]
    fn daemon_pty_control_requests_use_daemon_commands() {
        let output = daemon_pty_output_request("pty-a".to_string(), Some(42));
        assert_eq!(output["command"], "daemon-pty-output");
        assert_eq!(output["args"]["id"], "pty-a");
        assert_eq!(output["args"]["maxBytes"], 42);

        let attach = daemon_pty_attach_request("pty-a".to_string(), Some(1024));
        assert_eq!(attach["command"], "daemon-pty-attach");
        assert_eq!(attach["args"]["id"], "pty-a");
        assert_eq!(attach["args"]["maxBytes"], 1024);

        assert_eq!(daemon_pty_list_request()["command"], "daemon-pty-list");

        let write = daemon_pty_write_request("pty-a".to_string(), "input\n".to_string());
        assert_eq!(write["command"], "daemon-pty-write");
        assert_eq!(write["args"]["data"], "input\n");

        let resize = daemon_pty_resize_request("pty-a".to_string(), 100, 30);
        assert_eq!(resize["command"], "daemon-pty-resize");
        assert_eq!(resize["args"]["cols"], 100);
        assert_eq!(resize["args"]["rows"], 30);

        let detach = daemon_pty_detach_request("pty-a".to_string());
        assert_eq!(detach["command"], "daemon-pty-detach");
        assert_eq!(detach["args"]["id"], "pty-a");

        let attach_pane = daemon_pty_attach_pane_request("pty-a".to_string(), "pane-b".to_string());
        assert_eq!(attach_pane["command"], "daemon-pty-attach-pane");
        assert_eq!(attach_pane["args"]["paneId"], "pane-b");

        let mark_read = daemon_pty_mark_read_request("pty-a".to_string());
        assert_eq!(mark_read["command"], "daemon-pty-mark-read");

        let set_name = daemon_pty_set_name_request("pty-a".to_string(), Some("Build".to_string()));
        assert_eq!(set_name["command"], "daemon-pty-set-name");
        assert_eq!(set_name["args"]["name"], "Build");
        let clear_name = daemon_pty_set_name_request("pty-a".to_string(), None);
        assert!(clear_name["args"]["name"].is_null());

        let kill = daemon_pty_kill_request("pty-a".to_string());
        assert_eq!(kill["command"], "daemon-pty-kill");
        assert_eq!(kill["args"]["id"], "pty-a");
    }

    #[test]
    fn daemon_autostart_policy_respects_external_endpoint_and_opt_out() {
        assert_eq!(daemon_autostart_disabled_reason_for(None, None), None);
        assert_eq!(daemon_autostart_disabled_reason_for(Some("1"), Some("")), None);

        assert!(daemon_autostart_disabled_reason_for(Some("0"), None)
            .unwrap()
            .contains("ROUX_DAEMON_AUTOSTART"));
        assert!(daemon_autostart_disabled_reason_for(None, Some("/tmp/remote.sock"))
            .unwrap()
            .contains("ROUX_SOCKET"));
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
