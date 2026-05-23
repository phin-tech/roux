use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use roux_core::{
    ActionKind, NotificationAction, NotificationLevel, NotificationRequest, NotificationSource,
    RuntimeState, Watch, WatchKind, WatchMode, WatchOutcome, WatchResult, WatchScope,
    WatchUpdateEvent,
};

use super::checks;
use super::flap::FlapTracker;
use super::store::WatchStoreHandle;
use crate::daemon_client::DaemonClient;
use crate::state::AppState;

#[allow(dead_code)]
pub struct WatchHandle {
    pub cancel: CancellationToken,
    pub join: tokio::task::JoinHandle<()>,
    /// Monotonic id assigned at spawn time. The poll task carries the
    /// same id so its self-cleanup can distinguish "I'm still the live
    /// entry" from "a newer spawn has replaced me".
    pub generation: u64,
}

#[derive(Clone)]
pub struct WatchManager {
    store: WatchStoreHandle,
    handles: Arc<Mutex<HashMap<String, WatchHandle>>>,
    flap_trackers: Arc<Mutex<HashMap<String, FlapTracker>>>,
    /// Counter for [`WatchHandle::generation`]. Process-wide; ids are
    /// only ever compared per-watch_id, so wrap-around in a 64-bit
    /// counter is not a concern.
    next_generation: Arc<AtomicU64>,
    daemon_client: Option<DaemonClient>,
}

impl WatchManager {
    pub fn new(store: WatchStoreHandle, daemon_client: Option<DaemonClient>) -> Self {
        Self {
            store,
            handles: Arc::new(Mutex::new(HashMap::new())),
            flap_trackers: Arc::new(Mutex::new(HashMap::new())),
            next_generation: Arc::new(AtomicU64::new(1)),
            daemon_client,
        }
    }

    pub fn store(&self) -> &WatchStoreHandle {
        &self.store
    }

    pub fn start_all(&self, app: tauri::AppHandle) {
        let store = self.store.clone();
        let manager = self.clone();
        tokio::spawn(async move {
            let watches = store.list().await.unwrap_or_default();
            for (i, watch) in watches.iter().enumerate() {
                if matches!(watch.runtime_state, RuntimeState::Stopped | RuntimeState::Paused) {
                    continue;
                }
                let jitter = Duration::from_millis((i as u64) * 500 + rand_jitter());
                manager.spawn_watch(watch.id.clone(), Some(jitter), app.clone());
            }
        });
    }

    pub async fn create_watch(&self, mut watch: Watch, app: tauri::AppHandle) -> Watch {
        watch.runtime_state = RuntimeState::Active;
        let _ = self.store.add(watch.clone()).await;
        self.spawn_watch(watch.id.clone(), None, app);
        watch
    }

    /// Mirror a daemon-owned watch into the desktop's in-memory executor.
    /// Durable state remains daemon-owned; this local copy exists so the
    /// current Tauri client can run checks and surface UX notifications.
    pub async fn adopt_watch(&self, watch: Watch, app: tauri::AppHandle) {
        let id = watch.id.clone();
        let runnable = !matches!(watch.runtime_state, RuntimeState::Stopped | RuntimeState::Paused);
        let _ = self.store.replace(watch).await;
        if runnable {
            self.spawn_watch(id, None, app);
        } else {
            self.cancel_watch(&id);
        }
    }

    /// Replace the local mirror with daemon state, cancelling pollers for
    /// watches that disappeared and starting active watches from the daemon
    /// snapshot.
    pub async fn sync_watches(&self, watches: Vec<Watch>, app: tauri::AppHandle) {
        let next_ids: std::collections::HashSet<String> =
            watches.iter().map(|watch| watch.id.clone()).collect();
        if let Ok(existing) = self.store.list().await {
            for watch in existing {
                if !next_ids.contains(&watch.id) {
                    self.cancel_watch(&watch.id);
                }
            }
        }
        let _ = self.store.replace_all(watches).await;
        self.start_all(app);
    }

