//! # Session Service — Handle + Channel Pattern
//!
//! This module implements the session service using a handle + channel model:
//!
//! - **`SessionHandle`**: A cheap, cloneable handle that wraps an `mpsc::UnboundedSender`.
//!   Callers send messages through the handle and await responses via `oneshot` channels.
//!
//! - **`SessionMsg`**: An enum of all operations the service supports. Every variant carries
//!   a `oneshot::Sender` for the reply, preserving synchronous ordering guarantees.
//!
//! - **Service loop**: A single `tokio::spawn` task that owns all state (`Vec<Session>`).
//!   No Mutex — thread safety comes from channel serialization. A 500ms timer triggers
//!   periodic persistence via `spawn_blocking`.
//!
//! ## Replicating this pattern for other subsystems
//!
//! 1. Define a message enum with a variant per operation, each carrying a `oneshot::Sender<T>`
//! 2. Create a `Handle` struct wrapping `mpsc::UnboundedSender<Msg>` and implement async methods
//! 3. Write a `spawn()` function that creates the channel, spawns the service loop, returns the handle
//! 4. The service loop owns all state and uses `tokio::select!` over the receiver + any timers
//! 5. Include a `Shutdown` variant for graceful cleanup

use std::path::PathBuf;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};

use crate::session::Session;

enum SessionMsg {
    // `Session` is ~272 bytes; box it so this variant doesn't dominate the
    // enum size (clippy::large_enum_variant). All other variants are tiny.
    Add { session: Box<Session>, reply: oneshot::Sender<()> },
    Remove { id: String, reply: oneshot::Sender<()> },
    Archive { id: String, reply: oneshot::Sender<()> },
    Restore { id: String, reply: oneshot::Sender<()> },
    Get { id: String, reply: oneshot::Sender<Option<Session>> },
    List { reply: oneshot::Sender<Vec<Session>> },
    UpdateStatus { id: String, status: roux_core::SessionStatus, reply: oneshot::Sender<()> },
    SetGitRepo { id: String, is_git_repo: bool, reply: oneshot::Sender<()> },
    SetProject { id: String, project_id: Option<String>, reply: oneshot::Sender<()> },
    SetNameOverride { id: String, name_override: Option<String>, reply: oneshot::Sender<()> },
    Shutdown { reply: oneshot::Sender<()> },
}

/// Error returned when the session service is unavailable (task exited or channel closed).
#[derive(Debug, thiserror::Error)]
#[error("session service unavailable")]
pub struct ServiceError;

#[derive(Clone)]
pub struct SessionHandle {
    tx: mpsc::UnboundedSender<SessionMsg>,
}

impl SessionHandle {
    fn send(&self, msg: SessionMsg) -> Result<(), ServiceError> {
        self.tx.send(msg).map_err(|_| ServiceError)
    }

