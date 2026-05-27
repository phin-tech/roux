use crate::blocking;
use crate::endpoint::{self, resolve_socket_endpoint, SocketEndpoint};
use crate::error::{RouxError, RouxResult};
use crate::protocol::CommandRequest;
use crate::requests::{CreateSessionShell, MailboxPost, ReconnectSessionShell};
use crate::streams::{
    AliasEventStreamFrame, MailboxEventStreamFrame, SubscriptionEventStreamFrame,
    WatchEventStreamFrame, WorkItemEventStreamFrame,
};
use crate::types::{DaemonStatus, PtyRecord};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Roux {
    pub(crate) endpoint: SocketEndpoint,
    pub(crate) auth_token: Option<String>,
    pub(crate) timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct RouxBuilder {
    endpoint: Option<SocketEndpoint>,
    auth_token: Option<String>,
    timeout: Duration,
}

impl Roux {
    pub fn builder() -> RouxBuilder {
        RouxBuilder::default()
    }

    pub fn connect() -> RouxResult<Self> {
        Self::builder().connect()
    }

    pub fn with_timeout(&self, timeout: Duration) -> Self {
        Self { endpoint: self.endpoint.clone(), auth_token: self.auth_token.clone(), timeout }
    }

    pub fn with_default_timeout(&self) -> Self {
        self.with_timeout(DEFAULT_TIMEOUT)
    }

    pub fn request_timeout(&self) -> Duration {
        self.timeout
    }

    pub async fn status(&self) -> RouxResult<DaemonStatus> {
        self.command(CommandRequest::new("daemon-status")).await
    }

    pub async fn status_value(&self) -> RouxResult<Value> {
        self.command(CommandRequest::new("daemon-status")).await
    }

    pub fn status_blocking(&self) -> RouxResult<Value> {
        self.command_blocking(CommandRequest::new("daemon-status"))
    }

    pub async fn sessions(&self) -> RouxResult<Vec<roux_core::Session>> {
        self.command(CommandRequest::new("session-list")).await
    }

    pub async fn get_session(&self, id: impl Into<String>) -> RouxResult<roux_core::Session> {
        self.command(CommandRequest::new("session-poll").session_id(id.into())).await
    }

    pub async fn ptys(&self) -> RouxResult<Vec<PtyRecord>> {
        self.command(CommandRequest::new("daemon-pty-list")).await
    }

    pub async fn projects(&self) -> RouxResult<Vec<roux_core::Project>> {
        self.command(CommandRequest::new("project-list")).await
    }

    pub async fn watches(&self) -> RouxResult<Vec<roux_core::Watch>> {
        self.command(CommandRequest::new("watch-list")).await
    }

    pub async fn create_project(&self, name: impl Into<String>) -> RouxResult<roux_core::Project> {
        self.command(CommandRequest::new("project-create").args(serde_json::json!({
            "name": name.into(),
        })))
        .await
    }

    pub async fn remove_project(&self, id: impl Into<String>) -> RouxResult<()> {
        let _: Value =
            self.command(CommandRequest::new("project-remove").args(id_arg(id.into()))).await?;
        Ok(())
    }

    pub async fn rename_project(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("project-rename").args(serde_json::json!({
                "id": id.into(),
                "name": name.into(),
            })))
            .await?;
        Ok(())
    }

    pub async fn update_project(
        &self,
        id: impl Into<String>,
        patch: roux_core::ProjectUpdate,
    ) -> RouxResult<roux_core::Project> {
        self.command(CommandRequest::new("project-update").args(serde_json::json!({
            "id": id.into(),
            "patch": patch,
        })))
        .await
    }

    pub async fn list_aliases(
        &self,
        project_id: Option<String>,
        global: bool,
        only_unbound: bool,
    ) -> RouxResult<Vec<roux_core::AgentAlias>> {
        self.command(CommandRequest::new("alias-list").args(alias_list_args(
            project_id,
            global,
            only_unbound,
        )))
        .await
    }

    pub async fn get_alias(
        &self,
        alias: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<Option<roux_core::AgentAlias>> {
        let alias = alias.into();
        match self
            .command(
                CommandRequest::new("alias-get").args(alias_get_args(alias.clone(), project_id)),
            )
            .await
        {
            Ok(alias) => Ok(Some(alias)),
            Err(err) if err.to_string().contains(&format!("alias '{alias}' not found")) => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn whoami_aliases(
        &self,
        session_id: impl Into<String>,
    ) -> RouxResult<Vec<roux_core::AgentAlias>> {
        self.command(
            CommandRequest::new("alias-whoami")
                .session_id(session_id.into())
                .args(serde_json::json!({})),
        )
        .await
    }

    pub async fn add_alias_member(
        &self,
        alias: impl Into<String>,
        pane_id: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<roux_core::AgentAlias> {
        self.command(CommandRequest::new("alias-add-member").args(alias_member_args(
            alias.into(),
            pane_id.into(),
            project_id,
        )))
        .await
    }

    pub async fn remove_alias_member(
        &self,
        alias: impl Into<String>,
        pane_id: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<bool> {
        let value: Value = self
            .command(CommandRequest::new("alias-remove-member").args(alias_member_args(
                alias.into(),
                pane_id.into(),
                project_id,
            )))
            .await?;
        bool_field(value, "removed")
    }

    pub async fn set_alias_mode(
        &self,
        alias: impl Into<String>,
        mode: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<roux_core::AgentAlias> {
        self.command(CommandRequest::new("alias-mode").args(alias_mode_args(
            alias.into(),
            mode.into(),
            project_id,
        )))
        .await
    }

    pub async fn list_subscriptions(
        &self,
        alias: Option<String>,
        project_id: Option<String>,
        global: bool,
    ) -> RouxResult<Vec<roux_core::BusSubscription>> {
        self.command(
            CommandRequest::new("bus-subscriptions")
                .args(subscription_list_args(alias, project_id, global)),
        )
        .await
    }

    pub async fn create_subscription(
        &self,
        alias: impl Into<String>,
        pattern: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<roux_core::BusSubscription> {
        self.command(CommandRequest::new("bus-subscribe").args(subscription_create_args(
            alias.into(),
            pattern.into(),
            project_id,
        )))
        .await
    }

    pub async fn delete_subscription(&self, id: impl Into<String>) -> RouxResult<bool> {
        let value: Value =
            self.command(CommandRequest::new("bus-unsubscribe").args(id_arg(id.into()))).await?;
        bool_field(value, "removed")
    }

    pub async fn mailbox_list_for_recipient(
        &self,
        alias: impl Into<String>,
        unread_only: bool,
        project_id: Option<String>,
        global: bool,
    ) -> RouxResult<Vec<roux_core::Event>> {
        self.command(CommandRequest::new("mailbox-peek").args(mailbox_peek_args(
            alias.into(),
            unread_only,
            project_id,
            global,
            None,
        )))
        .await
    }

    pub async fn mailbox_list_for_topic(
        &self,
        topic: impl Into<String>,
        project_id: Option<String>,
        global: bool,
    ) -> RouxResult<Vec<roux_core::Event>> {
        self.command(CommandRequest::new("bus-tail").args(bus_tail_args(
            Some(topic.into()),
            project_id,
            global,
            None,
        )))
        .await
    }

    pub async fn mailbox_list_all(
        &self,
        project_id: Option<String>,
        global: bool,
        limit: Option<u32>,
    ) -> RouxResult<Vec<roux_core::Event>> {
        self.command(
            CommandRequest::new("bus-tail").args(bus_tail_args(None, project_id, global, limit)),
        )
        .await
    }

    pub async fn mailbox_unread_count(
        &self,
        alias: impl Into<String>,
        project_id: Option<String>,
        global: bool,
    ) -> RouxResult<u32> {
        let value: Value = self
            .command(CommandRequest::new("mailbox-count").args(mailbox_count_args(
                alias.into(),
                project_id,
                global,
            )))
            .await?;
        u32_field(value, "unread")
    }

    pub async fn mailbox_get_event(
        &self,
        event_id: impl Into<String>,
    ) -> RouxResult<Option<roux_core::Event>> {
        self.command(CommandRequest::new("mailbox-get").args(event_id_arg(event_id.into()))).await
    }

    pub async fn mailbox_read_state(
        &self,
        event_id: impl Into<String>,
        recipient: impl Into<String>,
    ) -> RouxResult<Option<roux_core::ReadState>> {
        self.command(CommandRequest::new("mailbox-read-state").args(serde_json::json!({
            "event_id": event_id.into(),
            "recipient": recipient.into(),
        })))
        .await
    }

    pub async fn mailbox_post(&self, request: MailboxPost) -> RouxResult<roux_core::Event> {
        self.command(CommandRequest::new("mailbox-post").args(request.into_args())).await
    }

    pub async fn mailbox_mark_read(
        &self,
        event_id: impl Into<String>,
        recipient: impl Into<String>,
    ) -> RouxResult<bool> {
        let value: Value = self
            .command(CommandRequest::new("mailbox-mark-read").args(serde_json::json!({
                "event_id": event_id.into(),
                "recipient": recipient.into(),
            })))
            .await?;
        bool_field(value, "changed")
    }

    pub async fn mailbox_ack(
        &self,
        event_id: impl Into<String>,
        recipient: impl Into<String>,
        result: Option<String>,
    ) -> RouxResult<bool> {
        let value: Value = self
            .command(CommandRequest::new("mailbox-ack").args(mailbox_alias_event_args(
                event_id.into(),
                recipient.into(),
                result,
            )))
            .await?;
        bool_field(value, "changed")
    }

    pub async fn mailbox_clear_read(
        &self,
        recipient: impl Into<String>,
        project_id: Option<String>,
        global: bool,
    ) -> RouxResult<u32> {
        let value: Value = self
            .command(CommandRequest::new("mailbox-clear").args(mailbox_clear_args(
                recipient.into(),
                project_id,
                global,
            )))
            .await?;
        u32_field(value, "cleared")
    }

    pub async fn mailbox_retract(
        &self,
        event_id: impl Into<String>,
        sender: impl Into<String>,
    ) -> RouxResult<roux_core::Event> {
        self.command(CommandRequest::new("mailbox-retract").args(mailbox_alias_event_args(
            event_id.into(),
            sender.into(),
            None,
        )))
        .await
    }

    pub async fn mailbox_dismiss(
        &self,
        event_id: impl Into<String>,
        recipient: impl Into<String>,
    ) -> RouxResult<bool> {
        let value: Value = self
            .command(CommandRequest::new("mailbox-dismiss").args(mailbox_alias_event_args(
                event_id.into(),
                recipient.into(),
                None,
            )))
            .await?;
        bool_field(value, "changed")
    }

    pub async fn create_session_shell(
        &self,
        request: CreateSessionShell,
    ) -> RouxResult<roux_core::Session> {
        self.command(CommandRequest::new("session-create-shell").args(request.into_args())).await
    }

    pub async fn reconnect_session_shell(
        &self,
        request: ReconnectSessionShell,
    ) -> RouxResult<roux_core::Session> {
        let session_id = request.id.clone();
        self.command(
            CommandRequest::new("session-reconnect-shell")
                .session_id(session_id)
                .args(request.into_args()),
        )
        .await
    }

    pub async fn read_notes<Target, Output>(&self, target: Target) -> RouxResult<Output>
    where
        Target: Serialize,
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("notes-read").args(to_value(target)?)).await
    }

    pub async fn write_notes<Target>(
        &self,
        target: Target,
        content: impl Into<String>,
        tags: Vec<String>,
    ) -> RouxResult<()>
    where
        Target: Serialize,
    {
        let _: Value = self
            .command(CommandRequest::new("notes-write").args(serde_json::json!({
                "target": to_value(target)?,
                "content": content.into(),
                "tags": tags,
            })))
            .await?;
        Ok(())
    }

    pub async fn append_notes<Target>(
        &self,
        target: Target,
        content: impl Into<String>,
        timestamped: bool,
        tags: Vec<String>,
    ) -> RouxResult<()>
    where
        Target: Serialize,
    {
        let _: Value = self
            .command(CommandRequest::new("notes-append").args(serde_json::json!({
                "target": to_value(target)?,
                "content": content.into(),
                "timestamped": timestamped,
                "tags": tags,
            })))
            .await?;
        Ok(())
    }

    pub async fn notes_path<Target>(&self, target: Target, dir: bool) -> RouxResult<String>
    where
        Target: Serialize,
    {
        self.command(CommandRequest::new("notes-path").args(serde_json::json!({
            "target": to_value(target)?,
            "dir": dir,
        })))
        .await
    }

    pub async fn search_notes<Query>(&self, query: Query) -> RouxResult<Vec<String>>
    where
        Query: Serialize,
    {
        self.command(CommandRequest::new("notes-search").args(to_value(query)?)).await
    }

    pub async fn notes_vault_root(&self) -> RouxResult<String> {
        self.command(CommandRequest::new("notes-vault-root")).await
    }

    pub async fn list_automation_hooks<Output>(
        &self,
        repo_path: Option<String>,
    ) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("hook-show").args(optional_repo_path_args(repo_path)))
            .await
    }

    pub async fn preview_automation_hooks<Request, Output>(
        &self,
        request: Request,
    ) -> RouxResult<Output>
    where
        Request: Serialize,
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("hook-preview").args(to_value(request)?)).await
    }

    pub async fn run_automation_hook<Request, Output>(&self, request: Request) -> RouxResult<Output>
    where
        Request: Serialize,
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("hook-run").args(to_value(request)?)).await
    }

    pub async fn approve_automation_hook(&self, approval_id: impl Into<String>) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("hook-approve").args(serde_json::json!({
                "approvalId": approval_id.into(),
            })))
            .await?;
        Ok(())
    }

    pub async fn clear_automation_hook_approvals(&self) -> RouxResult<()> {
        let _: Value = self.command(CommandRequest::new("hook-clear-approvals")).await?;
        Ok(())
    }

    pub async fn list_automation_hook_logs<Output>(&self) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("hook-log-list")).await
    }

    pub async fn read_automation_hook_log(&self, path: impl Into<String>) -> RouxResult<String> {
        self.command(CommandRequest::new("hook-log-read").args(serde_json::json!({
            "path": path.into(),
        })))
        .await
    }

    pub async fn set_session_name_override(
        &self,
        session_id: impl Into<String>,
        name_override: Option<String>,
    ) -> RouxResult<()> {
        let _: Value = self
            .command(
                CommandRequest::new("session-rename")
                    .session_id(session_id.into())
                    .args(serde_json::json!({ "name": name_override.unwrap_or_default() })),
            )
            .await?;
        Ok(())
    }

    pub async fn set_session_project(
        &self,
        session_id: impl Into<String>,
        project_id: Option<String>,
    ) -> RouxResult<()> {
        self.session_optional_value(
            session_id.into(),
            "session-set-project",
            "projectId",
            project_id,
        )
        .await
    }

    pub async fn set_session_pinned_pr_url(
        &self,
        session_id: impl Into<String>,
        url: Option<String>,
    ) -> RouxResult<()> {
        self.session_optional_value(session_id.into(), "session-set-pinned-pr-url", "url", url)
            .await
    }

    pub async fn set_session_smol_machine(
        &self,
        session_id: impl Into<String>,
        machine_name: Option<String>,
    ) -> RouxResult<()> {
        self.session_optional_value(
            session_id.into(),
            "session-set-smol-machine",
            "machineName",
            machine_name,
        )
        .await
    }

    async fn session_optional_value(
        &self,
        session_id: String,
        command: &'static str,
        key: &'static str,
        value: Option<String>,
    ) -> RouxResult<()> {
        let _: Value = self
            .command(
                CommandRequest::new(command)
                    .session_id(session_id)
                    .args(optional_string_arg(key, value)),
            )
            .await?;
        Ok(())
    }

    pub async fn archive_session(&self, id: impl Into<String>) -> RouxResult<roux_core::Session> {
        self.command(CommandRequest::new("session-archive").session_id(id.into())).await
    }

    pub async fn restore_session(&self, id: impl Into<String>) -> RouxResult<roux_core::Session> {
        self.command(CommandRequest::new("session-restore").session_id(id.into())).await
    }

    pub async fn delete_session(&self, id: impl Into<String>) -> RouxResult<()> {
        let _: Value =
            self.command(CommandRequest::new("session-delete").session_id(id.into())).await?;
        Ok(())
    }

    pub async fn session_worktree_exists(&self, id: impl Into<String>) -> RouxResult<bool> {
        let value: Value = self
            .command(CommandRequest::new("session-worktree-exists").session_id(id.into()))
            .await?;
        bool_field(value, "exists")
    }

    pub async fn refresh_session_branch(
        &self,
        id: impl Into<String>,
    ) -> RouxResult<Option<String>> {
        let value: Value = self
            .command(CommandRequest::new("session-refresh-branch").session_id(id.into()))
            .await?;
        Ok(value.get("branch").and_then(|branch| branch.as_str()).map(str::to_string))
    }

    pub async fn list_worktrees(
        &self,
        repo_path: impl Into<String>,
    ) -> RouxResult<Vec<roux_core::Worktree>> {
        self.command(CommandRequest::new("worktree-list").args(repo_path_arg(repo_path.into())))
            .await
    }

    pub async fn create_worktree(
        &self,
        repo_path: impl Into<String>,
        branch: impl Into<String>,
        start_point: Option<String>,
        fetch_first: bool,
    ) -> RouxResult<String> {
        let value: Value = self
            .command(CommandRequest::new("worktree-create").args(worktree_create_args(
                repo_path.into(),
                branch.into(),
                start_point,
                fetch_first,
            )))
            .await?;
        string_field(value, "path")
    }

    pub async fn remove_worktree(
        &self,
        repo_path: impl Into<String>,
        worktree_path: impl Into<String>,
        also_branch: bool,
        force: bool,
    ) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("worktree-remove").args(serde_json::json!({
                "repoPath": repo_path.into(),
                "worktreePath": worktree_path.into(),
                "alsoBranch": also_branch,
                "force": force,
            })))
            .await?;
        Ok(())
    }

    pub async fn list_branches(&self, repo_path: impl Into<String>) -> RouxResult<Vec<String>> {
        self.command(
            CommandRequest::new("worktree-list-branches").args(repo_path_arg(repo_path.into())),
        )
        .await
    }

    pub async fn git_init(&self, path: impl Into<String>) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("git-init").args(serde_json::json!({
                "path": path.into(),
            })))
            .await?;
        Ok(())
    }

    pub async fn create_watch(
        &self,
        config: roux_core::CreateWatchConfig,
    ) -> RouxResult<roux_core::Watch> {
        self.command(CommandRequest::new("watch-create").args(watch_config_arg(config))).await
    }

    pub async fn find_or_create_watch(
        &self,
        config: roux_core::CreateWatchConfig,
    ) -> RouxResult<roux_core::Watch> {
        self.command(CommandRequest::new("watch-find-or-create").args(watch_config_arg(config)))
            .await
    }

    pub async fn remove_watch(&self, id: impl Into<String>) -> RouxResult<()> {
        let _: Value =
            self.command(CommandRequest::new("watch-remove").args(id_arg(id.into()))).await?;
        Ok(())
    }

    pub async fn pause_watch(&self, id: impl Into<String>) -> RouxResult<roux_core::Watch> {
        self.command(CommandRequest::new("watch-pause").args(id_arg(id.into()))).await
    }

    pub async fn resume_watch(&self, id: impl Into<String>) -> RouxResult<roux_core::Watch> {
        self.command(CommandRequest::new("watch-resume").args(id_arg(id.into()))).await
    }

    pub async fn replace_watch(&self, watch: roux_core::Watch) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("watch-replace").args(serde_json::json!({
                "watch": watch,
            })))
            .await?;
        Ok(())
    }

    pub async fn remove_watches_for_session(
        &self,
        session_id: impl Into<String>,
    ) -> RouxResult<()> {
        let _: Value = self
            .command(CommandRequest::new("watch-remove-for-session").args(serde_json::json!({
                "sessionId": session_id.into(),
            })))
            .await?;
        Ok(())
    }

    pub async fn cleanup_watch_orphans(&self) -> RouxResult<()> {
        let _: Value = self.command(CommandRequest::new("watch-cleanup-orphans")).await?;
        Ok(())
    }

    pub async fn start_daemon_process<Output>(
        &self,
        command: impl Into<String>,
        working_dir: Option<String>,
    ) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(
            CommandRequest::new("daemon-process-start")
                .args(process_start_args(command.into(), working_dir)),
        )
        .await
    }

    pub async fn daemon_process_output<Output>(
        &self,
        id: impl Into<String>,
        max_bytes: Option<usize>,
    ) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(
            CommandRequest::new("daemon-process-output").args(max_bytes_args(id.into(), max_bytes)),
        )
        .await
    }

    pub async fn list_daemon_processes<Output>(&self) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("daemon-process-list")).await
    }

    pub async fn kill_daemon_process<Output>(&self, id: impl Into<String>) -> RouxResult<Output>
    where
        Output: DeserializeOwned + Send + 'static,
    {
        self.command(CommandRequest::new("daemon-process-kill").args(id_arg(id.into()))).await
    }

    pub async fn work_item_list(
        &self,
        project_id: Option<impl Into<String>>,
    ) -> RouxResult<Vec<roux_core::WorkItem>> {
        let args = match project_id {
            Some(pid) => serde_json::json!({ "projectId": pid.into() }),
            None => serde_json::json!({}),
        };
        self.command(CommandRequest::new("work-item-list").args(args)).await
    }

    pub async fn work_item_create(
        &self,
        input: roux_core::WorkItemInput,
    ) -> RouxResult<roux_core::WorkItem> {
        self.command(
            CommandRequest::new("work-item-create")
                .args(serde_json::to_value(input).map_err(RouxError::Decode)?),
        )
        .await
    }

    pub async fn work_item_update(
        &self,
        id: impl Into<String>,
        input: roux_core::WorkItemInput,
    ) -> RouxResult<roux_core::WorkItem> {
        let mut args = serde_json::to_value(input).map_err(RouxError::Decode)?;
        args["id"] = serde_json::Value::String(id.into());
        self.command(CommandRequest::new("work-item-update").args(args)).await
    }

    pub async fn work_item_move(
        &self,
        id: impl Into<String>,
        status: roux_core::WorkItemStatus,
        sort_order: f64,
    ) -> RouxResult<roux_core::WorkItem> {
        self.command(CommandRequest::new("work-item-move").args(serde_json::json!({
            "id": id.into(),
            "status": status.as_str(),
            "sortOrder": sort_order,
        })))
        .await
    }

    pub async fn work_item_delete(&self, id: impl Into<String>) -> RouxResult<String> {
        let value: Value =
            self.command(CommandRequest::new("work-item-delete").args(id_arg(id.into()))).await?;
        value
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| RouxError::Command("missing id in delete response".to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn work_item_start(
        &self,
        id: impl Into<String>,
        profile: Option<String>,
        repo_path: Option<String>,
        name: Option<String>,
        worktree_path: Option<String>,
        branch: Option<String>,
        base: Option<String>,
        fetch_first: Option<bool>,
    ) -> RouxResult<roux_core::WorkItemStartResult> {
        let args = work_item_start_args(
            id.into(),
            profile,
            repo_path,
            name,
            worktree_path,
            branch,
            base,
            fetch_first,
        );
        self.command(CommandRequest::new("work-item-start").args(args)).await
    }

    pub async fn work_item_plan(
        &self,
        id: impl Into<String>,
        profile: Option<String>,
        repo_path: Option<String>,
        name: Option<String>,
        worktree_path: Option<String>,
    ) -> RouxResult<roux_core::WorkItemPlanResult> {
        let args = work_item_plan_args(id.into(), profile, repo_path, name, worktree_path);
        self.command(CommandRequest::new("work-item-plan").args(args)).await
    }

    pub async fn work_item_review_accept(
        &self,
        id: impl Into<String>,
    ) -> RouxResult<roux_core::WorkItemReviewAcceptResult> {
        self.command(CommandRequest::new("work-item-review-accept").args(id_arg(id.into()))).await
    }

    pub async fn work_item_runs_list(
        &self,
        work_item_id: Option<impl Into<String>>,
    ) -> RouxResult<Vec<roux_core::WorkItemRun>> {
        let args = match work_item_id {
            Some(id) => serde_json::json!({ "workItemId": id.into() }),
            None => serde_json::json!({}),
        };
        self.command(CommandRequest::new("work-item-runs-list").args(args)).await
    }

    pub async fn work_item_run_events(
        &self,
        run_id: impl Into<String>,
    ) -> RouxResult<Vec<roux_core::WorkItemRunEvent>> {
        self.command(
            CommandRequest::new("work-item-run-events")
                .args(serde_json::json!({ "runId": run_id.into() })),
        )
        .await
    }

    pub async fn work_item_run_stop(
        &self,
        run_id: impl Into<String>,
    ) -> RouxResult<roux_core::WorkItemRun> {
        self.command(
            CommandRequest::new("work-item-run-stop")
                .args(serde_json::json!({ "runId": run_id.into() })),
        )
        .await
    }

    pub async fn work_item_decision_create(
        &self,
        run_id: impl Into<String>,
        question: impl Into<String>,
        options: Vec<roux_core::WorkItemDecisionOption>,
        default_value: Option<String>,
        timeout_at: Option<u64>,
    ) -> RouxResult<roux_core::WorkItemDecision> {
        let mut args = serde_json::json!({
            "runId": run_id.into(),
            "question": question.into(),
            "options": options,
        });
        if let Some(default_value) = default_value {
            args["defaultValue"] = serde_json::Value::String(default_value);
        }
        if let Some(timeout_at) = timeout_at {
            args["timeoutAt"] = serde_json::Value::Number(timeout_at.into());
        }
        self.command(CommandRequest::new("work-item-decision-create").args(args)).await
    }

    pub async fn work_item_decisions_list(
        &self,
        work_item_id: Option<impl Into<String>>,
    ) -> RouxResult<Vec<roux_core::WorkItemDecision>> {
        let args = match work_item_id {
            Some(id) => serde_json::json!({ "workItemId": id.into() }),
            None => serde_json::json!({}),
        };
        self.command(CommandRequest::new("work-item-decisions-list").args(args)).await
    }

    pub async fn work_item_decision_resolve(
        &self,
        id: impl Into<String>,
        value: impl Into<String>,
        resolved_by: Option<String>,
    ) -> RouxResult<roux_core::WorkItemDecision> {
        let mut args = serde_json::json!({
            "id": id.into(),
            "value": value.into(),
        });
        if let Some(resolved_by) = resolved_by {
            args["resolvedBy"] = serde_json::Value::String(resolved_by);
        }
        self.command(CommandRequest::new("work-item-decision-resolve").args(args)).await
    }

    pub async fn command<T>(&self, request: CommandRequest) -> RouxResult<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let client = self.clone();
        tokio::task::spawn_blocking(move || {
            let value = client.command_blocking(request)?;
            serde_json::from_value(value).map_err(RouxError::Decode)
        })
        .await
        .map_err(|err| RouxError::Transport(format!("SDK task join failed: {err}")))?
    }

    pub fn command_blocking(&self, request: CommandRequest) -> RouxResult<Value> {
        let response = blocking::send_request(
            &self.endpoint,
            self.auth_token.as_deref(),
            self.timeout,
            request,
        )?;
        response.into_result()
    }

    pub fn stream_lines_blocking<F>(&self, request: CommandRequest, on_line: F) -> RouxResult<()>
    where
        F: FnMut(&str) -> bool,
    {
        blocking::stream_client_request(
            &self.endpoint,
            self.auth_token.as_deref(),
            request,
            on_line,
        )
    }

    pub async fn stream_lines<F>(&self, request: CommandRequest, on_line: F) -> RouxResult<()>
    where
        F: FnMut(&str) -> bool + Send + 'static,
    {
        let client = self.clone();
        tokio::task::spawn_blocking(move || client.stream_lines_blocking(request, on_line))
            .await
            .map_err(|err| RouxError::Transport(format!("SDK task join failed: {err}")))?
    }

    pub fn watch_events_blocking<F>(&self, backlog: bool, on_frame: F) -> RouxResult<()>
    where
        F: FnMut(WatchEventStreamFrame) -> bool,
    {
        self.stream_json_frames_blocking(
            CommandRequest::new("watch-events").args(serde_json::json!({ "backlog": backlog })),
            on_frame,
        )
    }

    pub fn mailbox_events_blocking<F>(&self, on_frame: F) -> RouxResult<()>
    where
        F: FnMut(MailboxEventStreamFrame) -> bool,
    {
        self.stream_json_frames_blocking(CommandRequest::new("mailbox-events"), on_frame)
    }

    pub fn alias_events_blocking<F>(&self, on_frame: F) -> RouxResult<()>
    where
        F: FnMut(AliasEventStreamFrame) -> bool,
    {
        self.stream_json_frames_blocking(CommandRequest::new("alias-events"), on_frame)
    }

    pub fn subscription_events_blocking<F>(&self, on_frame: F) -> RouxResult<()>
    where
        F: FnMut(SubscriptionEventStreamFrame) -> bool,
    {
        self.stream_json_frames_blocking(CommandRequest::new("subscription-events"), on_frame)
    }

    pub fn work_item_events_blocking<F>(&self, on_frame: F) -> RouxResult<()>
    where
        F: FnMut(WorkItemEventStreamFrame) -> bool,
    {
        self.stream_json_frames_blocking(CommandRequest::new("work-item-events"), on_frame)
    }

    fn stream_json_frames_blocking<T, F>(
        &self,
        request: CommandRequest,
        mut on_frame: F,
    ) -> RouxResult<()>
    where
        T: DeserializeOwned,
        F: FnMut(T) -> bool,
    {
        let mut parse_error = None;
        let result = self.stream_lines_blocking(request, |line| match serde_json::from_str(line) {
            Ok(frame) => on_frame(frame),
            Err(err) => {
                if parse_error.is_none() {
                    parse_error = Some(RouxError::Decode(err));
                }
                true
            }
        });
        result.and(match parse_error {
            Some(err) => Err(err),
            None => Ok(()),
        })
    }
}