    /// Atomic find-or-create for `GithubPr` watches keyed on
    /// `(scope, repo, pr_number)`. Concurrent callers serialize through
    /// the store actor, so two racing `installSessionPrEffect` triggers
    /// resolve to the same watch instead of creating duplicates. Returns
    /// the resolved watch; only newly-created watches kick off a poll
    /// loop (existing ones are already running).
    pub async fn find_or_create_github_pr_watch(
        &self,
        mut watch: Watch,
        app: tauri::AppHandle,
    ) -> Watch {
        watch.runtime_state = RuntimeState::Active;
        match self.store.find_or_add_github_pr(watch.clone()).await {
            Ok((resolved, was_new)) => {
                if was_new {
                    self.spawn_watch(resolved.id.clone(), None, app);
                }
                resolved
            }
            Err(_) => watch,
        }
    }

    pub async fn remove_watch(&self, id: &str) {
        self.cancel_watch(id);
        let _ = self.store.remove(id).await;
    }

    /// Remove every watch scoped to the given session. Called when a
    /// session is deleted so its recurring pollers (e.g. `gh pr view`
    /// every 30s) don't outlive the session and keep firing forever.
    pub async fn remove_watches_for_session(&self, session_id: &str) {
        let watches = match self.store.list().await {
            Ok(list) => list,
            Err(_) => return,
        };
        for watch in watches {
            if let WatchScope::Session { session_id: scoped } = &watch.scope {
                if scoped == session_id {
                    self.remove_watch(&watch.id).await;
                }
            }
        }
    }

    pub async fn pause_watch(&self, id: &str, app: &tauri::AppHandle) {
        self.cancel_watch(id);
        let _ = self
            .store
            .update(id, |w| {
                w.runtime_state = RuntimeState::Paused;
            })
            .await;
        self.emit_watch_update(id, app).await;
    }

    pub async fn resume_watch(&self, id: &str, app: tauri::AppHandle) {
        let _ = self
            .store
            .update(id, |w| {
                w.runtime_state = RuntimeState::Active;
            })
            .await;
        self.emit_watch_update(id, &app).await;
        self.spawn_watch(id.to_string(), None, app);
    }

    async fn emit_watch_update(&self, id: &str, app: &tauri::AppHandle) {
        if let Ok(Some(watch)) = self.store.get(id).await {
            let event = WatchUpdateEvent { watch, changed: false, previous_outcome: None };
            let _ = app.emit("watch-update", &event);
        }
    }

    fn cancel_watch(&self, id: &str) {
        let mut handles = self.handles.lock().unwrap();
        if let Some(handle) = handles.remove(id) {
            handle.cancel.cancel();
        }
    }

    fn spawn_watch(
        &self,
        watch_id: String,
        initial_delay: Option<Duration>,
        app: tauri::AppHandle,
    ) {
        self.cancel_watch(&watch_id);

        let store = self.store.clone();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let handles = Arc::clone(&self.handles);
        let flap_trackers = Arc::clone(&self.flap_trackers);
        let watch_id_for_handles = watch_id.clone();
        let watch_id_for_cleanup = watch_id.clone();
        let handles_for_cleanup = Arc::clone(&self.handles);
        let daemon_client = self.daemon_client.clone();
        // Generation tag so the cleanup at task end can tell "I'm still
        // the live entry" from "a later spawn_watch replaced me". Without
        // this, an old cancelled task that races past `cancel_watch` and
        // the new `insert` will delete the new task's handle, leaving a
        // live poll loop with no way to pause/remove it.
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
                    Ok(Some(w)) => w,
                    _ => break,
                };

                let previous_outcome = watch.last_result.as_ref().map(|r| r.outcome().clone());

                let check_timeout = match &watch.kind {
                    WatchKind::HttpHealth { .. } => Duration::from_secs(10),
                    _ => Duration::from_secs(30),
                };

                let state = app.state::<AppState>();
                let mut hook_context = crate::automation_hooks::HookContext::for_watch(
                    crate::automation_hooks::HookEvent::PreWatchRun,
                    &watch,
                );
                // Race the hook against cancellation. If the watch is
                // cancelled (session deleted, watch removed) while a
                // user-defined pre-watch-run hook is wedged, dropping
                // the future cancels the hook child (kill_on_drop on
                // the spawned process).
                let pre_hook_result = tokio::select! {
                    res = state.automation_hooks.run_blocking(
                        crate::automation_hooks::HookEvent::PreWatchRun,
                        hook_context.clone(),
                    ) => res,
                    _ = cancel_clone.cancelled() => break,
                };