    pub async fn add(&self, session: Session) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::Add { session: Box::new(session), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::Remove { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    /// Soft-delete: flip `archived = true`, stamp `ended_at`, clear `primary_pty_id`.
    /// Keeps the record in `Vec<Session>` so it persists to disk for the
    /// history view. Callers should kill the session's PTYs separately.
    pub async fn archive(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::Archive { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    /// Inverse of `archive`: clear `archived` and `ended_at`. The session
    /// reappears in `list()` with whatever `status` it held (typically
    /// `Disconnected` since the PTY is dead); the reconnect flow attaches
    /// a fresh PTY when the user opens it.
    pub async fn restore(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::Restore { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Session>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::Get { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn list(&self) -> Result<Vec<Session>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::List { reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn update_status(
        &self,
        id: &str,
        status: roux_core::SessionStatus,
    ) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::UpdateStatus { id: id.to_string(), status, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn set_git_repo(&self, id: &str, is_git_repo: bool) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::SetGitRepo { id: id.to_string(), is_git_repo, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn set_project(
        &self,
        id: &str,
        project_id: Option<String>,
    ) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::SetProject { id: id.to_string(), project_id, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn set_name_override(
        &self,
        id: &str,
        name_override: Option<String>,
    ) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(SessionMsg::SetNameOverride {
            id: id.to_string(),
            name_override,
            reply: reply_tx,
        })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        // Shutdown is best-effort — if the service is already gone, that's fine.
        let _ = self.tx.send(SessionMsg::Shutdown { reply: reply_tx });
        let _ = reply_rx.await;
    }
}

/// Spawn the session service. Returns a handle for sending messages and a JoinHandle for the task.
pub fn spawn(initial_sessions: Vec<Session>) -> (SessionHandle, JoinHandle<()>) {
    spawn_with_path(initial_sessions, crate::session::persistence_path())
}

/// Spawn with an explicit persistence path (for testing).
pub fn spawn_with_path(
    initial_sessions: Vec<Session>,
    persist_path: PathBuf,
) -> (SessionHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let join = tauri::async_runtime::spawn(service_loop(rx, initial_sessions, persist_path));
    (SessionHandle { tx }, join)
}

async fn service_loop(
    mut rx: mpsc::UnboundedReceiver<SessionMsg>,
    mut sessions: Vec<Session>,
    persist_path: PathBuf,
) {
    let mut dirty = false;
    let mut tick = interval(Duration::from_millis(500));
    // The first tick fires immediately — consume it so we don't persist on startup.
    tick.tick().await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(SessionMsg::Add { session, reply }) => {
                        sessions.push(*session);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::Remove { id, reply }) => {
                        sessions.retain(|s| s.id != id);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::Archive { id, reply }) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.archived = true;
                            s.ended_at = Some(now);
                            s.primary_pty_id = None;
                            dirty = true;
                        }
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::Restore { id, reply }) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.archived = false;
                            s.ended_at = None;
                            dirty = true;
                        }
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::Get { id, reply }) => {
                        let found = sessions.iter().find(|s| s.id == id).cloned();
                        let _ = reply.send(found);
                    }
                    Some(SessionMsg::List { reply }) => {
                        let _ = reply.send(sessions.clone());
                    }
                    Some(SessionMsg::UpdateStatus { id, status, reply }) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.status = status;
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::SetGitRepo { id, is_git_repo, reply }) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.is_git_repo = is_git_repo;
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::SetProject { id, project_id, reply }) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.project_id = project_id;
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::SetNameOverride { id, name_override, reply }) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                            s.name_override = name_override;
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(SessionMsg::Shutdown { reply }) => {
                        if dirty {
                            persist_to_disk(&sessions, &persist_path);
                        }
                        let _ = reply.send(());
                        break;
                    }
                    None => break, // all senders dropped
                }
            }
            _ = tick.tick() => {
                if dirty {
                    let snapshot = sessions.clone();
                    let path = persist_path.clone();
                    // Await the write so it completes before processing more messages
                    // or handling shutdown — prevents stale overwrites.
                    let _ = tokio::task::spawn_blocking(move || {
                        write_to_path(&snapshot, &path);
                    }).await;
                    dirty = false;
                }
            }
        }
    }
}

/// Synchronous disk write — used from `spawn_blocking` and from shutdown (where blocking is fine).
fn persist_to_disk(sessions: &[Session], path: &std::path::Path) {
    write_to_path(sessions, path);
}

