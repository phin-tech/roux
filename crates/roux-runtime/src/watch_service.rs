use std::path::{Path, PathBuf};

use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};

use roux_core::{RuntimeState, Watch, WatchKind, WatchScope};

enum WatchMsg {
    Add {
        watch: Box<Watch>,
        reply: oneshot::Sender<()>,
    },
    /// Find an existing `GithubPr` watch matching `(scope, repo, pr_number)`,
    /// or insert the supplied watch atomically.
    FindOrAddGithubPr {
        watch: Box<Watch>,
        reply: oneshot::Sender<(Watch, bool)>,
    },
    Replace {
        watch: Box<Watch>,
        reply: oneshot::Sender<()>,
    },
    ReplaceAll {
        watches: Vec<Watch>,
        reply: oneshot::Sender<()>,
    },
    Remove {
        id: String,
        reply: oneshot::Sender<()>,
    },
    RemoveForSession {
        session_id: String,
        reply: oneshot::Sender<usize>,
    },
    Get {
        id: String,
        reply: oneshot::Sender<Option<Watch>>,
    },
    List {
        reply: oneshot::Sender<Vec<Watch>>,
    },
    Update {
        id: String,
        f: Box<dyn FnOnce(&mut Watch) + Send>,
        reply: oneshot::Sender<()>,
    },
    CleanupOrphans {
        session_ids: Vec<String>,
        project_ids: Vec<String>,
        reply: oneshot::Sender<usize>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
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
        self.send(WatchMsg::Add { watch: Box::new(watch), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    /// Atomically find-or-insert a `GithubPr` watch keyed on
    /// `(scope, repo, pr_number)`. Returns `(watch, was_new)`.
    pub async fn find_or_add_github_pr(&self, watch: Watch) -> Result<(Watch, bool), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::FindOrAddGithubPr { watch: Box::new(watch), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn replace(&self, watch: Watch) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Replace { watch: Box::new(watch), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn replace_all(&self, watches: Vec<Watch>) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::ReplaceAll { watches, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Remove { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove_for_session(&self, session_id: &str) -> Result<usize, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::RemoveForSession {
            session_id: session_id.to_string(),
            reply: reply_tx,
        })?;
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

    pub async fn update(
        &self,
        id: &str,
        f: impl FnOnce(&mut Watch) + Send + 'static,
    ) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(WatchMsg::Update { id: id.to_string(), f: Box::new(f), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn cleanup_orphans(
        &self,
        session_ids: Vec<String>,
        project_ids: Vec<String>,
    ) -> Result<usize, ServiceError> {
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

pub fn service_with_path(
    initial_watches: Vec<Watch>,
    persist_path: Option<PathBuf>,
) -> (WatchStoreHandle, impl std::future::Future<Output = ()> + Send + 'static) {
    let (tx, rx) = mpsc::unbounded_channel();
    (WatchStoreHandle { tx }, service_loop(rx, initial_watches, persist_path))
}

pub fn spawn_with_path(
    initial_watches: Vec<Watch>,
    persist_path: impl Into<Option<PathBuf>>,
) -> (WatchStoreHandle, tokio::task::JoinHandle<()>) {
    let (handle, future) = service_with_path(initial_watches, persist_path.into());
    let join = tokio::spawn(future);
    (handle, join)
}

async fn service_loop(
    mut rx: mpsc::UnboundedReceiver<WatchMsg>,
    mut watches: Vec<Watch>,
    persist_path: Option<PathBuf>,
) {
    let mut dirty = false;
    let mut tick = interval(Duration::from_millis(500));
    tick.tick().await;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(WatchMsg::Add { watch, reply }) => {
                        watches.push(*watch);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::FindOrAddGithubPr { watch, reply }) => {
                        let existing = if let WatchKind::GithubPr { repo, pr_number } = &watch.kind {
                            watches.iter().find(|w| {
                                scopes_equal(&w.scope, &watch.scope)
                                    && matches!(
                                        &w.kind,
                                        WatchKind::GithubPr { repo: r, pr_number: n }
                                            if r == repo && n == pr_number
                                    )
                            }).cloned()
                        } else {
                            None
                        };
                        match existing {
                            Some(w) => {
                                let _ = reply.send((w, false));
                            }
                            None => {
                                let new_watch = *watch;
                                watches.push(new_watch.clone());
                                dirty = true;
                                let _ = reply.send((new_watch, true));
                            }
                        }
                    }
                    Some(WatchMsg::Replace { watch, reply }) => {
                        replace_watch(&mut watches, *watch);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::ReplaceAll { watches: next, reply }) => {
                        watches = next;
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::Remove { id, reply }) => {
                        watches.retain(|w| w.id != id);
                        dirty = true;
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::RemoveForSession { session_id, reply }) => {
                        let before = watches.len();
                        watches.retain(|w| {
                            !matches!(&w.scope, WatchScope::Session { session_id: scoped } if scoped == &session_id)
                        });
                        let removed = before.saturating_sub(watches.len());
                        if removed > 0 {
                            dirty = true;
                        }
                        let _ = reply.send(removed);
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
                            dirty = true;
                        }
                        let _ = reply.send(());
                    }
                    Some(WatchMsg::CleanupOrphans { session_ids, project_ids, reply }) => {
                        let before = watches.len();
                        watches.retain(|w| match &w.scope {
                            WatchScope::Global => true,
                            WatchScope::Session { session_id } => session_ids.contains(session_id),
                            WatchScope::Project { project_id } => project_ids.contains(project_id),
                        });
                        let removed = before.saturating_sub(watches.len());
                        if removed > 0 {
                            dirty = true;
                        }
                        let _ = reply.send(removed);
                    }
                    Some(WatchMsg::Shutdown { reply }) => {
                        if dirty {
                            if let Some(path) = persist_path.as_deref() {
                                write_to_path(&watches, path);
                            }
                        }
                        let _ = reply.send(());
                        break;
                    }
                    None => break,
                }
            }
            _ = tick.tick() => {
                if dirty {
                    if let Some(path) = persist_path.clone() {
                        let snapshot = watches.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            write_to_path(&snapshot, &path);
                        }).await;
                    }
                    dirty = false;
                }
            }
        }
    }
}

fn replace_watch(watches: &mut Vec<Watch>, watch: Watch) {
    if let Some(existing) = watches.iter_mut().find(|candidate| candidate.id == watch.id) {
        *existing = watch;
    } else {
        watches.push(watch);
    }
}

fn write_to_path(watches: &[Watch], path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(watches) {
        let _ = std::fs::write(path, json);
    }
}

fn scopes_equal(a: &WatchScope, b: &WatchScope) -> bool {
    match (a, b) {
        (WatchScope::Global, WatchScope::Global) => true,
        (WatchScope::Session { session_id: x }, WatchScope::Session { session_id: y }) => x == y,
        (WatchScope::Project { project_id: x }, WatchScope::Project { project_id: y }) => x == y,
        _ => false,
    }
}

pub fn load_persisted_from(path: impl AsRef<Path>) -> Vec<Watch> {
    let path = path.as_ref();
    if path.exists() {
        let content = std::fs::read_to_string(path).unwrap_or_default();
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
    use roux_core::{NotifyConfig, WatchMode};

    fn make_watch(id: &str) -> Watch {
        Watch {
            id: id.to_string(),
            name: format!("Watch {}", id),
            kind: WatchKind::HttpHealth {
                url: "http://localhost".to_string(),
                expected_status: 200,
            },
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
        let (handle, _join) = spawn_with_path(vec![], Some(path));
        handle.add(make_watch("w1")).await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "w1");
    }

    #[tokio::test]
    async fn remove() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("w1"), make_watch("w2")], Some(path));
        handle.remove("w1").await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "w2");
    }

