//! Typed Rust SDK for Roux's daemon-first local API.

mod endpoint;
mod error;
mod protocol;
mod types;

pub mod blocking;

pub use endpoint::{parse_socket_endpoint, resolve_socket_endpoint, SocketEndpoint};
pub use error::{RouxError, RouxResult};
pub use protocol::{CommandRequest, CommandResponse};
pub use types::{DaemonStatus, PtyAttachFrame, PtyKind, PtyRecord, PtySnapshot};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Roux {
    endpoint: SocketEndpoint,
    auth_token: Option<String>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct RouxBuilder {
    endpoint: Option<SocketEndpoint>,
    auth_token: Option<String>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct Session {
    client: Roux,
    session: roux_core::Session,
}

#[derive(Debug, Clone)]
pub struct Pty {
    client: Roux,
    id: String,
}

#[derive(Debug, Clone)]
pub struct SpawnTask {
    client: Roux,
    command: String,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    initial_size: Option<(u16, u16)>,
}

#[derive(Debug, Clone)]
pub struct SpawnShell {
    client: Roux,
    id: Option<String>,
    working_dir: Option<String>,
    session_id: Option<String>,
    pane_id: Option<String>,
    profile: Option<String>,
    nono_profile: Option<String>,
    nono_allow_dirs: Vec<String>,
    initial_size: Option<(u16, u16)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesEnv {
    pub vault_root: String,
    pub session_slug: String,
    pub repo_slug: String,
    pub project_slug: Option<String>,
    pub context_paths: Vec<String>,
    pub project_prompt: String,
}

#[derive(Debug, Clone)]
pub struct CreateSessionShell {
    pub id: String,
    pub repo_path: String,
    pub name: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fetch_first: bool,
    pub profile: Option<String>,
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Vec<String>,
    pub initial_size: Option<(u16, u16)>,
    pub project_id: Option<String>,
    pub blueprint_id: Option<String>,
    pub smol_machine_name: Option<String>,
    pub notes: Option<NotesEnv>,
}

#[derive(Debug, Clone)]
pub struct ReconnectSessionShell {
    pub id: String,
    pub profile: Option<String>,
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Vec<String>,
    pub initial_size: Option<(u16, u16)>,
    pub notes: Option<NotesEnv>,
}

#[derive(Debug, Clone, Default)]
pub struct MailboxPost {
    pub to: Option<String>,
    pub topic: Option<String>,
    pub body: String,
    pub subject: Option<String>,
    pub kind: Option<String>,
    pub project_id: Option<String>,
    pub correlation_id: Option<String>,
    pub structured: Option<Value>,
    pub from: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WatchEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "update")]
    Update { event: Box<roux_core::WatchUpdateEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum MailboxEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: Box<roux_core::MailboxEvent> },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum AliasEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: roux_core::AliasEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SubscriptionEventStreamFrame {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "event")]
    Event { event: roux_core::BusSubscriptionEvent },
    #[serde(rename = "warning")]
    Warning { message: String },
    #[serde(rename = "error")]
    Error { error: String },
}

impl Roux {
    pub fn builder() -> RouxBuilder {
        RouxBuilder::default()
    }