fn write_to_path(sessions: &[Session], path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(sessions) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            name: format!("Session {}", id),
            repo_root: "/tmp/repo".to_string(),
            worktree_path: "/tmp/repo".to_string(),
            branch: "main".to_string(),
            is_worktree: false,
            status: roux_core::SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: None,
            is_git_repo: false,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: None,
        }
    }

    fn temp_persist_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        (dir, path)
    }

    #[tokio::test]
    async fn add_and_list() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], path);

        handle.add(make_session("s1")).await.unwrap();
        handle.add(make_session("s2")).await.unwrap();

        let sessions = handle.list().await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[1].id, "s2");
    }

    #[tokio::test]
    async fn remove() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1"), make_session("s2")], path);

        handle.remove("s1").await.unwrap();

        let sessions = handle.list().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s2");
    }

    #[tokio::test]
    async fn archive_keeps_record_and_flips_flag() {
        let (_dir, path) = temp_persist_path();
        let mut seed = make_session("s1");
        seed.primary_pty_id = Some("pty-1".to_string());
        let (handle, _join) = spawn_with_path(vec![seed], path);

        handle.archive("s1").await.unwrap();

        let sessions = handle.list().await.unwrap();
        assert_eq!(sessions.len(), 1, "archived sessions stay in the vec");
        assert!(sessions[0].archived);
        assert!(sessions[0].ended_at.is_some());
        assert!(
            sessions[0].primary_pty_id.is_none(),
            "archive clears primary_pty_id (PTY is dead)",
        );
    }

    #[tokio::test]
    async fn restore_clears_archive_state() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        handle.archive("s1").await.unwrap();
        handle.restore("s1").await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert!(!session.archived);
        assert!(session.ended_at.is_none());
    }

    #[tokio::test]
    async fn archive_missing_id_is_noop() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        handle.archive("does-not-exist").await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert!(!session.archived);
    }

    #[tokio::test]
    async fn get_existing() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        let session = handle.get("s1").await.unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().id, "s1");
    }

    #[tokio::test]
    async fn get_missing() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], path);

        let session = handle.get("nonexistent").await.unwrap();
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn update_status() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        handle.update_status("s1", roux_core::SessionStatus::Generating).await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert_eq!(session.status, roux_core::SessionStatus::Generating);
    }

    #[tokio::test]
    async fn set_git_repo() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        handle.set_git_repo("s1", true).await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert!(session.is_git_repo);
    }

    #[tokio::test]
    async fn set_project() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_session("s1")], path);

        handle.set_project("s1", Some("proj-1".to_string())).await.unwrap();

        let session = handle.get("s1").await.unwrap().unwrap();
        assert_eq!(session.project_id.as_deref(), Some("proj-1"));
    }

    #[tokio::test]
    async fn shutdown_persists_dirty_state() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path.clone());

        handle.add(make_session("s1")).await.unwrap();
        handle.add(make_session("s2")).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();

        // Read back from disk
        let content = std::fs::read_to_string(&path).unwrap();
        let persisted: Vec<Session> = serde_json::from_str(&content).unwrap();
        assert_eq!(persisted.len(), 2);
        assert_eq!(persisted[0].id, "s1");
        assert_eq!(persisted[1].id, "s2");
    }

    #[tokio::test]
    async fn shutdown_after_mutation_persists() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![make_session("s1")], path.clone());

        handle.update_status("s1", roux_core::SessionStatus::Generating).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let persisted: Vec<Session> = serde_json::from_str(&content).unwrap();
        assert_eq!(persisted[0].status, roux_core::SessionStatus::Generating);
    }

    #[tokio::test]
    async fn shutdown_without_dirty_does_not_write() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path.clone());

        handle.shutdown().await;
        join.await.unwrap();

        // No file should be created since nothing was dirty
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn operations_after_shutdown_return_error() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path);

        handle.shutdown().await;
        join.await.unwrap();

        // All operations should now return ServiceError
        assert!(handle.add(make_session("s1")).await.is_err());
        assert!(handle.list().await.is_err());
        assert!(handle.get("s1").await.is_err());
        assert!(handle.remove("s1").await.is_err());
        assert!(handle.update_status("s1", roux_core::SessionStatus::Idle).await.is_err());
        assert!(handle.set_git_repo("s1", true).await.is_err());
        assert!(handle.set_project("s1", None).await.is_err());
    }
}
