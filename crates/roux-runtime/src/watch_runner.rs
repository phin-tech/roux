use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use roux_core::{
    RuntimeState, Watch, WatchKind, WatchMode, WatchOutcome, WatchResult, WatchScope,
    WatchUpdateEvent,
};

use crate::automation_hooks::{AutomationHookManager, HookContext, HookEvent};
use crate::watch_checks;
use crate::watch_service::WatchStoreHandle;

const EVENT_BUFFER: usize = 512;

#[allow(dead_code)]
struct WatchTask {
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<()>,
    generation: u64,
}

#[derive(Clone)]
pub struct WatchRunner {
    store: WatchStoreHandle,
    hooks: AutomationHookManager,
    handles: Arc<Mutex<HashMap<String, WatchTask>>>,
    events: broadcast::Sender<WatchUpdateEvent>,
    next_generation: Arc<AtomicU64>,
}

impl WatchRunner {
    pub fn new(store: WatchStoreHandle, hooks: AutomationHookManager) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            store,
            hooks,
            handles: Arc::new(Mutex::new(HashMap::new())),
            events,
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WatchUpdateEvent> {
        self.events.subscribe()
    }

    pub fn shutdown(&self) {
        self.cancel_all();
    }

    pub async fn start_all(&self) {
        let watches = self.store.list().await.unwrap_or_default();
        for (i, watch) in watches.iter().enumerate() {
            if matches!(watch.runtime_state, RuntimeState::Stopped | RuntimeState::Paused) {
                self.cancel_watch(&watch.id);
                continue;
            }
            let jitter = Duration::from_millis((i as u64) * 500 + rand_jitter());
            self.spawn_watch(watch.id.clone(), Some(jitter));
        }
    }

    pub async fn sync_to_store(&self) {
        let watches = self.store.list().await.unwrap_or_default();
        let live_ids: HashSet<String> = watches.iter().map(|watch| watch.id.clone()).collect();
        let handle_ids: Vec<String> = self.handles.lock().unwrap().keys().cloned().collect();
        for id in handle_ids {
            if !live_ids.contains(&id) {
                self.cancel_watch(&id);
            }
        }
        self.start_all().await;
    }

    pub async fn add_watch(&self, watch: Watch) -> Result<Watch, String> {
        self.store.add(watch.clone()).await.map_err(|err| err.to_string())?;
        if is_runnable(&watch) {
            self.spawn_watch(watch.id.clone(), None);
        }
        Ok(watch)
    }

    pub async fn find_or_add_github_pr(&self, watch: Watch) -> Result<Watch, String> {
        if matches!(watch.kind, WatchKind::GithubPr { .. }) {
            let (resolved, was_new) = self
                .store
                .find_or_add_github_pr(watch.clone())
                .await
                .map_err(|err| err.to_string())?;
            if was_new && is_runnable(&resolved) {
                self.spawn_watch(resolved.id.clone(), None);
            }
            Ok(resolved)
        } else {
            self.add_watch(watch).await
        }
    }

    pub async fn replace_watch(&self, watch: Watch) -> Result<Watch, String> {
        self.store.replace(watch.clone()).await.map_err(|err| err.to_string())?;
        if is_runnable(&watch) {
            self.spawn_watch(watch.id.clone(), None);
        } else {
            self.cancel_watch(&watch.id);
        }
        self.emit_watch_update(watch.clone(), false, None);
        Ok(watch)
    }

    pub async fn remove_watch(&self, id: &str) -> Result<(), String> {
        self.cancel_watch(id);
        self.store.remove(id).await.map_err(|err| err.to_string())
    }

    pub async fn pause_watch(&self, id: &str) -> Result<Watch, String> {
        self.cancel_watch(id);
        self.store
            .update(id, |watch| {
                watch.runtime_state = RuntimeState::Paused;
            })
            .await
            .map_err(|err| err.to_string())?;
        self.emit_snapshot(id, false, None).await
    }

    pub async fn resume_watch(&self, id: &str) -> Result<Watch, String> {
        self.store
            .update(id, |watch| {
                watch.runtime_state = RuntimeState::Active;
            })
            .await
            .map_err(|err| err.to_string())?;
        let watch = self.emit_snapshot(id, false, None).await?;
        self.spawn_watch(id.to_string(), None);
        Ok(watch)
    }

    pub async fn remove_watches_for_session(&self, session_id: &str) -> Result<usize, String> {
        let watches = self.store.list().await.map_err(|err| err.to_string())?;
        let ids: Vec<String> = watches
            .iter()
            .filter(|watch| {
                matches!(&watch.scope, WatchScope::Session { session_id: scoped } if scoped == session_id)
            })
            .map(|watch| watch.id.clone())
            .collect();
        for id in &ids {
            self.cancel_watch(id);
        }
        self.store.remove_for_session(session_id).await.map_err(|err| err.to_string())
    }