    pub fn connect() -> RouxResult<Self> {
        Self::builder().connect()
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

    pub fn session(&self, session: roux_core::Session) -> Session {
        Session { client: self.clone(), session }
    }

    pub fn pty(&self, id: impl Into<String>) -> Pty {
        Pty { client: self.clone(), id: id.into() }
    }

    pub fn spawn_task(&self, command: impl Into<String>) -> SpawnTask {
        SpawnTask {
            client: self.clone(),
            command: command.into(),
            id: None,
            working_dir: None,
            session_id: None,
            pane_id: None,
            profile: None,
            initial_size: None,
        }
    }

    pub fn spawn_shell(&self) -> SpawnShell {
        SpawnShell {
            client: self.clone(),
            id: None,
            working_dir: None,
            session_id: None,
            pane_id: None,
            profile: None,
            nono_profile: None,
            nono_allow_dirs: Vec::new(),
            initial_size: None,
        }
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
                parse_error = Some(RouxError::Decode(err));
                false
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
        Self { endpoint: None, auth_token: None, timeout: Duration::from_secs(5) }
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

impl CreateSessionShell {
    fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), Value::String(self.id));
        args.insert("repoPath".into(), Value::String(self.repo_path));
        args.insert("name".into(), Value::String(self.name));
        insert_optional_string(&mut args, "worktreePath", self.worktree_path);
        insert_optional_string(&mut args, "branch", self.branch);
        insert_optional_string(&mut args, "base", self.base);
        if self.fetch_first {
            args.insert("fetchFirst".into(), Value::Bool(true));
        }
        insert_optional_string(&mut args, "profile", self.profile);
        insert_optional_string(&mut args, "nonoProfile", self.nono_profile);
        if !self.nono_allow_dirs.is_empty() {
            args.insert("nonoAllowDirs".into(), serde_json::json!(self.nono_allow_dirs));
        }
        insert_initial_size(&mut args, self.initial_size);
        insert_optional_string(&mut args, "projectId", self.project_id);
        insert_optional_string(&mut args, "blueprintId", self.blueprint_id);
        insert_optional_string(&mut args, "smolMachineName", self.smol_machine_name);
        insert_notes_env(&mut args, self.notes);
        Value::Object(args)
    }
}

impl ReconnectSessionShell {
    fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        insert_optional_string(&mut args, "profile", self.profile);
        insert_optional_string(&mut args, "nonoProfile", self.nono_profile);
        if !self.nono_allow_dirs.is_empty() {
            args.insert("nonoAllowDirs".into(), serde_json::json!(self.nono_allow_dirs));
        }
        insert_initial_size(&mut args, self.initial_size);
        insert_notes_env(&mut args, self.notes);
        Value::Object(args)
    }
}

