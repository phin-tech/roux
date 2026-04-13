use std::path::PathBuf;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};

use roux_core::Project;

enum ProjectMsg {
    Add { project: Project, reply: oneshot::Sender<()> },
    Remove { id: String, reply: oneshot::Sender<()> },
    Rename { id: String, name: String, reply: oneshot::Sender<()> },
    List { reply: oneshot::Sender<Vec<Project>> },
    Shutdown { reply: oneshot::Sender<()> },
}

#[derive(Debug, thiserror::Error)]
#[error("project service unavailable")]
pub struct ServiceError;

#[derive(Clone)]
pub struct ProjectHandle {
    tx: mpsc::UnboundedSender<ProjectMsg>,
}

impl ProjectHandle {
    fn send(&self, msg: ProjectMsg) -> Result<(), ServiceError> {
        self.tx.send(msg).map_err(|_| ServiceError)
    }

    pub async fn add(&self, project: Project) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProjectMsg::Add { project, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProjectMsg::Remove { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn rename(&self, id: &str, name: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProjectMsg::Rename {
            id: id.to_string(),
            name: name.to_string(),
            reply: reply_tx,
        })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn list(&self) -> Result<Vec<Project>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(ProjectMsg::List { reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn shutdown(&self) {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self.tx.send(ProjectMsg::Shutdown { reply: reply_tx });
        let _ = reply_rx.await;
    }
}

pub fn spawn(initial_projects: Vec<Project>) -> (ProjectHandle, JoinHandle<()>) {
    spawn_with_path(initial_projects, persistence_path())
}

pub fn spawn_with_path(
    initial_projects: Vec<Project>,
    persist_path: PathBuf,
) -> (ProjectHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let join = tauri::async_runtime::spawn(service_loop(rx, initial_projects, persist_path));
    (ProjectHandle { tx }, join)
}

async fn service_loop(
    mut rx: mpsc::UnboundedReceiver<ProjectMsg>,
    mut projects: Vec<Project>,
    persist_path: PathBuf,
) {
    let mut dirty = false;
    let mut tick = interval(Duration::from_millis(500));
    tick.tick().await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(ProjectMsg::Add { project, reply }) => {
                        projects.push(project);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(ProjectMsg::Remove { id, reply }) => {
                        projects.retain(|p| p.id != id);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(ProjectMsg::Rename { id, name, reply }) => {
                        if let Some(p) = projects.iter_mut().find(|p| p.id == id) {
                            p.name = name;
                        }
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(ProjectMsg::List { reply }) => {
                        let _ = reply.send(projects.clone());
                    }
                    Some(ProjectMsg::Shutdown { reply }) => {
                        if dirty {
                            write_to_path(&projects, &persist_path);
                        }
                        let _ = reply.send(());
                        break;
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                if dirty {
                    let snapshot = projects.clone();
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
    crate::paths::roux_config_dir().join("projects.json")
}

fn write_to_path(projects: &[Project], path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(projects) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_persisted() -> Vec<Project> {
    let path = persistence_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(id: &str) -> Project {
        Project { id: id.to_string(), name: format!("Project {}", id) }
    }

    fn temp_persist_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        (dir, path)
    }

    #[tokio::test]
    async fn add_and_list() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], path);
        handle.add(make_project("p1")).await.unwrap();
        handle.add(make_project("p2")).await.unwrap();
        let projects = handle.list().await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, "p1");
    }

    #[tokio::test]
    async fn remove() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_project("p1"), make_project("p2")], path);
        handle.remove("p1").await.unwrap();
        let projects = handle.list().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "p2");
    }

    #[tokio::test]
    async fn rename() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_project("p1")], path);
        handle.rename("p1", "New Name").await.unwrap();
        let projects = handle.list().await.unwrap();
        assert_eq!(projects[0].name, "New Name");
    }

    #[tokio::test]
    async fn shutdown_persists() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path.clone());
        handle.add(make_project("p1")).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let persisted: Vec<Project> = serde_json::from_str(&content).unwrap();
        assert_eq!(persisted.len(), 1);
    }

    #[tokio::test]
    async fn operations_after_shutdown_return_error() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], path);
        handle.shutdown().await;
        join.await.unwrap();
        assert!(handle.add(make_project("p1")).await.is_err());
        assert!(handle.list().await.is_err());
    }
}
