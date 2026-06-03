use crate::blocking;
use crate::client::Roux;
use crate::error::{RouxError, RouxResult};
use crate::protocol::CommandRequest;
use crate::types::{PtyAttachFrame, PtyRecord, PtySnapshot};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Session {
    pub(crate) client: Roux,
    pub(crate) session: roux_core::Session,
}

#[derive(Debug, Clone)]
pub struct Pty {
    pub(crate) client: Roux,
    pub(crate) id: String,
}

#[derive(Debug, Clone)]
pub struct SpawnTask {
    pub(crate) client: Roux,
    pub(crate) command: String,
    pub(crate) id: Option<String>,
    pub(crate) working_dir: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) pane_id: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) profile_data: Option<roux_core::SpawnProfile>,
    pub(crate) env_overrides: Option<BTreeMap<String, roux_core::TerminalEnvRule>>,
    pub(crate) initial_size: Option<(u16, u16)>,
}

#[derive(Debug, Clone)]
pub struct SpawnShell {
    pub(crate) client: Roux,
    pub(crate) id: Option<String>,
    pub(crate) working_dir: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) pane_id: Option<String>,
    pub(crate) profile: Option<String>,
    pub(crate) profile_data: Option<roux_core::SpawnProfile>,
    pub(crate) env_overrides: Option<BTreeMap<String, roux_core::TerminalEnvRule>>,
    pub(crate) initial_size: Option<(u16, u16)>,
}

impl Roux {
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
            profile_data: None,
            env_overrides: None,
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
            profile_data: None,
            env_overrides: None,
            initial_size: None,
        }
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
                    .args(serde_json::json!({ "name": name.unwrap_or_default() })),
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
                        if parse_error.is_none() {
                            parse_error = Some(RouxError::Decode(err));
                        }
                        true
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

    pub fn profile_data(mut self, profile: roux_core::SpawnProfile) -> Self {
        self.profile_data = Some(profile);
        self
    }

    pub fn env_overrides(
        mut self,
        env_overrides: BTreeMap<String, roux_core::TerminalEnvRule>,
    ) -> Self {
        self.env_overrides = Some(env_overrides);
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
        if let Some(profile_data) =
            self.profile_data.and_then(|profile| serde_json::to_value(profile).ok())
        {
            args.insert("profileData".into(), profile_data);
        }
        if let Some(env_overrides) =
            self.env_overrides.and_then(|env| serde_json::to_value(env).ok())
        {
            args.insert("envOverrides".into(), env_overrides);
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

    pub fn profile_data(mut self, profile: roux_core::SpawnProfile) -> Self {
        self.profile_data = Some(profile);
        self
    }

    pub fn env_overrides(
        mut self,
        env_overrides: BTreeMap<String, roux_core::TerminalEnvRule>,
    ) -> Self {
        self.env_overrides = Some(env_overrides);
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
        if let Some(profile_data) =
            self.profile_data.and_then(|profile| serde_json::to_value(profile).ok())
        {
            args.insert("profileData".into(), profile_data);
        }
        if let Some(env_overrides) =
            self.env_overrides.and_then(|env| serde_json::to_value(env).ok())
        {
            args.insert("envOverrides".into(), env_overrides);
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