    #[tokio::test]
    async fn update() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("w1")], Some(path));
        handle.update("w1", |w| w.runtime_state = RuntimeState::Active).await.unwrap();
        let watch = handle.get("w1").await.unwrap().unwrap();
        assert!(matches!(watch.runtime_state, RuntimeState::Active));
    }

    #[tokio::test]
    async fn replace_upserts_by_id() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("w1")], Some(path));
        let mut replacement = make_watch("w1");
        replacement.name = "Replaced".to_string();
        handle.replace(replacement).await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].name, "Replaced");
    }

    #[tokio::test]
    async fn replace_all_swaps_store_contents() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![make_watch("old")], Some(path));
        handle.replace_all(vec![make_watch("new")]).await.unwrap();
        let watches = handle.list().await.unwrap();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].id, "new");
    }

    #[tokio::test]
    async fn shutdown_persists() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], Some(path.clone()));
        handle.add(make_watch("w1")).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let persisted: Vec<Watch> = serde_json::from_str(&content).unwrap();
        assert_eq!(persisted.len(), 1);
    }

    #[tokio::test]
    async fn in_memory_store_does_not_persist() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], None);
        handle.add(make_watch("w1")).await.unwrap();
        handle.shutdown().await;
        join.await.unwrap();
        assert!(!path.exists());
    }

    fn make_pr_watch(id: &str, repo: &str, pr_number: u64, scope: WatchScope) -> Watch {
        Watch {
            id: id.to_string(),
            name: format!("PR: {} #{}", repo, pr_number),
            kind: WatchKind::GithubPr { repo: repo.to_string(), pr_number },
            mode: WatchMode::Recurring { interval_secs: 30 },
            scope,
            runtime_state: RuntimeState::Pending,
            last_result: None,
            last_checked: None,
            notify: NotifyConfig::default(),
            created_at: 0,
        }
    }

    #[tokio::test]
    async fn find_or_add_inserts_new_pr_watch_and_reports_was_new() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], Some(path));
        let scope = WatchScope::Session { session_id: "s1".into() };
        let w = make_pr_watch("w1", "phin-tech/roux", 42, scope);
        let (resolved, was_new) = handle.find_or_add_github_pr(w.clone()).await.unwrap();
        assert!(was_new);
        assert_eq!(resolved.id, "w1");
        assert_eq!(handle.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn find_or_add_returns_existing_match_for_same_scope_repo_and_pr() {
        let (_dir, path) = temp_persist_path();
        let scope = WatchScope::Session { session_id: "s1".into() };
        let existing = make_pr_watch("first", "phin-tech/roux", 42, scope.clone());
        let (handle, _join) = spawn_with_path(vec![existing], Some(path));

        let attempt = make_pr_watch("would-be-duplicate", "phin-tech/roux", 42, scope);
        let (resolved, was_new) = handle.find_or_add_github_pr(attempt).await.unwrap();
        assert!(!was_new);
        assert_eq!(resolved.id, "first");
        assert_eq!(handle.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn find_or_add_treats_different_scopes_as_distinct() {
        let (_dir, path) = temp_persist_path();
        let s1 = WatchScope::Session { session_id: "s1".into() };
        let s2 = WatchScope::Session { session_id: "s2".into() };
        let existing = make_pr_watch("first", "phin-tech/roux", 42, s1);
        let (handle, _join) = spawn_with_path(vec![existing], Some(path));

        let other = make_pr_watch("second", "phin-tech/roux", 42, s2);
        let (resolved, was_new) = handle.find_or_add_github_pr(other).await.unwrap();
        assert!(was_new);
        assert_eq!(resolved.id, "second");
        assert_eq!(handle.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn find_or_add_concurrent_calls_only_insert_once() {
        let (_dir, path) = temp_persist_path();
        let (handle, _join) = spawn_with_path(vec![], Some(path));
        let scope = WatchScope::Session { session_id: "s1".into() };

        let a = make_pr_watch("a", "phin-tech/roux", 42, scope.clone());
        let b = make_pr_watch("b", "phin-tech/roux", 42, scope);
        let h1 = handle.clone();
        let h2 = handle.clone();
        let (r1, r2) = tokio::join!(h1.find_or_add_github_pr(a), h2.find_or_add_github_pr(b),);
        let (_, was_new_1) = r1.unwrap();
        let (_, was_new_2) = r2.unwrap();
        assert_eq!(was_new_1 as u8 + was_new_2 as u8, 1);
        assert_eq!(handle.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_for_session_removes_only_session_scoped_watches() {
        let (_dir, path) = temp_persist_path();
        let mut scoped = make_watch("session");
        scoped.scope = WatchScope::Session { session_id: "s1".into() };
        let mut other_session = make_watch("other-session");
        other_session.scope = WatchScope::Session { session_id: "s2".into() };
        let global = make_watch("global");
        let (handle, _join) = spawn_with_path(vec![scoped, other_session, global], Some(path));

        let removed = handle.remove_for_session("s1").await.unwrap();
        let watches = handle.list().await.unwrap();

        assert_eq!(removed, 1);
        assert_eq!(watches.len(), 2);
        assert!(!watches.iter().any(|watch| watch.id == "session"));
    }

    #[tokio::test]
    async fn operations_after_shutdown_return_error() {
        let (_dir, path) = temp_persist_path();
        let (handle, join) = spawn_with_path(vec![], Some(path));
        handle.shutdown().await;
        join.await.unwrap();
        assert!(handle.list().await.is_err());
    }
}
