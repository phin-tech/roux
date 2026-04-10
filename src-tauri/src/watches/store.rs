use std::path::PathBuf;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};

use roux_core::{RuntimeState, Watch, WatchScope};

enum WatchMsg {
    Add { watch: Watch, reply: oneshot::Sender<()> },
    Remove { id: String, reply: oneshot::Sender<()> },
    Get { id: String, reply: oneshot::Sender<Option<Watch>> },
    List { reply: oneshot::Sender<Vec<Watch>> },
    Update { id: String, f: Box<dyn FnOnce(&mut Watch) + Send>, reply: oneshot::Sender<()> },
    CleanupOrphans { session_ids: Vec<String>, project_ids: Vec<String>, reply: oneshot::Sender<()> },
    Shutdown { reply: oneshot::Sender<()> },
}

#[derive(Debug, thiserror::Error)]
#[error("watch store service unavailable")]
pub struct ServiceError;

#[derive(Clone)]
pub struct WatchStoreHandle {
    tx: mpsc::UnboundedSender<WatchMsg>,
}

impl WatchStoreHandle {
    fn send(&self, msg: WatchMsg) -> Result<(), ServiceError> {
        self.tx.send(msg).map_err(|_| ServiceError)
    }

    pub async fn add(&self, watch: Watch) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Add { watch, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Remove { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Watch>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Get { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn list(&self) -> Result<Vec<Watch>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::List { reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn update(&self, id: &str, f: impl FnOnce(&mut Watch) + Send + 'static) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Update { id: id.to_string(), f: Box::new(f), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn cleanup_orphans(&self, session_ids: Vec<String>, project_ids: Vec<String>) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::CleanupOrphans { session_ids, project_ids, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.tx.send(WatchMsg::Shutdown { reply: reply_tx });
        let _ = reply_rx.await;
    }
}

pub fn spawn(initial_watches: Vec<Watch>) -> (WatchStoreHandle, JoinHandle<()>) {
    spawn_with_path(initial_watches, persistence_path())
}

pub fn spawn_with_path(
    initial_watches: Vec<Watch>,
    persist_path: PathBuf,
) -> (WatchStoreHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let join = tauri::async_runtime::spawn(service_loop(rx, initial_watches, persist_path));
    (WatchStoreHandle { tx }, join)
}

async fn service_loop(
    mut rx: mpsc::UnboundedReceiver<WatchMsg>,
    mut watches: Vec<Watch>,
    persist_path: PathBuf,
) {
    let mut dirty = false;
    let mut tick = interval(Duration::from_millis(500));
    tick.tick().await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(WatchMsg::Add { watch, reply }) => {
                        watches.push(watch);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::Remove { id, reply }) => {
                        watches.retain(|w| w.id != id);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::Get { id, reply }) => {
                        let found = watches.iter().find(|w| w.id == id).cloned();
                        let _ = reply.send(found);
                    }
                    Some(WatchMsg::List { reply }) => {
                        let _ = reply.send(watches.clone());
                    }
                    Some(WatchMsg::Update { id, f, reply }) => {
                        if let Some(w) = watches.iter_mut().find(|w| w.id == id) {
                            f(w);
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::CleanupOrphans { session_ids, project_ids, reply }) => {
                        let before = watches.len();
                        watches.retain(|w| match &w.scope {
                            WatchScope::Global => true,
                            WatchScope::Session { session_id } => session_ids.contains(session_id),
                            WatchScope::Project { project_id } => project_ids.contains(project_id),
                        });
                        if watches.len() != before {
                            dirty = true;
                        }
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::Shutdown { reply }) => {
                        if dirty {
                            write_to_path(&watches, &persist_path);
                        }
                        let _ = reply.send(());
                        break;
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                if dirty {
                    let snapshot = watches.clone();
                    let path = persist_path.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        write_to_path(&snapshot, &path);
                    }).await;
                    dirty = false;
                }
            }
        }
    }
}

fn persistence_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("roux").join("watches.json")
}

fn write_to_path(watches: &[Watch], path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(watches) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_persisted() -> Vec<Watch> {
    let path = persistence_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let mut watches: Vec<Watch> = serde_json::from_str(&content).unwrap_or_default();
        for w in &mut watches {
            match &w.runtime_state {
                RuntimeState::Stopped | RuntimeState::Paused => {}
                _ => w.runtime_state = RuntimeState::Pending,
            }
        }
        watches
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{NotifyConfig, WatchKind, WatchMode};

    fn make_watch(id: &str) -> Watch {
        Watch {
            id: id.to_string(),
            name: format!("Watch {}", id),
            kind: WatchKind::HttpHealth { url: "http://localhost".to_string(), expected_status: 200 },
            mode: WatchMode::Recurring { interval_secs: 30 },
            scope: WatchScope::Global,
            runtime_state: RuntimeState::Pending,
            last_result: None,
            last_checked: None,
            notify: NotifyConfig::default(),
            created_at: 0,
        }
    }

    fn temp_persist_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("watches.json");
        (dir, path)
    }

    #[tokio::test]
    async fn add_and_list() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], path);
        handle.add(make_watch("w1")).await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "w1");
    }

    #[tokio::test]
    async fn remove() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("w1"), make_watch("w2")], path);
        handle.remove("w1").await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "w2");
    }

    #[tokio::test]
    async fn update() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("w1")], path);
        handle.update("w1", |w| w.runtime_state = RuntimeState::Active).await.unwrap();
        let watch = handle.get("w1").await.unwrap().unwrap();
        assert!(matches!(watch.runtime_state, RuntimeState::Active));
    }

    #[tokio::test]
    async fn shutdown_persists() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path.clone());
        handle.add(make_watch("w1")).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let persisted: Vec<Watch> = serde_json::from_str(&content).unwrap();
        assert_eq!(persisted.len(), 1);
    }

    #[tokio::test]
    async fn operations_after_shutdown_return_error() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path);
        handle.shutdown().await;
        join.await.unwrap();
        assert!(handle.list().await.is_err());
    }
}