    pub async fn cleanup_orphans(
        &self,
        session_ids: Vec<String>,
        project_ids: Vec<String>,
    ) -> Result<usize, String> {
        let removed = self
            .store
            .cleanup_orphans(session_ids, project_ids)
            .await
            .map_err(|err| err.to_string())?;
        self.sync_to_store().await;
        Ok(removed)
    }

    fn spawn_watch(&self, watch_id: String, initial_delay: Option<Duration>) {
        self.cancel_watch(&watch_id);

        let store = self.store.clone();
        let hooks = self.hooks.clone();
        let events = self.events.clone();
        let handles = Arc::clone(&self.handles);
        let handles_for_cleanup = Arc::clone(&self.handles);
        let watch_id_for_handles = watch_id.clone();
        let watch_id_for_cleanup = watch_id.clone();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);

        let join = tokio::spawn(async move {
            if let Some(delay) = initial_delay {
                tokio::select! {
                    _ = sleep(delay) => {}
                    _ = cancel_clone.cancelled() => return,
                }
            }

            loop {
                let watch = match store.get(&watch_id).await {
                    Ok(Some(watch)) => watch,
                    _ => break,
                };
                if !is_runnable(&watch) {
                    break;
                }

                let previous_outcome = watch.last_result.as_ref().map(|r| r.outcome().clone());
                let check_timeout = match &watch.kind {
                    WatchKind::HttpHealth { .. } => Duration::from_secs(10),
                    _ => Duration::from_secs(30),
                };

                let mut hook_context = HookContext::for_watch(HookEvent::PreWatchRun, &watch);
                let pre_hook_result = tokio::select! {
                    res = hooks.run_blocking(HookEvent::PreWatchRun, hook_context.clone()) => res,
                    _ = cancel_clone.cancelled() => break,
                };

                let result = match pre_hook_result {
                    Ok(_) => {
                        tokio::select! {
                            r = tokio::time::timeout(check_timeout, watch_checks::execute_check(&watch.kind)) => {
                                match r {
                                    Ok(result) => result,
                                    Err(_) => watch_checks::timeout_result(&watch.kind),
                                }
                            }
                            _ = cancel_clone.cancelled() => break,
                        }
                    }
                    Err(e) => {
                        hook_context.hook_type = HookEvent::PreWatchRun.as_str().to_string();
                        WatchResult::CommandRun {
                            exit_code: -1,
                            stdout: String::new(),
                            stderr: format!("pre-watch-run hook failed: {e}"),
                            outcome: WatchOutcome::Failure,
                        }
                    }
                };

                let new_outcome = result.outcome().clone();
                let changed = previous_outcome.as_ref() != Some(&new_outcome);
                let now = unix_now_ms();

                let result_clone = result.clone();
                let _ = store
                    .update(&watch_id, move |watch| {
                        watch.last_result = Some(result_clone);
                        watch.last_checked = Some(now);
                        watch.runtime_state = RuntimeState::Active;
                    })
                    .await;

                if let Ok(Some(updated_watch)) = store.get(&watch_id).await {
                    let event = WatchUpdateEvent {
                        watch: updated_watch.clone(),
                        changed,
                        previous_outcome,
                    };
                    let _ = events.send(event.clone());

                    let mut post_context =
                        HookContext::for_watch(HookEvent::PostWatchRun, &updated_watch);
                    post_context.previous_outcome = event.previous_outcome.clone();
                    post_context.outcome =
                        updated_watch.last_result.as_ref().map(|r| r.outcome().clone());
                    hooks.spawn_background(HookEvent::PostWatchRun, post_context.clone());
                    if changed {
                        hooks.spawn_background(
                            HookEvent::PostWatchChange,
                            HookContext {
                                hook_type: HookEvent::PostWatchChange.as_str().into(),
                                ..post_context.clone()
                            },
                        );
                        match post_context.outcome.as_ref() {
                            Some(WatchOutcome::Failure) => {
                                hooks.spawn_background(
                                    HookEvent::PostWatchFailure,
                                    HookContext {
                                        hook_type: HookEvent::PostWatchFailure.as_str().into(),
                                        ..post_context.clone()
                                    },
                                );
                            }
                            Some(WatchOutcome::Success) => {
                                hooks.spawn_background(
                                    HookEvent::PostWatchSuccess,
                                    HookContext {
                                        hook_type: HookEvent::PostWatchSuccess.as_str().into(),
                                        ..post_context.clone()
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                }

                let should_stop = matches!(watch.mode, WatchMode::OneShot)
                    || matches!(
                        (&watch.kind, &new_outcome),
                        (
                            WatchKind::GithubAction { .. },
                            WatchOutcome::Success | WatchOutcome::Failure
                        )
                    )
                    || matches!(
                        (&watch.kind, &result),
                        (WatchKind::GithubPr { .. }, WatchResult::GithubPr { ref state, .. })
                        if state == "merged" || state == "closed"
                    );
                if should_stop {
                    let _ = store
                        .update(&watch_id, |watch| {
                            watch.runtime_state = RuntimeState::Stopped;
                        })
                        .await;
                    if let Ok(Some(stopped_watch)) = store.get(&watch_id).await {
                        let _ = events.send(WatchUpdateEvent {
                            watch: stopped_watch,
                            changed: false,
                            previous_outcome: Some(new_outcome),
                        });
                    }
                    break;
                }

                let interval = match &watch.mode {
                    WatchMode::Recurring { interval_secs } => Duration::from_secs(*interval_secs),
                    WatchMode::OneShot => break,
                };

                tokio::select! {
                    _ = sleep(interval) => {}
                    _ = cancel_clone.cancelled() => break,
                }
            }

            let mut handles_guard = handles_for_cleanup.lock().unwrap();
            if handles_guard.get(&watch_id_for_cleanup).map(|h| h.generation) == Some(generation) {
                handles_guard.remove(&watch_id_for_cleanup);
            }
        });

        handles
            .lock()
            .unwrap()
            .insert(watch_id_for_handles, WatchTask { cancel, join, generation });
    }

    fn cancel_watch(&self, id: &str) {
        let mut handles = self.handles.lock().unwrap();
        if let Some(handle) = handles.remove(id) {
            handle.cancel.cancel();
        }
    }

    fn cancel_all(&self) {
        let mut handles = self.handles.lock().unwrap();
        for (_, handle) in handles.drain() {
            handle.cancel.cancel();
        }
    }

    async fn emit_snapshot(
        &self,
        id: &str,
        changed: bool,
        previous_outcome: Option<WatchOutcome>,
    ) -> Result<Watch, String> {
        match self.store.get(id).await.map_err(|err| err.to_string())? {
            Some(watch) => {
                self.emit_watch_update(watch.clone(), changed, previous_outcome);
                Ok(watch)
            }
            None => Err("watch not found".to_string()),
        }
    }

    fn emit_watch_update(
        &self,
        watch: Watch,
        changed: bool,
        previous_outcome: Option<WatchOutcome>,
    ) {
        let _ = self.events.send(WatchUpdateEvent { watch, changed, previous_outcome });
    }
}

fn is_runnable(watch: &Watch) -> bool {
    !matches!(watch.runtime_state, RuntimeState::Stopped | RuntimeState::Paused)
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 5000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{NotifyConfig, WatchMode};

    fn shell_watch(id: &str) -> Watch {
        Watch {
            id: id.to_string(),
            name: "Shell".to_string(),
            kind: WatchKind::ShellCommand {
                command: "printf ok".to_string(),
                working_dir: None,
                success_exit_code: 0,
            },
            mode: WatchMode::OneShot,
            scope: WatchScope::Global,
            runtime_state: RuntimeState::Active,
            last_result: None,
            last_checked: None,
            notify: NotifyConfig::default(),
            created_at: 0,
        }
    }

    async fn next_update(rx: &mut broadcast::Receiver<WatchUpdateEvent>) -> WatchUpdateEvent {
        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for watch update")
            .expect("watch event channel closed")
    }

    #[tokio::test]
    async fn one_shot_watch_runs_and_emits_update() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _join) =
            crate::watch_service::spawn_with_path(vec![], Some(dir.path().join("watches.json")));
        let runner =
            WatchRunner::new(store.clone(), AutomationHookManager::from_config_root(dir.path()));
        let mut rx = runner.subscribe();
        runner.add_watch(shell_watch("w1")).await.unwrap();

        let event = next_update(&mut rx).await;
        assert_eq!(event.watch.id, "w1");
        assert!(matches!(event.watch.last_result, Some(WatchResult::CommandRun { .. })));
        let stopped = next_update(&mut rx).await;
        assert!(matches!(stopped.watch.runtime_state, RuntimeState::Stopped));
    }

    #[tokio::test]
    async fn pause_cancels_and_emits_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let watch = Watch { mode: WatchMode::Recurring { interval_secs: 30 }, ..shell_watch("w1") };
        let (store, _join) = crate::watch_service::spawn_with_path(
            vec![watch],
            Some(dir.path().join("watches.json")),
        );
        let runner =
            WatchRunner::new(store.clone(), AutomationHookManager::from_config_root(dir.path()));
        let mut rx = runner.subscribe();
        runner.start_all().await;
        let paused = runner.pause_watch("w1").await.unwrap();

        assert!(matches!(paused.runtime_state, RuntimeState::Paused));
        let event = next_update(&mut rx).await;
        assert!(matches!(event.watch.runtime_state, RuntimeState::Paused));
    }
}