impl Default for RouxBuilder {
    fn default() -> Self {
        Self { endpoint: None, auth_token: None, timeout: DEFAULT_TIMEOUT }
    }
}

impl RouxBuilder {
    pub fn endpoint(mut self, endpoint: SocketEndpoint) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn auth_token(mut self, auth_token: impl Into<String>) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn connect(self) -> RouxResult<Roux> {
        let endpoint =
            self.endpoint.or_else(resolve_socket_endpoint).ok_or(RouxError::NotRunning)?;
        let auth_token = self.auth_token.or_else(endpoint::load_socket_auth_token);
        Ok(Roux { endpoint, auth_token, timeout: self.timeout })
    }
}

fn to_value<T: Serialize>(value: T) -> RouxResult<Value> {
    serde_json::to_value(value).map_err(RouxError::Decode)
}

fn id_arg(id: String) -> Value {
    serde_json::json!({ "id": id })
}

fn event_id_arg(event_id: String) -> Value {
    serde_json::json!({ "event_id": event_id })
}

fn repo_path_arg(repo_path: String) -> Value {
    serde_json::json!({ "repoPath": repo_path })
}

fn optional_repo_path_args(repo_path: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    insert_optional_string(&mut args, "repoPath", repo_path);
    Value::Object(args)
}