                let result = match pre_hook_result {
                    Ok(_) => {
                        tokio::select! {
                            r = tokio::time::timeout(check_timeout, checks::execute_check(&watch.kind)) => {
                                match r {
                                    Ok(result) => result,
                                    Err(_) => checks::timeout_result(&watch.kind),
                                }
                            }
                            _ = cancel_clone.cancelled() => break,
                        }
                    }
                    Err(e) => {
                        hook_context.hook_type =
                            crate::automation_hooks::HookEvent::PreWatchRun.as_str().to_string();
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

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let result_clone = result.clone();
                let _ = store
                    .update(&watch_id, move |w| {
                        w.last_result = Some(result_clone);
                        w.last_checked = Some(now);
                        w.runtime_state = RuntimeState::Active;
                    })
                    .await;

                if let Ok(Some(updated_watch)) = store.get(&watch_id).await {
                    if let Some(client) = daemon_client.as_ref() {
                        if let Err(err) = client.replace_watch(updated_watch.clone()).await {
                            rlog!("daemon watch sync failed for {}: {err}", updated_watch.id);
                        }
                    }
                    let event = WatchUpdateEvent {
                        watch: updated_watch.clone(),
                        changed,
                        previous_outcome,
                    };
                    let _ = app.emit("watch-update", &event);

                    let mut post_context = crate::automation_hooks::HookContext::for_watch(
                        crate::automation_hooks::HookEvent::PostWatchRun,
                        &updated_watch,
                    );
                    post_context.previous_outcome = event.previous_outcome.clone();
                    post_context.outcome =
                        updated_watch.last_result.as_ref().map(|r| r.outcome().clone());
                    state.automation_hooks.spawn_background(
                        crate::automation_hooks::HookEvent::PostWatchRun,
                        post_context.clone(),
                    );
                    if changed {
                        state.automation_hooks.spawn_background(
                            crate::automation_hooks::HookEvent::PostWatchChange,
                            crate::automation_hooks::HookContext {
                                hook_type: crate::automation_hooks::HookEvent::PostWatchChange
                                    .as_str()
                                    .into(),
                                ..post_context.clone()
                            },
                        );
                        match post_context.outcome.as_ref() {
                            Some(WatchOutcome::Failure) => {
                                state.automation_hooks.spawn_background(
                                    crate::automation_hooks::HookEvent::PostWatchFailure,
                                    crate::automation_hooks::HookContext {
                                        hook_type:
                                            crate::automation_hooks::HookEvent::PostWatchFailure
                                                .as_str()
                                                .into(),
                                        ..post_context.clone()
                                    },
                                );
                            }
                            Some(WatchOutcome::Success) => {
                                state.automation_hooks.spawn_background(
                                    crate::automation_hooks::HookEvent::PostWatchSuccess,
                                    crate::automation_hooks::HookContext {
                                        hook_type:
                                            crate::automation_hooks::HookEvent::PostWatchSuccess
                                                .as_str()
                                                .into(),
                                        ..post_context.clone()
                                    },
                                );
                            }
                            _ => {}
                        }
                    }

                    // Desktop notification with flap debouncing
                    if changed {
                        let outcome = updated_watch.last_result.as_ref().map(|r| r.outcome());

                        let suppress = {
                            let mut trackers = flap_trackers.lock().unwrap();
                            let tracker =
                                trackers.entry(watch_id.clone()).or_insert_with(FlapTracker::new);
                            if let Some(o) = outcome {
                                tracker.record(o.clone(), now);
                            }
                            tracker.is_flapping()
                        };

                        let should_notify = !suppress
                            && match outcome {
                                Some(WatchOutcome::Failure) => {
                                    updated_watch.notify.desktop_notification
                                        && updated_watch.notify.on_failure
                                }
                                Some(WatchOutcome::Success) => {
                                    updated_watch.notify.desktop_notification
                                        && updated_watch.notify.on_success
                                }
                                _ => false,
                            };
                        if should_notify {
                            let title = match outcome {
                                Some(WatchOutcome::Failure) => format!("❌ {}", updated_watch.name),
                                Some(WatchOutcome::Success) => format!("✅ {}", updated_watch.name),
                                _ => updated_watch.name.clone(),
                            };
                            let body = match &updated_watch.last_result {
                                Some(WatchResult::GithubRun { conclusion, url, .. }) => {
                                    format!(
                                        "{} — {}",
                                        conclusion.as_deref().unwrap_or("unknown"),
                                        url
                                    )
                                }
                                Some(WatchResult::HttpCheck {
                                    status_code,
                                    response_time_ms,
                                    ..
                                }) => {
                                    format!("HTTP {} ({}ms)", status_code, response_time_ms)
                                }
                                Some(WatchResult::CommandRun { exit_code, .. }) => {
                                    format!("Exit code: {}", exit_code)
                                }
                                Some(WatchResult::GithubPr { state, checks, reviews, .. }) => {
                                    let passed = checks
                                        .iter()
                                        .filter(|c| c.conclusion.as_deref() == Some("success"))
                                        .count();
                                    let approvals =
                                        reviews.iter().filter(|r| r.state == "approved").count();
                                    format!(
                                        "{} — {}/{} checks passed, {} approval(s)",
                                        state,
                                        passed,
                                        checks.len(),
                                        approvals
                                    )
                                }
                                None => String::new(),
                            };
                            // Push through the notification service. The policy
                            // layer inside the service decides whether to fan
                            // out to the OS (respecting focus + kill switch).
                            let state = app.state::<AppState>();
                            let session_id = match &updated_watch.scope {
                                WatchScope::Session { session_id } => Some(session_id.clone()),
                                _ => None,
                            };
                            let level = match outcome {
                                Some(WatchOutcome::Failure) => NotificationLevel::Error,
                                Some(WatchOutcome::Success) => NotificationLevel::Success,
                                _ => NotificationLevel::Info,
                            };
                            let mut actions: Vec<NotificationAction> = Vec::new();
                            if let Some(ref sid) = session_id {
                                actions.push(NotificationAction {
                                    id: "focus".into(),
                                    label: "Focus session".into(),
                                    kind: ActionKind::FocusSession { session_id: sid.clone() },
                                    primary: true,
                                });
                            }
                            if matches!(outcome, Some(WatchOutcome::Failure)) {
                                actions.push(NotificationAction {
                                    id: "retry".into(),
                                    label: "Retry".into(),
                                    kind: ActionKind::RetryWatch {
                                        watch_id: updated_watch.id.clone(),
                                    },
                                    primary: actions.is_empty(),
                                });
                                actions.push(NotificationAction {
                                    id: "dismiss_source".into(),
                                    label: "Dismiss all from source".into(),
                                    kind: ActionKind::DismissSource,
                                    primary: false,
                                });
                            } else {
                                actions.push(NotificationAction {
                                    id: "dismiss".into(),
                                    label: "Dismiss".into(),
                                    kind: ActionKind::Dismiss,
                                    primary: false,
                                });
                            }
                            state.notification_manager.push(
                                NotificationRequest {
                                    level,
                                    source: NotificationSource::Watch {
                                        watch_id: updated_watch.id.clone(),
                                    },
                                    title,
                                    subtitle: None,
                                    body: Some(body),
                                    session_id,
                                    actions,
                                    dedup_key: None,
                                },
                                Some(&app),
                            );
                        }
                    }
                }

                // Auto-stop conditions
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
                        .update(&watch_id, |w| {
                            w.runtime_state = RuntimeState::Stopped;
                        })
                        .await;
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
            // Only remove the entry if it's still ours. A later spawn
            // for the same watch_id may have replaced us, in which case
            // its handle must stay in the map.
            if handles_guard.get(&watch_id_for_cleanup).map(|h| h.generation) == Some(generation) {
                handles_guard.remove(&watch_id_for_cleanup);
            }
        });

        let mut handles_guard = handles.lock().unwrap();
        handles_guard.insert(watch_id_for_handles, WatchHandle { cancel, join, generation });
    }
}

fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 5000) as u64
}