impl MailboxPost {
    fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        insert_optional_string(&mut args, "to", self.to);
        insert_optional_string(&mut args, "topic", self.topic);
        args.insert("body".into(), Value::String(self.body));
        insert_optional_string(&mut args, "subject", self.subject);
        insert_optional_string(&mut args, "kind", self.kind);
        insert_optional_string(&mut args, "project_id", self.project_id);
        insert_optional_string(&mut args, "correlation_id", self.correlation_id);
        if let Some(structured) = self.structured {
            args.insert("structured".into(), structured);
        }
        insert_optional_string(&mut args, "from", self.from);
        Value::Object(args)
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

fn insert_initial_size(
    args: &mut serde_json::Map<String, Value>,
    initial_size: Option<(u16, u16)>,
) {
    if let Some((cols, rows)) = initial_size {
        args.insert("initialSize".into(), serde_json::json!([cols, rows]));
    }
}

fn insert_notes_env(args: &mut serde_json::Map<String, Value>, notes: Option<NotesEnv>) {
    if let Some(notes) = notes {
        args.insert(
            "notesEnv".into(),
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
}

impl Session {
    pub fn id(&self) -> &str {
        &self.session.id
    }

    pub fn info(&self) -> &roux_core::Session {
        &self.session
    }

    pub async fn refresh(&mut self) -> RouxResult<&roux_core::Session> {
        let session: roux_core::Session =
            self.client.command(CommandRequest::new("session-poll").session_id(self.id())).await?;
        self.session = session;
        Ok(&self.session)
    }

    pub async fn send_text(&self, text: impl Into<String>, enter: bool) -> RouxResult<PtyWrite> {
        self.client
            .command(
                CommandRequest::new("send")
                    .session_id(self.id())
                    .args(serde_json::json!({ "text": text.into(), "enter": enter })),
            )
            .await
    }

    pub async fn latest_output(&self, max_bytes: usize) -> RouxResult<LatestOutput> {
        self.client
            .command(
                CommandRequest::new("latest-output")
                    .session_id(self.id())
                    .args(serde_json::json!({ "maxBytes": max_bytes })),
            )
            .await
    }

    pub async fn rename(&self, name: Option<String>) -> RouxResult<()> {
        let _: Value = self
            .client
            .command(
                CommandRequest::new("session-rename")
                    .session_id(self.id())
                    .args(serde_json::json!({ "name": name })),
            )
            .await?;
        Ok(())
    }

    pub async fn archive(&self) -> RouxResult<()> {
        let _: Value = self
            .client
            .command(CommandRequest::new("session-archive").session_id(self.id()))
            .await?;
        Ok(())
    }
}

impl Pty {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn snapshot(&self, max_bytes: usize) -> RouxResult<PtySnapshot> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-output")
                    .args(serde_json::json!({ "id": self.id(), "maxBytes": max_bytes })),
            )
            .await
    }

    pub async fn write(&self, data: impl Into<String>) -> RouxResult<PtyWrite> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-write")
                    .args(serde_json::json!({ "id": self.id(), "data": data.into() })),
            )
            .await
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> RouxResult<PtyRecord> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-resize")
                    .args(serde_json::json!({ "id": self.id(), "cols": cols, "rows": rows })),
            )
            .await
    }

    pub async fn kill(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-kill").args(serde_json::json!({ "id": self.id() })),
            )
            .await
    }

    pub async fn detach(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-detach")
                    .args(serde_json::json!({ "id": self.id() })),
            )
            .await
    }

    pub async fn attach_to_pane(
        &self,
        pane_id: impl Into<String>,
    ) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-attach-pane")
                    .args(serde_json::json!({ "id": self.id(), "paneId": pane_id.into() })),
            )
            .await
    }

    pub async fn mark_read(&self) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-mark-read")
                    .args(serde_json::json!({ "id": self.id() })),
            )
            .await
    }

    pub async fn set_name(&self, name: Option<String>) -> RouxResult<Option<PtyRecord>> {
        self.client
            .command(
                CommandRequest::new("daemon-pty-set-name")
                    .args(serde_json::json!({ "id": self.id(), "name": name })),
            )
            .await
    }

    pub async fn attach<F>(&self, max_replay_bytes: usize, mut on_frame: F) -> RouxResult<()>
    where
        F: FnMut(PtyAttachFrame) -> bool + Send + 'static,
    {
        let client = self.client.clone();
        let id = self.id.clone();
        tokio::task::spawn_blocking(move || {
            let mut parse_error = None;
            let result = blocking::stream_client_request(
                &client.endpoint,
                client.auth_token.as_deref(),
                CommandRequest::new("daemon-pty-attach")
                    .args(serde_json::json!({ "id": id, "maxBytes": max_replay_bytes })),
                |line| match serde_json::from_str::<PtyAttachFrame>(line) {
                    Ok(frame) => on_frame(frame),
                    Err(err) => {
                        parse_error = Some(RouxError::Decode(err));
                        false
                    }
                },
            );
            result.and(match parse_error {
                Some(err) => Err(err),
                None => Ok(()),
            })
        })
        .await
        .map_err(|err| RouxError::Transport(format!("SDK task join failed: {err}")))?
    }
}

impl SpawnTask {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    pub fn initial_size(mut self, cols: u16, rows: u16) -> Self {
        self.initial_size = Some((cols, rows));
        self
    }

    pub async fn spawn(self) -> RouxResult<Pty> {
        let client = self.client.clone();
        let record = self.spawn_record().await?;
        Ok(client.pty(record.id))
    }

    pub async fn spawn_record(self) -> RouxResult<PtyRecord> {
        let mut args = serde_json::Map::new();
        args.insert("command".into(), Value::String(self.command));
        if let Some(id) = self.id {
            args.insert("id".into(), Value::String(id));
        }
        if let Some(working_dir) = self.working_dir {
            args.insert("workingDir".into(), Value::String(working_dir));
        }
        if let Some(session_id) = self.session_id {
            args.insert("sessionId".into(), Value::String(session_id));
        }
        if let Some(pane_id) = self.pane_id {
            args.insert("paneId".into(), Value::String(pane_id));
        }
        if let Some(profile) = self.profile {
            args.insert("profile".into(), Value::String(profile));
        }
        if let Some((cols, rows)) = self.initial_size {
            args.insert("initialSize".into(), serde_json::json!([cols, rows]));
        }
        self.client
            .command(CommandRequest::new("daemon-pty-spawn-task").args(Value::Object(args)))
            .await
    }
}