#[allow(clippy::too_many_arguments)]
fn work_item_start_args(
    id: String,
    profile: Option<String>,
    repo_path: Option<String>,
    name: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    fetch_first: Option<bool>,
) -> Value {
    let mut args = serde_json::json!({ "id": id });
    if let Some(profile) = profile {
        args["profile"] = Value::String(profile);
    }
    if let Some(repo_path) = repo_path {
        args["repoPath"] = Value::String(repo_path);
    }
    if let Some(name) = name {
        args["name"] = Value::String(name);
    }
    if let Some(worktree_path) = worktree_path {
        args["worktreePath"] = Value::String(worktree_path);
    }
    if let Some(branch) = branch {
        args["branch"] = Value::String(branch);
    }
    if let Some(base) = base {
        args["base"] = Value::String(base);
    }
    if let Some(fetch_first) = fetch_first {
        args["fetchFirst"] = Value::Bool(fetch_first);
    }
    args
}

fn work_item_plan_args(
    id: String,
    profile: Option<String>,
    repo_path: Option<String>,
    name: Option<String>,
    worktree_path: Option<String>,
) -> Value {
    let mut args = serde_json::json!({ "id": id });
    if let Some(profile) = profile {
        args["profile"] = Value::String(profile);
    }
    if let Some(repo_path) = repo_path {
        args["repoPath"] = Value::String(repo_path);
    }
    if let Some(name) = name {
        args["name"] = Value::String(name);
    }
    if let Some(worktree_path) = worktree_path {
        args["worktreePath"] = Value::String(worktree_path);
    }
    args
}

