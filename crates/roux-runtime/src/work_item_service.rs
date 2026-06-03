//! Work item service — clonable handle over the SQLite-backed work item store.
//!
//! `WorkItemHandle` is `Clone` (Arc-based) and its methods are synchronous
//! (the store is a Mutex-guarded SQLite connection). This mirrors the
//! `MailboxManager` pattern; it is **not** a channel-based actor service and
//! is **not** added to the `RuntimeHost.services` vec.
//!
//! All mutations broadcast a `WorkItemEvent` **after** the successful write
//! so listeners receive only persisted state (persist-before-broadcast).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use uuid::Uuid;

use roux_core::{
    Attachment, AttachmentDocument, AttachmentInput, AttachmentTargetKind, WorkItem,
    WorkItemDecision, WorkItemDecisionOption, WorkItemEvent, WorkItemInput, WorkItemRun,
    WorkItemRunEvent, WorkItemRunEventKind, WorkItemRunKind, WorkItemRunStatus, WorkItemStatus,
};

use crate::work_item_store::WorkItemStore;

const BROADCAST_CAPACITY: usize = 256;

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Clone)]
pub struct WorkItemHandle {
    inner: Arc<Mutex<WorkItemStore>>,
    broadcast_tx: broadcast::Sender<WorkItemEvent>,
}

impl WorkItemHandle {
    pub fn open(path: &Path) -> Result<Self, String> {
        let store =
            WorkItemStore::open(path).map_err(|e| format!("failed to open board.db: {e}"))?;
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Ok(WorkItemHandle { inner: Arc::new(Mutex::new(store)), broadcast_tx })
    }

    pub fn in_memory() -> Self {
        let store = WorkItemStore::open_in_memory().expect("in-memory SQLite should always work");
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        WorkItemHandle { inner: Arc::new(Mutex::new(store)), broadcast_tx }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<WorkItemEvent> {
        self.broadcast_tx.subscribe()
    }

    fn broadcast(&self, event: WorkItemEvent) {
        let _ = self.broadcast_tx.send(event);
    }

    pub fn list(&self, project_id: Option<&str>) -> Result<Vec<WorkItem>, String> {
        self.inner.lock().unwrap().list(project_id).map_err(|e| format!("work-item list: {e}"))
    }

    pub fn get(&self, id: &str) -> Result<Option<WorkItem>, String> {
        self.inner.lock().unwrap().get(id).map_err(|e| format!("work-item get: {e}"))
    }

    pub fn create(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .create(id, input, now)
            .map_err(|e| format!("work-item create: {e}"))?;
        self.broadcast(WorkItemEvent::Created { item: item.clone() });
        Ok(item)
    }

    pub fn update(&self, id: &str, input: WorkItemInput) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .update(id, input, now)
            .map_err(|e| format!("work-item update: {e}"))?;
        if let Some(ref i) = item {
            self.broadcast(WorkItemEvent::Updated { item: i.clone() });
        }
        Ok(item)
    }