impl SpawnShell {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn pane_id(mut self, pane_id: impl Into<String>) -> Self {
        self.pane_id = Some(pane_id.into());
        self
    }

    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    pub fn nono_profile(mut self, profile: impl Into<String>) -> Self {
        self.nono_profile = Some(profile.into());
        self
    }

    pub fn nono_allow_dirs(mut self, allow_dirs: Vec<String>) -> Self {
        self.nono_allow_dirs = allow_dirs;
        self
    }

    pub fn initial_size(mut self, cols: u16, rows: u16) -> Self {
        self.initial_size = Some((cols, rows));
        self
    }

    pub async fn spawn(self) -> RouxResult<Pty> {
        let client = self.client.clone();
        let record = self.spawn_record().await?;
        Ok(client.pty(record.id))
    }

    pub async fn spawn_record(self) -> RouxResult<PtyRecord> {
        let mut args = serde_json::Map::new();
        if let Some(id) = self.id {
            args.insert("id".into(), Value::String(id));
        }
        if let Some(working_dir) = self.working_dir {
            args.insert("workingDir".into(), Value::String(working_dir));
        }
        if let Some(session_id) = self.session_id {
            args.insert("sessionId".into(), Value::String(session_id));
        }
        if let Some(pane_id) = self.pane_id {
            args.insert("paneId".into(), Value::String(pane_id));
        }
        if let Some(profile) = self.profile {
            args.insert("profile".into(), Value::String(profile));
        }
        if let Some(nono_profile) = self.nono_profile {
            args.insert("nonoProfile".into(), Value::String(nono_profile));
        }
        if !self.nono_allow_dirs.is_empty() {
            args.insert("nonoAllowDirs".into(), serde_json::json!(self.nono_allow_dirs));
        }
        if let Some((cols, rows)) = self.initial_size {
            args.insert("initialSize".into(), serde_json::json!([cols, rows]));
        }
        self.client
            .command(CommandRequest::new("daemon-pty-spawn-shell").args(Value::Object(args)))
            .await
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PtyWrite {
    pub id: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LatestOutput {
    pub session_id: Option<String>,
    pub pane_id: Option<String>,
    pub pty_id: String,
    pub max_bytes: usize,
    pub output: String,
    pub output_bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    #[cfg(not(windows))]
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn parses_socket_endpoints() {
        assert_eq!(
            parse_socket_endpoint("tcp://127.0.0.1:4444"),
            Some(SocketEndpoint::Tcp("127.0.0.1:4444".to_string()))
        );
        assert_eq!(
            parse_socket_endpoint("unix:///tmp/roux.sock"),
            Some(SocketEndpoint::Unix("/tmp/roux.sock".into()))
        );
        #[cfg(not(windows))]
        assert_eq!(
            parse_socket_endpoint("/tmp/roux.sock"),
            Some(SocketEndpoint::Unix("/tmp/roux.sock".into()))
        );
    }

    #[test]
    fn command_request_serializes_protocol_shape() {
        let request = CommandRequest::new("daemon-status")
            .session_id("session-a")
            .pane_id("pane-a")
            .auth_token("secret")
            .args(serde_json::json!({ "maxBytes": 1024 }));

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "command": "daemon-status",
                "session_id": "session-a",
                "pane_id": "pane-a",
                "auth_token": "secret",
                "args": { "maxBytes": 1024 },
            })
        );
    }

    #[test]
    fn command_response_turns_error_frame_into_error() {
        let response: CommandResponse = serde_json::from_value(serde_json::json!({
            "ok": false,
            "error": "nope",
        }))
        .unwrap();

        assert_eq!(response.into_result().unwrap_err().to_string(), "nope");
    }