fn optional_string_arg(key: &'static str, value: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert(key.into(), value.map(Value::String).unwrap_or(Value::Null));
    Value::Object(args)
}

fn bool_field(value: Value, key: &'static str) -> RouxResult<bool> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| RouxError::Transport(format!("daemon response missing boolean `{key}`")))
}

fn u32_field(value: Value, key: &'static str) -> RouxResult<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| RouxError::Transport(format!("daemon response missing u32 `{key}`")))
}

fn string_field(value: Value, key: &'static str) -> RouxResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RouxError::Transport(format!("daemon response missing string `{key}`")))
}

fn insert_project_filter_args(
    args: &mut serde_json::Map<String, Value>,
    project_id: Option<String>,
    global: bool,
) {
    insert_optional_string(args, "project_id", project_id);
    if global {
        args.insert("global".into(), Value::Bool(true));
    }
}

fn alias_list_args(project_id: Option<String>, global: bool, only_unbound: bool) -> Value {
    let mut args = serde_json::Map::new();
    insert_project_filter_args(&mut args, project_id, global);
    if only_unbound {
        args.insert("only_unbound".into(), Value::Bool(true));
    }
    Value::Object(args)
}

fn alias_get_args(alias: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    insert_optional_string(&mut args, "project_id", project_id);
    Value::Object(args)
}