    pub fn move_item(
        &self,
        id: &str,
        status: WorkItemStatus,
        sort_order: f64,
    ) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .move_item(id, status.clone(), sort_order, now)
            .map_err(|e| format!("work-item move: {e}"))?;
        if item.is_some() {
            self.broadcast(WorkItemEvent::Moved { id: id.to_string(), status, sort_order });
        }
        Ok(item)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let deleted =
            self.inner.lock().unwrap().delete(id).map_err(|e| format!("work-item delete: {e}"))?;
        if deleted {
            self.broadcast(WorkItemEvent::Deleted { id: id.to_string() });
        }
        Ok(deleted)
    }

    pub fn create_attachment(&self, input: AttachmentInput) -> Result<Attachment, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let byte_len = input.content.len() as u64;
        let sha256 = sha256_hex(input.content.as_bytes());
        let attachment = self
            .inner
            .lock()
            .unwrap()
            .create_attachment(id, input, byte_len, sha256, now)
            .map_err(|e| format!("attachment create: {e}"))?;
        self.broadcast(WorkItemEvent::DocumentAttached { attachment: attachment.clone() });
        Ok(attachment)
    }

    pub fn list_attachments(
        &self,
        target_kind: Option<AttachmentTargetKind>,
        target_id: Option<&str>,
    ) -> Result<Vec<Attachment>, String> {
        if target_id.is_some() && target_kind.is_none() {
            return Err("targetKind required when targetId is provided".to_string());
        }
        self.inner
            .lock()
            .unwrap()
            .list_attachments(target_kind, target_id)
            .map_err(|e| format!("attachment list: {e}"))
    }

    pub fn get_attachment_document(
        &self,
        document_id: &str,
    ) -> Result<Option<AttachmentDocument>, String> {
        self.inner
            .lock()
            .unwrap()
            .get_attachment_document(document_id)
            .map_err(|e| format!("attachment get: {e}"))
    }

    pub fn set_session(&self, id: &str, session_id: &str) -> Result<Option<WorkItem>, String> {
        self.attach_session(id, session_id)
    }

    pub fn attach_session(&self, id: &str, session_id: &str) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .set_session(id, session_id, now)
            .map_err(|e| format!("work-item attach-session: {e}"))?;
        if item.is_some() {
            self.broadcast(WorkItemEvent::SessionBound {
                id: id.to_string(),
                session_id: session_id.to_string(),
            });
        }
        Ok(item)
    }

    pub fn detach_session(&self, id: &str) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .detach_session(id, now)
            .map_err(|e| format!("work-item detach-session: {e}"))?;
        if let Some(ref item) = item {
            self.broadcast(WorkItemEvent::Updated { item: item.clone() });
        }
        Ok(item)
    }

    /// Bind a session only if the item is still unbound, broadcasting
    /// `SessionBound` only when this call wins. Returns whether the bind
    /// happened so callers can roll back a now-orphaned session on a lost race.
    pub fn set_session_if_unbound(&self, id: &str, session_id: &str) -> Result<bool, String> {
        let now = now_secs();
        let bound = self
            .inner
            .lock()
            .unwrap()
            .set_session_if_unbound(id, session_id, now)
            .map_err(|e| format!("work-item set-session: {e}"))?;
        if bound {
            self.broadcast(WorkItemEvent::SessionBound {
                id: id.to_string(),
                session_id: session_id.to_string(),
            });
        }
        Ok(bound)
    }

    pub fn has_active_run(&self, work_item_id: &str) -> Result<bool, String> {
        self.inner
            .lock()
            .unwrap()
            .has_active_run(work_item_id)
            .map_err(|e| format!("work-item active run check: {e}"))
    }

    pub fn upsert_by_external(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .upsert_by_external(id, input, now)
            .map_err(|e| format!("work-item upsert: {e}"))?;
        Ok(item)
    }

    /// Insert an item without broadcasting a per-item event. Used by the
    /// import handler so the batch `Imported` event is the only signal emitted.
    pub fn update_silent(
        &self,
        id: &str,
        input: WorkItemInput,
    ) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        self.inner
            .lock()
            .unwrap()
            .update(id, input, now)
            .map_err(|e| format!("work-item update: {e}"))
    }

    pub fn insert_silent(&self, input: WorkItemInput) -> Result<WorkItem, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        self.inner
            .lock()
            .unwrap()
            .create(id, input, now)
            .map_err(|e| format!("work-item insert: {e}"))
    }

    pub fn broadcast_imported(&self, ids: Vec<String>) {
        self.broadcast(WorkItemEvent::Imported { ids });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_run(
        &self,
        work_item_id: &str,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
    ) -> Result<WorkItemRun, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let run = self
            .inner
            .lock()
            .unwrap()
            .create_run(
                id,
                work_item_id,
                session_id,
                provider,
                profile_id,
                worktree_path,
                branch,
                now,
            )
            .map_err(|e| format!("work-item run create: {e}"))?;
        self.broadcast(WorkItemEvent::RunCreated { run: run.clone() });
        Ok(run)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_starting_run(
        &self,
        work_item_id: &str,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
    ) -> Result<WorkItemRun, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let run = self
            .inner
            .lock()
            .unwrap()
            .create_run_with_status(
                id,
                work_item_id,
                session_id,
                provider,
                profile_id,
                worktree_path,
                branch,
                WorkItemRunStatus::Starting,
                now,
            )
            .map_err(|e| format!("work-item run create: {e}"))?;
        self.broadcast(WorkItemEvent::RunCreated { run: run.clone() });
        Ok(run)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_planning_run(
        &self,
        work_item_id: &str,
        session_id: Option<&str>,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
    ) -> Result<WorkItemRun, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let run = self
            .inner
            .lock()
            .unwrap()
            .create_run_with_kind_status(
                id,
                work_item_id,
                WorkItemRunKind::Planning,
                session_id,
                provider,
                profile_id,
                worktree_path,
                branch,
                WorkItemRunStatus::Starting,
                now,
            )
            .map_err(|e| format!("work-item run create: {e}"))?;
        self.broadcast(WorkItemEvent::RunCreated { run: run.clone() });
        Ok(run)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_start_failure(
        &self,
        id: &str,
        error: &str,
        session_id: Option<&str>,
        worktree_path: Option<&str>,
        agent_profile: Option<&str>,
        repo_path: Option<&str>,
        base_branch: Option<&str>,
    ) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .record_start_failure(
                id,
                error,
                session_id,
                worktree_path,
                agent_profile,
                repo_path,
                base_branch,
                now,
            )
            .map_err(|e| format!("work-item start failure: {e}"))?;
        if let Some(ref item) = item {
            self.broadcast(WorkItemEvent::Updated { item: item.clone() });
        }
        Ok(item)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_start(
        &self,
        id: &str,
        session_id: &str,
        worktree_path: Option<&str>,
        agent_profile: Option<&str>,
        repo_path: Option<&str>,
        base_branch: Option<&str>,
        sort_order: f64,
    ) -> Result<Option<WorkItem>, String> {
        let now = now_secs();
        let item = self
            .inner
            .lock()
            .unwrap()
            .complete_start(
                id,
                session_id,
                worktree_path,
                agent_profile,
                repo_path,
                base_branch,
                sort_order,
                now,
            )
            .map_err(|e| format!("work-item complete start: {e}"))?;
        if let Some(ref item) = item {
            self.broadcast(WorkItemEvent::SessionBound {
                id: item.id.clone(),
                session_id: session_id.to_string(),
            });
            self.broadcast(WorkItemEvent::Moved {
                id: item.id.clone(),
                status: item.status.clone(),
                sort_order: item.sort_order,
            });
            self.broadcast(WorkItemEvent::Updated { item: item.clone() });
        }
        Ok(item)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_run(
        &self,
        work_item_id: &str,
        session_id: &str,
        provider: Option<&str>,
        profile_id: Option<&str>,
        worktree_path: Option<&str>,
        branch: Option<&str>,
        sort_order: f64,
    ) -> Result<Option<WorkItemRun>, String> {
        let run_id = Uuid::new_v4().to_string();
        let now = now_secs();
        let result = self
            .inner
            .lock()
            .unwrap()
            .dispatch_run(
                run_id,
                work_item_id,
                session_id,
                provider,
                profile_id,
                worktree_path,
                branch,
                sort_order,
                now,
            )
            .map_err(|e| format!("work-item dispatch run: {e}"))?;
        if let Some((item, run)) = result {
            self.broadcast(WorkItemEvent::SessionBound {
                id: item.id.clone(),
                session_id: session_id.to_string(),
            });
            self.broadcast(WorkItemEvent::RunCreated { run: run.clone() });
            self.broadcast(WorkItemEvent::Moved {
                id: item.id,
                status: item.status,
                sort_order: item.sort_order,
            });
            Ok(Some(run))
        } else {
            Ok(None)
        }
    }

    pub fn list_runs(&self, work_item_id: Option<&str>) -> Result<Vec<WorkItemRun>, String> {
        self.inner
            .lock()
            .unwrap()
            .list_runs(work_item_id)
            .map_err(|e| format!("work-item run list: {e}"))
    }

    pub fn get_run(&self, id: &str) -> Result<Option<WorkItemRun>, String> {
        self.inner.lock().unwrap().get_run(id).map_err(|e| format!("work-item run get: {e}"))
    }

    pub fn append_run_event(
        &self,
        run_id: &str,
        kind: WorkItemRunEventKind,
        payload: serde_json::Value,
    ) -> Result<WorkItemRunEvent, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let event = self
            .inner
            .lock()
            .unwrap()
            .append_run_event(id, run_id, kind, payload, now)
            .map_err(|e| format!("work-item run event append: {e}"))?;
        self.broadcast(WorkItemEvent::RunEventAppended { event: event.clone() });
        Ok(event)
    }

    pub fn list_run_events(&self, run_id: &str) -> Result<Vec<WorkItemRunEvent>, String> {
        self.inner
            .lock()
            .unwrap()
            .list_run_events(run_id)
            .map_err(|e| format!("work-item run event list: {e}"))
    }

    pub fn set_run_status(
        &self,
        id: &str,
        status: WorkItemRunStatus,
        mut payload: serde_json::Value,
    ) -> Result<Option<WorkItemRun>, String> {
        let now = now_secs();
        if let serde_json::Value::Object(ref mut object) = payload {
            object
                .entry("status")
                .or_insert_with(|| serde_json::Value::String(status.as_str().to_string()));
        }
        let event_id = Uuid::new_v4().to_string();
        let result = self
            .inner
            .lock()
            .unwrap()
            .update_run_status_with_event(id, status, event_id, payload, now)
            .map_err(|e| format!("work-item run status update: {e}"))?;
        if let Some((run, event)) = result {
            self.broadcast(WorkItemEvent::RunUpdated { run: run.clone() });
            self.broadcast(WorkItemEvent::RunEventAppended { event });
            Ok(Some(run))
        } else {
            Ok(None)
        }
    }

    /// Recover transient start records left behind by a daemon crash before
    /// the profile dispatch completed. This is intended to run during daemon
    /// startup before the socket accepts new start/plan requests.
    pub fn fail_starting_runs_after_restart(&self) -> Result<Vec<WorkItemRun>, String> {
        let starting_runs = self
            .list_runs(None)?
            .into_iter()
            .filter(|run| run.status == WorkItemRunStatus::Starting)
            .collect::<Vec<_>>();
        let mut recovered = Vec::new();
        for run in starting_runs {
            if let Some(updated) = self.set_run_status(
                &run.id,
                WorkItemRunStatus::Failed,
                serde_json::json!({
                    "reason": "daemonRestartedBeforeRunStarted",
                    "previousStatus": WorkItemRunStatus::Starting.as_str(),
                }),
            )? {
                recovered.push(updated);
            }
        }
        Ok(recovered)
    }

    pub fn accept_review(
        &self,
        work_item_id: &str,
        mut payload: serde_json::Value,
    ) -> Result<Option<(WorkItem, WorkItemRun)>, String> {
        let now = now_secs();
        if let serde_json::Value::Object(ref mut object) = payload {
            object.entry("status").or_insert_with(|| {
                serde_json::Value::String(WorkItemRunStatus::Done.as_str().to_string())
            });
        }
        let event_id = Uuid::new_v4().to_string();
        let result = self
            .inner
            .lock()
            .unwrap()
            .accept_review(work_item_id, event_id, payload, now)
            .map_err(|e| format!("work-item review accept: {e}"))?;
        if let Some((item, run, event)) = result {
            self.broadcast(WorkItemEvent::RunUpdated { run: run.clone() });
            self.broadcast(WorkItemEvent::RunEventAppended { event });
            self.broadcast(WorkItemEvent::Moved {
                id: item.id.clone(),
                status: item.status.clone(),
                sort_order: item.sort_order,
            });
            self.broadcast(WorkItemEvent::Updated { item: item.clone() });
            Ok(Some((item, run)))
        } else {
            Ok(None)
        }
    }

    pub fn create_decision(
        &self,
        run_id: &str,
        question: &str,
        options: Vec<WorkItemDecisionOption>,
        default_value: Option<&str>,
        timeout_at: Option<u64>,
    ) -> Result<WorkItemDecision, String> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let decision = self
            .inner
            .lock()
            .unwrap()
            .create_decision(id, run_id, question, options, default_value, timeout_at, now)
            .map_err(|e| format!("work-item decision create: {e}"))?;
        self.append_run_event(
            &decision.run_id,
            WorkItemRunEventKind::Decision,
            serde_json::to_value(&decision)
                .map_err(|e| format!("work-item decision event encode: {e}"))?,
        )?;
        self.broadcast(WorkItemEvent::DecisionCreated { decision: decision.clone() });
        if decision.timeout_at.is_some_and(|deadline| deadline <= now) {
            return Ok(self.timeout_decision_to_default(&decision.id)?.unwrap_or(decision));
        }
        Ok(decision)
    }

    pub fn list_pending_decisions(
        &self,
        work_item_id: Option<&str>,
    ) -> Result<Vec<WorkItemDecision>, String> {
        self.expire_due_decisions()?;
        self.inner
            .lock()
            .unwrap()
            .list_pending_decisions(work_item_id)
            .map_err(|e| format!("work-item decision list: {e}"))
    }

    pub fn expire_due_decisions(&self) -> Result<Vec<WorkItemDecision>, String> {
        let now = now_secs();
        let decisions = self
            .inner
            .lock()
            .unwrap()
            .timeout_due_decisions(now)
            .map_err(|e| format!("work-item decision timeout: {e}"))?;
        for decision in &decisions {
            self.record_decision_timeout(decision)?;
        }
        Ok(decisions)
    }

    pub fn timeout_decision_to_default(
        &self,
        id: &str,
    ) -> Result<Option<WorkItemDecision>, String> {
        let now = now_secs();
        let decision = self
            .inner
            .lock()
            .unwrap()
            .timeout_decision_to_default(id, now)
            .map_err(|e| format!("work-item decision timeout: {e}"))?;
        if let Some(ref decision) = decision {
            self.record_decision_timeout(decision)?;
        }
        Ok(decision)
    }

    pub fn resolve_decision(
        &self,
        id: &str,
        value: &str,
        resolved_by: Option<&str>,
    ) -> Result<Option<WorkItemDecision>, String> {
        let now = now_secs();
        let decision = self
            .inner
            .lock()
            .unwrap()
            .resolve_decision(id, value, resolved_by, now)
            .map_err(|e| format!("work-item decision resolve: {e}"))?;
        if let Some(ref decision) = decision {
            self.append_run_event(
                &decision.run_id,
                WorkItemRunEventKind::DecisionResolved,
                serde_json::to_value(decision)
                    .map_err(|e| format!("work-item decision event encode: {e}"))?,
            )?;
            self.broadcast(WorkItemEvent::DecisionResolved { decision: decision.clone() });
        }
        Ok(decision)
    }

    fn record_decision_timeout(&self, decision: &WorkItemDecision) -> Result<(), String> {
        self.append_run_event(
            &decision.run_id,
            WorkItemRunEventKind::DecisionTimedOut,
            serde_json::to_value(decision)
                .map_err(|e| format!("work-item decision event encode: {e}"))?,
        )?;
        self.broadcast(WorkItemEvent::DecisionTimedOut { decision: decision.clone() });
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{
        AttachmentContentKind, AttachmentInput, AttachmentTargetKind, ExternalRef, WorkItemInput,
    };

    fn input(title: &str) -> WorkItemInput {
        WorkItemInput { title: title.to_string(), ..Default::default() }
    }

    #[test]
    fn create_and_list() {
        let handle = WorkItemHandle::in_memory();
        handle.create(input("Task A")).unwrap();
        handle.create(input("Task B")).unwrap();
        let items = handle.list(None).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn create_broadcasts_event() {
        let handle = WorkItemHandle::in_memory();
        let mut rx = handle.subscribe_events();

        handle.create(input("Task")).unwrap();

        let event = rx.try_recv().expect("Created event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Created { .. }));
    }

    #[test]
    fn delete_broadcasts_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        let deleted = handle.delete(&item.id).unwrap();
        assert!(deleted);

        let event = rx.try_recv().expect("Deleted event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Deleted { .. }));
    }

    #[test]
    fn move_item_broadcasts_moved_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        // Subscribe after create so the create broadcast is already gone.
        let mut rx = handle.subscribe_events();

        handle.move_item(&item.id, WorkItemStatus::Doing, 1.0).unwrap();

        // The only event in the channel is the Moved event.
        let event = rx.try_recv().expect("Moved event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Moved { .. }));
    }

    #[test]
    fn set_session_broadcasts_session_bound() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        handle.set_session(&item.id, "sess-1").unwrap();

        let event = rx.try_recv().expect("SessionBound event should be broadcast");
        assert!(matches!(event, WorkItemEvent::SessionBound { .. }));
    }

    #[test]
    fn attach_session_rejects_session_bound_to_another_item() {
        let handle = WorkItemHandle::in_memory();
        let first = handle.create(input("Task one")).unwrap();
        let second = handle.create(input("Task two")).unwrap();
        handle.attach_session(&first.id, "sess-1").unwrap().unwrap();

        let err = handle.attach_session(&second.id, "sess-1").unwrap_err();

        assert!(err.contains("session already bound"), "unexpected error: {err}");
        assert_eq!(handle.get(&first.id).unwrap().unwrap().session_id.as_deref(), Some("sess-1"));
        assert!(handle.get(&second.id).unwrap().unwrap().session_id.is_none());
    }

    #[test]
    fn detach_session_clears_only_target_item() {
        let handle = WorkItemHandle::in_memory();
        let first = handle.create(input("Task one")).unwrap();
        let second = handle.create(input("Task two")).unwrap();
        handle.attach_session(&first.id, "sess-1").unwrap().unwrap();
        handle.attach_session(&second.id, "sess-2").unwrap().unwrap();

        let detached = handle.detach_session(&first.id).unwrap().unwrap();

        assert!(detached.session_id.is_none());
        assert_eq!(handle.get(&second.id).unwrap().unwrap().session_id.as_deref(), Some("sess-2"));
    }

    #[test]
    fn run_created_event_broadcasts() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        let run = handle
            .create_run(&item.id, Some("sess-1"), Some("claude"), Some("claude"), None, None)
            .unwrap();

        assert_eq!(run.work_item_id, item.id);
        let event = rx.try_recv().expect("RunCreated event should be broadcast");
        assert!(matches!(event, WorkItemEvent::RunCreated { .. }));
    }

    #[test]
    fn bound_item_can_create_multiple_active_implementation_runs_in_same_session() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        handle.attach_session(&item.id, "sess-1").unwrap().unwrap();

        let first = handle
            .create_starting_run(
                &item.id,
                Some("sess-1"),
                Some("claude"),
                Some("claude"),
                None,
                None,
            )
            .unwrap();
        let second = handle
            .create_starting_run(
                &item.id,
                Some("sess-1"),
                Some("claude"),
                Some("claude"),
                None,
                None,
            )
            .unwrap();

        assert_ne!(first.id, second.id);
        let runs = handle.list_runs(Some(&item.id)).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|run| run.session_id.as_deref() == Some("sess-1")));
    }

    #[test]
    fn dispatch_run_broadcasts_session_run_and_move_events() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let mut rx = handle.subscribe_events();

        let run = handle
            .dispatch_run(&item.id, "sess-1", Some("claude"), Some("claude"), None, None, 1.0)
            .unwrap()
            .expect("item should exist");

        assert_eq!(run.work_item_id, item.id);
        let event = rx.try_recv().expect("SessionBound event should be broadcast");
        assert!(matches!(event, WorkItemEvent::SessionBound { .. }));
        let event = rx.try_recv().expect("RunCreated event should be broadcast");
        assert!(matches!(event, WorkItemEvent::RunCreated { .. }));
        let event = rx.try_recv().expect("Moved event should be broadcast");
        assert!(matches!(event, WorkItemEvent::Moved { .. }));
    }

    #[test]
    fn run_status_update_broadcasts_run_and_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let run = handle.create_run(&item.id, Some("sess-1"), None, None, None, None).unwrap();
        let mut rx = handle.subscribe_events();

        let updated = handle
            .set_run_status(
                &run.id,
                roux_core::WorkItemRunStatus::Stopped,
                serde_json::json!({ "reason": "user" }),
            )
            .unwrap()
            .expect("run should exist");

        assert_eq!(updated.status, roux_core::WorkItemRunStatus::Stopped);
        let event = rx.try_recv().expect("RunUpdated event should be broadcast");
        assert!(matches!(event, WorkItemEvent::RunUpdated { .. }));
        let event = rx.try_recv().expect("RunEventAppended event should be broadcast");
        assert!(matches!(
            event,
            WorkItemEvent::RunEventAppended {
                event: WorkItemRunEvent { kind: WorkItemRunEventKind::StatusChanged, .. }
            }
        ));
    }

    #[test]
    fn restart_recovery_fails_starting_runs_and_unblocks_card() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let run = handle
            .create_starting_run(&item.id, Some("sess-1"), None, Some("claude"), None, None)
            .unwrap();
        assert_eq!(run.status, WorkItemRunStatus::Starting);
        assert!(handle.has_active_run(&item.id).unwrap());
        let mut rx = handle.subscribe_events();

        let recovered = handle.fail_starting_runs_after_restart().unwrap();

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].id, run.id);
        assert_eq!(recovered[0].status, WorkItemRunStatus::Failed);
        assert!(!handle.has_active_run(&item.id).unwrap());
        assert!(matches!(
            rx.try_recv().expect("RunUpdated should be broadcast"),
            WorkItemEvent::RunUpdated { .. }
        ));
        assert!(matches!(
            rx.try_recv().expect("RunEventAppended should be broadcast"),
            WorkItemEvent::RunEventAppended {
                event: WorkItemRunEvent { kind: WorkItemRunEventKind::StatusChanged, .. }
            }
        ));
    }

    #[test]
    fn decision_events_broadcast() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let run = handle.create_run(&item.id, Some("sess-1"), None, None, None, None).unwrap();
        let mut rx = handle.subscribe_events();

        let decision = handle
            .create_decision(
                &run.id,
                "Choose path?",
                vec![roux_core::WorkItemDecisionOption { value: "go".into(), label: "Go".into() }],
                Some("go"),
                None,
            )
            .unwrap();
        let audit = rx.try_recv().expect("decision audit event should be broadcast");
        assert!(matches!(audit, WorkItemEvent::RunEventAppended { .. }));
        let created = rx.try_recv().expect("DecisionCreated event should be broadcast");
        assert!(matches!(created, WorkItemEvent::DecisionCreated { .. }));

        handle.resolve_decision(&decision.id, "go", Some("user")).unwrap();
        let audit = rx.try_recv().expect("resolution audit event should be broadcast");
        assert!(matches!(audit, WorkItemEvent::RunEventAppended { .. }));
        let resolved = rx.try_recv().expect("DecisionResolved event should be broadcast");
        assert!(matches!(resolved, WorkItemEvent::DecisionResolved { .. }));
    }

    #[test]
    fn decision_timeout_broadcasts_audit_event() {
        let handle = WorkItemHandle::in_memory();
        let item = handle.create(input("Task")).unwrap();
        let run = handle.create_run(&item.id, Some("sess-1"), None, None, None, None).unwrap();
        let decision = handle
            .create_decision(
                &run.id,
                "Choose path?",
                vec![roux_core::WorkItemDecisionOption { value: "go".into(), label: "Go".into() }],
                Some("go"),
                Some(now_secs() + 60),
            )
            .unwrap();
        let mut rx = handle.subscribe_events();

        let timed_out = handle.timeout_decision_to_default(&decision.id).unwrap().unwrap();

        assert_eq!(timed_out.status, roux_core::WorkItemDecisionStatus::TimedOut);
        assert_eq!(timed_out.resolved_value.as_deref(), Some("go"));
        let audit = rx.try_recv().expect("timeout audit event should be broadcast");
        assert!(matches!(
            audit,
            WorkItemEvent::RunEventAppended {
                event: WorkItemRunEvent { kind: WorkItemRunEventKind::DecisionTimedOut, .. }
            }
        ));
        let timed_out = rx.try_recv().expect("DecisionTimedOut event should be broadcast");
        assert!(matches!(timed_out, WorkItemEvent::DecisionTimedOut { .. }));
    }

    #[test]
    fn upsert_by_external_no_duplicate() {
        let handle = WorkItemHandle::in_memory();
        let ext = WorkItemInput {
            title: "First".into(),
            external_ref: Some(ExternalRef {
                provider: "test".into(),
                external_id: "x-1".into(),
                url: None,
            }),
            ..Default::default()
        };
        handle.upsert_by_external(ext.clone()).unwrap();
        let mut ext2 = ext;
        ext2.title = "Updated".into();
        handle.upsert_by_external(ext2).unwrap();

        let items = handle.list(None).unwrap();
        assert_eq!(items.len(), 1, "no duplicate on re-import");
        assert_eq!(items[0].title, "Updated");
    }

    #[test]
    fn create_attachment_can_be_retrieved_by_document_id() {
        let handle = WorkItemHandle::in_memory();
        let attachment = handle
            .create_attachment(AttachmentInput {
                target_kind: AttachmentTargetKind::Session,
                target_id: "session-1".into(),
                title: Some("Plan".into()),
                content_kind: AttachmentContentKind::Text,
                content: "Implement the plan".into(),
                mime_type: Some("text/markdown".into()),
                source_path: None,
            })
            .unwrap();

        assert!(attachment.document_id.starts_with("session-1."));

        let document = handle
            .get_attachment_document(&attachment.document_id)
            .unwrap()
            .expect("attachment should be found by document id");
        assert_eq!(document.attachment.id, attachment.id);
        assert_eq!(document.content, "Implement the plan");
    }

    #[test]
    fn create_attachment_broadcasts_document_attached_event() {
        let handle = WorkItemHandle::in_memory();
        let mut rx = handle.subscribe_events();

        let attachment = handle
            .create_attachment(AttachmentInput {
                target_kind: AttachmentTargetKind::WorkItem,
                target_id: "item-1".into(),
                title: Some("Plan".into()),
                content_kind: AttachmentContentKind::Text,
                content: "Use the plan".into(),
                mime_type: Some("text/markdown".into()),
                source_path: None,
            })
            .unwrap();

        let event = rx.try_recv().expect("DocumentAttached event should be broadcast");
        match event {
            WorkItemEvent::DocumentAttached { attachment: event_attachment } => {
                assert_eq!(event_attachment.id, attachment.id);
                assert_eq!(event_attachment.document_id, attachment.document_id);
            }
            other => panic!("expected DocumentAttached, got {other:?}"),
        }
    }
}