    #[test]
    fn blocking_tcp_client_injects_auth_token_and_returns_raw_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(br#"{"ok":true,"data":{"kind":"roux-daemon","pid":1,"socket":"tcp://test","startedAtMs":1,"uptimeMs":2,"sessionCount":0,"projectCount":0,"watchCount":0,"processCount":0,"ptyCount":0,"capabilities":["daemon-status"]}}"#)
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let status = blocking::send_raw_request(
            &SocketEndpoint::Tcp(addr),
            Some("secret"),
            Duration::from_secs(5),
            CommandRequest::new("daemon-status"),
        )
        .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], "secret");
        assert_eq!(status["ok"], true);
        assert_eq!(status["data"]["kind"], "roux-daemon");
    }

    #[cfg(not(windows))]
    #[test]
    fn blocking_unix_socket_client_uses_sock_file_without_auth() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("roux.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"ok":true,"data":{"seen":true}}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder().endpoint(SocketEndpoint::Unix(sock)).connect().unwrap();
        let response = client.command_blocking(CommandRequest::new("daemon-status")).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], Value::Null);
        assert_eq!(response["seen"], true);
    }

    #[cfg(not(windows))]
    #[test]
    fn stream_lines_blocking_uses_unix_sock_without_auth() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("roux.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"type":"ready"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.write_all(br#"{"type":"event","id":"one"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder().endpoint(SocketEndpoint::Unix(sock)).connect().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines_for_callback = lines.clone();
        client
            .stream_lines_blocking(CommandRequest::new("watch-events"), move |line| {
                lines_for_callback.lock().unwrap().push(line.to_string());
                true
            })
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["auth_token"], Value::Null);
        assert_eq!(
            *lines.lock().unwrap(),
            vec![r#"{"type":"ready"}"#, r#"{"type":"event","id":"one"}"#]
        );
    }

    #[test]
    fn typed_watch_events_stream_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(br#"{"type":"ready"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            stream.write_all(br#"{"type":"warning","message":"heads up"}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_callback = frames.clone();
        client
            .watch_events_blocking(true, move |frame| {
                let label = match frame {
                    WatchEventStreamFrame::Ready => "ready",
                    WatchEventStreamFrame::Warning { .. } => "warning",
                    WatchEventStreamFrame::Update { .. } => "update",
                    WatchEventStreamFrame::Error { .. } => "error",
                };
                frames_for_callback.lock().unwrap().push(label);
                true
            })
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "watch-events");
        assert_eq!(request["args"]["backlog"], true);
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "warning"]);
    }

    #[test]
    fn typed_status_decodes_from_daemon_response() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"kind":"roux-daemon","pid":42,"socket":"tcp://test","startedAtMs":10,"uptimeMs":20,"sessionCount":1,"projectCount":2,"watchCount":3,"processCount":4,"ptyCount":5,"capabilities":["daemon-status","daemon-pty-list"]}}"#
                .to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let status = rt.block_on(client.status()).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "daemon-status");
        assert_eq!(status.kind, "roux-daemon");
        assert_eq!(status.pty_count, 5);
        assert!(status.capabilities.iter().any(|capability| capability == "daemon-pty-list"));
    }

    #[test]
    fn typed_project_create_uses_daemon_command_shape() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"id":"project-a","name":"Alpha"}}"#.to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let project = rt.block_on(client.create_project("Alpha")).unwrap();
        let request = handle.join().unwrap();

        assert_eq!(project.id, "project-a");
        assert_eq!(request["command"], "project-create");
        assert_eq!(request["args"]["name"], "Alpha");
    }

    #[test]
    fn typed_mailbox_post_uses_daemon_command_shape() {
        let (client, handle) = tcp_client_with_response(
            r#"{"ok":true,"data":{"id":"event-a","createdAt":1,"to":"reviewer","topic":"build.done","from":"runner","kind":"fyi","correlationId":"corr-a","projectId":"project-a","subject":"Build","body":"ready","structured":{"ok":true}}}"#
                .to_string(),
        );
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let event = rt
            .block_on(client.mailbox_post(MailboxPost {
                to: Some("reviewer".to_string()),
                topic: Some("build.done".to_string()),
                body: "ready".to_string(),
                subject: Some("Build".to_string()),
                kind: Some("fyi".to_string()),
                project_id: Some("project-a".to_string()),
                correlation_id: Some("corr-a".to_string()),
                structured: Some(serde_json::json!({ "ok": true })),
                from: Some("runner".to_string()),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(event.id, "event-a");
        assert_eq!(request["command"], "mailbox-post");
        assert_eq!(request["args"]["to"], "reviewer");
        assert_eq!(request["args"]["topic"], "build.done");
        assert_eq!(request["args"]["body"], "ready");
        assert_eq!(request["args"]["subject"], "Build");
        assert_eq!(request["args"]["kind"], "fyi");
        assert_eq!(request["args"]["project_id"], "project-a");
        assert_eq!(request["args"]["correlation_id"], "corr-a");
        assert_eq!(request["args"]["structured"]["ok"], true);
        assert_eq!(request["args"]["from"], "runner");
    }

    #[test]
    fn typed_spawn_task_builder_returns_pty_handle() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_pty_record_json("pty-1"))
                        .as_bytes(),
                )
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let pty = rt
            .block_on(
                client
                    .spawn_task("printf hello")
                    .id("pty-requested")
                    .working_dir("/tmp")
                    .session_id("session-a")
                    .pane_id("pane-a")
                    .profile("task")
                    .initial_size(100, 30)
                    .spawn(),
            )
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(pty.id(), "pty-1");
        assert_eq!(request["command"], "daemon-pty-spawn-task");
        assert_eq!(request["args"]["command"], "printf hello");
        assert_eq!(request["args"]["id"], "pty-requested");
        assert_eq!(request["args"]["workingDir"], "/tmp");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
    }

    #[test]
    fn typed_create_session_shell_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_session_json("session-a", false))
                        .as_bytes(),
                )
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let session = rt
            .block_on(client.create_session_shell(CreateSessionShell {
                id: "session-a".to_string(),
                repo_path: "/repo".to_string(),
                name: "Daemon Session".to_string(),
                worktree_path: None,
                branch: Some("feature/demo".to_string()),
                base: Some("origin/main".to_string()),
                fetch_first: true,
                profile: Some("plain-shell".to_string()),
                nono_profile: Some("strict".to_string()),
                nono_allow_dirs: vec!["/tmp".to_string()],
                initial_size: Some((100, 30)),
                project_id: Some("project-a".to_string()),
                blueprint_id: Some("blueprint-a".to_string()),
                smol_machine_name: Some("vm-a".to_string()),
                notes: Some(NotesEnv {
                    vault_root: "/vault".to_string(),
                    session_slug: "feature-demo--sessio".to_string(),
                    repo_slug: "repo-a".to_string(),
                    project_slug: Some("project-a".to_string()),
                    context_paths: vec!["/repo/docs".to_string()],
                    project_prompt: "Use project notes".to_string(),
                }),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(session.id, "session-a");
        assert_eq!(request["command"], "session-create-shell");
        assert_eq!(request["args"]["id"], "session-a");
        assert_eq!(request["args"]["repoPath"], "/repo");
        assert_eq!(request["args"]["branch"], "feature/demo");
        assert_eq!(request["args"]["base"], "origin/main");
        assert_eq!(request["args"]["fetchFirst"], true);
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["nonoProfile"], "strict");
        assert_eq!(request["args"]["nonoAllowDirs"][0], "/tmp");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([100, 30]));
        assert_eq!(request["args"]["projectId"], "project-a");
        assert_eq!(request["args"]["blueprintId"], "blueprint-a");
        assert_eq!(request["args"]["smolMachineName"], "vm-a");
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
        assert_eq!(request["args"]["notesEnv"]["contextPaths"][0], "/repo/docs");
    }

    #[test]
    fn typed_reconnect_session_shell_uses_daemon_command_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(r#"{{"ok":true,"data":{}}}"#, sample_session_json("session-a", false))
                        .as_bytes(),
                )
                .unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let session = rt
            .block_on(client.reconnect_session_shell(ReconnectSessionShell {
                id: "session-a".to_string(),
                profile: Some("plain-shell".to_string()),
                nono_profile: Some("strict".to_string()),
                nono_allow_dirs: vec!["/tmp".to_string()],
                initial_size: Some((120, 40)),
                notes: Some(NotesEnv {
                    vault_root: "/vault".to_string(),
                    session_slug: "feature-demo--sessio".to_string(),
                    repo_slug: "repo-a".to_string(),
                    project_slug: None,
                    context_paths: vec![],
                    project_prompt: "".to_string(),
                }),
            }))
            .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(session.id, "session-a");
        assert_eq!(request["command"], "session-reconnect-shell");
        assert_eq!(request["session_id"], "session-a");
        assert_eq!(request["args"]["profile"], "plain-shell");
        assert_eq!(request["args"]["nonoProfile"], "strict");
        assert_eq!(request["args"]["nonoAllowDirs"][0], "/tmp");
        assert_eq!(request["args"]["initialSize"], serde_json::json!([120, 40]));
        assert_eq!(request["args"]["notesEnv"]["vaultRoot"], "/vault");
    }

    #[test]
    fn typed_pty_attach_decodes_ndjson_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream
                .write_all(
                    format!(
                        "{{\"type\":\"ready\",\"id\":\"pty-1\",\"record\":{},\"replayOffset\":0,\"replayBytes\":[104,105]}}\n",
                        sample_pty_record_json("pty-1")
                    )
                    .as_bytes(),
                )
                .unwrap();
            stream.write_all(br#"{"type":"exit","code":0,"generation":1}"#).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });

        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        let pty = client.pty("pty-1");
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let frames = Arc::new(Mutex::new(Vec::new()));
        let frames_for_callback = frames.clone();
        rt.block_on(pty.attach(1024, move |frame| {
            let label = match frame {
                PtyAttachFrame::Ready { .. } => "ready",
                PtyAttachFrame::Output { .. } => "output",
                PtyAttachFrame::Exit { .. } => "exit",
                PtyAttachFrame::Error { .. } => "error",
            };
            frames_for_callback.lock().unwrap().push(label);
            true
        }))
        .unwrap();
        let request = handle.join().unwrap();

        assert_eq!(request["command"], "daemon-pty-attach");
        assert_eq!(request["args"]["id"], "pty-1");
        assert_eq!(*frames.lock().unwrap(), vec!["ready", "exit"]);
    }

    fn tcp_client_with_response(response: String) -> (Roux, thread::JoinHandle<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
            request
        });
        let client = Roux::builder()
            .endpoint(SocketEndpoint::Tcp(addr))
            .auth_token("secret")
            .connect()
            .unwrap();
        (client, handle)
    }

    fn sample_session_json(id: &str, archived: bool) -> String {
        serde_json::json!({
            "id": id,
            "name": "Daemon Session",
            "repoRoot": "/repo",
            "worktreePath": "/repo",
            "branch": "feature/demo",
            "isWorktree": false,
            "model": null,
            "cost": null,
            "createdAt": 1,
            "status": "idle",
            "isGitRepo": true,
            "nameOverride": null,
            "primaryPtyId": "pty-primary",
            "archived": archived,
            "endedAt": null,
            "projectId": "project-a",
            "blueprintId": "blueprint-a",
            "pinnedPrUrl": null,
            "smolMachineName": "vm-a"
        })
        .to_string()
    }

    fn sample_pty_record_json(id: &str) -> String {
        serde_json::json!({
            "id": id,
            "kind": "task",
            "command": "printf hello",
            "workingDir": "/tmp",
            "startedAtMs": 1,
            "running": false,
            "exitCode": 0,
            "generation": 1,
            "retainedOutputBytes": 5,
            "outputTruncated": false,
            "cols": 80,
            "rows": 24,
            "info": {
                "id": id,
                "session_id": "session-a",
                "role": "secondary",
                "status": { "type": "Exited", "code": 0, "at_ms": 2 },
                "name": null,
                "working_dir": "/tmp",
                "profile": "task",
                "unread_output": false,
                "bell_pending": false,
            }
        })
        .to_string()
    }
}