fn alias_member_args(alias: String, pane_id: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    args.insert("pane_id".into(), Value::String(pane_id));
    insert_optional_string(&mut args, "project_id", project_id);
    Value::Object(args)
}

fn alias_mode_args(alias: String, mode: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    args.insert("mode".into(), Value::String(mode));
    insert_optional_string(&mut args, "project_id", project_id);
    Value::Object(args)
}

fn subscription_list_args(
    alias: Option<String>,
    project_id: Option<String>,
    global: bool,
) -> Value {
    let mut args = serde_json::Map::new();
    insert_optional_string(&mut args, "alias", alias);
    insert_project_filter_args(&mut args, project_id, global);
    Value::Object(args)
}

fn subscription_create_args(alias: String, pattern: String, project_id: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    args.insert("pattern".into(), Value::String(pattern));
    insert_optional_string(&mut args, "project_id", project_id);
    Value::Object(args)
}

fn mailbox_peek_args(
    alias: String,
    unread_only: bool,
    project_id: Option<String>,
    global: bool,
    limit: Option<u32>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    if unread_only {
        args.insert("unread".into(), Value::Bool(true));
    }
    insert_project_filter_args(&mut args, project_id, global);
    if let Some(limit) = limit {
        args.insert("limit".into(), serde_json::json!(limit));
    }
    Value::Object(args)
}

fn bus_tail_args(
    topic: Option<String>,
    project_id: Option<String>,
    global: bool,
    limit: Option<u32>,
) -> Value {
    let mut args = serde_json::Map::new();
    insert_optional_string(&mut args, "topic", topic);
    insert_project_filter_args(&mut args, project_id, global);
    if let Some(limit) = limit {
        args.insert("limit".into(), serde_json::json!(limit));
    }
    Value::Object(args)
}

fn mailbox_count_args(alias: String, project_id: Option<String>, global: bool) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    insert_project_filter_args(&mut args, project_id, global);
    Value::Object(args)
}

fn mailbox_alias_event_args(event_id: String, alias: String, result: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("event_id".into(), Value::String(event_id));
    args.insert("alias".into(), Value::String(alias));
    insert_optional_string(&mut args, "result", result);
    Value::Object(args)
}

fn mailbox_clear_args(alias: String, project_id: Option<String>, global: bool) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("alias".into(), Value::String(alias));
    insert_project_filter_args(&mut args, project_id, global);
    Value::Object(args)
}

fn worktree_create_args(
    repo_path: String,
    branch: String,
    start_point: Option<String>,
    fetch_first: bool,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("repoPath".into(), Value::String(repo_path));
    args.insert("branch".into(), Value::String(branch));
    insert_optional_string(&mut args, "startPoint", start_point);
    if fetch_first {
        args.insert("fetchFirst".into(), Value::Bool(true));
    }
    Value::Object(args)
}

fn watch_config_arg(config: roux_core::CreateWatchConfig) -> Value {
    serde_json::json!({ "config": config })
}

fn process_start_args(command: String, working_dir: Option<String>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("command".into(), Value::String(command));
    insert_optional_string(&mut args, "workingDir", working_dir);
    Value::Object(args)
}

fn max_bytes_args(id: String, max_bytes: Option<usize>) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), Value::String(id));
    if let Some(max_bytes) = max_bytes {
        args.insert("maxBytes".into(), serde_json::json!(max_bytes));
    }
    Value::Object(args)
}

fn insert_optional_string(
    args: &mut serde_json::Map<String, Value>,
    key: &'static str,
    value: Option<String>,
) {
    if let Some(value) = value {
        args.insert(key.into(), Value::String(value));
    }
}
