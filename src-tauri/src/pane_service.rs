use std::collections::HashMap;

use roux_core::SpawnProfile;
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaneDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub pane_type: String,
    pub pty_id: String,
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub command: Option<String>,
    pub doc_path: Option<String>,
    pub spawn_profile_ref: Option<PaneSpawnProfileRef>,
    pub provider: Option<String>,
    pub provider_session_id: Option<String>,
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Option<Vec<String>>,
    pub notes_scope: Option<String>,
    pub notes_view_mode: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaneRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub pane_type: String,
    pub pty_id: String,
    pub name: Option<String>,
    pub working_dir: Option<String>,
    pub command: Option<String>,
    pub doc_path: Option<String>,
    pub spawn_profile_ref: Option<PaneSpawnProfileRef>,
    pub provider: Option<String>,
    pub provider_session_id: Option<String>,
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Option<Vec<String>>,
    pub notes_scope: Option<String>,
    pub notes_view_mode: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PaneSpawnProfileRef {
    Registered { id: String },
    Inline { profile: Box<SpawnProfile> },
}

impl PaneRecord {
    pub fn descriptor_with_working_dir(&self, working_dir: Option<String>) -> PaneDescriptor {
        PaneDescriptor {
            id: self.id.clone(),
            pane_type: self.pane_type.clone(),
            pty_id: self.pty_id.clone(),
            name: self.name.clone(),
            working_dir: working_dir.or_else(|| self.working_dir.clone()),
            command: self.command.clone(),
            doc_path: self.doc_path.clone(),
            spawn_profile_ref: self.spawn_profile_ref.clone(),
            provider: self.provider.clone(),
            provider_session_id: self.provider_session_id.clone(),
            nono_profile: self.nono_profile.clone(),
            nono_allow_dirs: self.nono_allow_dirs.clone(),
            notes_scope: self.notes_scope.clone(),
            notes_view_mode: self.notes_view_mode.clone(),
            session_id: self.session_id.clone(),
        }
    }
}

enum PaneMsg {
    Upsert {
        record: Box<PaneRecord>,
        reply: oneshot::Sender<()>,
    },
    Remove {
        id: String,
        reply: oneshot::Sender<()>,
    },
    ListByIds {
        ids: Vec<String>,
        reply: oneshot::Sender<Vec<PaneRecord>>,
    },
    ListBySession {
        session_id: String,
        reply: oneshot::Sender<Vec<PaneRecord>>,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("pane service unavailable")]
pub struct ServiceError;

#[derive(Clone)]
pub struct PaneHandle {
    tx: mpsc::UnboundedSender<PaneMsg>,
}

impl PaneHandle {
    fn send(&self, msg: PaneMsg) -> Result<(), ServiceError> {
        self.tx.send(msg).map_err(|_| ServiceError)
    }

    pub async fn upsert(&self, record: PaneRecord) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PaneMsg::Upsert { record: Box::new(record), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PaneMsg::Remove { id: id.to_string(), reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    pub async fn list_by_ids(&self, ids: Vec<String>) -> Result<Vec<PaneRecord>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PaneMsg::ListByIds { ids, reply: reply_tx })?;
        reply_rx.await.map_err(|_| ServiceError)
    }

    /// Return all panes whose id starts with `{session_id}-`.
    pub async fn list_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<PaneRecord>, ServiceError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(PaneMsg::ListBySession {
            session_id: session_id.to_string(),
            reply: reply_tx,
        })?;
        reply_rx.await.map_err(|_| ServiceError)
    }
}

pub fn spawn() -> (PaneHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let join = tauri::async_runtime::spawn(service_loop(rx));
    (PaneHandle { tx }, join)
}

async fn service_loop(mut rx: mpsc::UnboundedReceiver<PaneMsg>) {
    let mut panes: HashMap<String, PaneRecord> = HashMap::new();

    while let Some(msg) = rx.recv().await {
        match msg {
            PaneMsg::Upsert { record, reply } => {
                panes.insert(record.id.clone(), *record);
                let _ = reply.send(());
            }
            PaneMsg::Remove { id, reply } => {
                panes.remove(&id);
                let _ = reply.send(());
            }
            PaneMsg::ListByIds { ids, reply } => {
                let records = ids
                    .into_iter()
                    .filter_map(|id| panes.get(&id).cloned())
                    .collect();
                let _ = reply.send(records);
            }
            PaneMsg::ListBySession { session_id, reply } => {
                let prefix = format!("{}-", session_id);
                let records = panes
                    .values()
                    .filter(|r| {
                        // Prefer the explicit session_id field when present (populated by
                        // the frontend for both socket-created and frontend-created panes);
                        // fall back to the legacy id-prefix heuristic for older records
                        // that predate this field.
                        r.session_id.as_deref() == Some(&session_id)
                            || (r.session_id.is_none() && r.id.starts_with(&prefix))
                    })
                    .cloned()
                    .collect();
                let _ = reply.send(records);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, pty_id: &str) -> PaneRecord {
        PaneRecord {
            id: id.into(),
            pane_type: "shell".into(),
            pty_id: pty_id.into(),
            name: None,
            working_dir: None,
            command: None,
            doc_path: None,
            spawn_profile_ref: None,
            provider: None,
            provider_session_id: None,
            nono_profile: None,
            nono_allow_dirs: None,
            notes_scope: None,
            notes_view_mode: None,
            session_id: None,
        }
    }

    #[tokio::test]
    async fn upsert_and_list_by_ids_round_trip() {
        let (handle, _join) = spawn();
        handle.upsert(pane("p1", "pty-1")).await.unwrap();
        handle.upsert(pane("p2", "pty-2")).await.unwrap();

        let listed = handle.list_by_ids(vec!["p2".into(), "p1".into()]).await.unwrap();
        assert_eq!(listed.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["p2", "p1"]);
    }

    #[tokio::test]
    async fn remove_drops_record() {
        let (handle, _join) = spawn();
        handle.upsert(pane("p1", "pty-1")).await.unwrap();
        handle.remove("p1").await.unwrap();

        let listed = handle.list_by_ids(vec!["p1".into()]).await.unwrap();
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn later_upsert_replaces_existing_record() {
        let (handle, _join) = spawn();
        handle.upsert(pane("p1", "pty-1")).await.unwrap();
        let mut updated = pane("p1", "pty-2");
        updated.name = Some("Main".into());
        handle.upsert(updated.clone()).await.unwrap();

        let listed = handle.list_by_ids(vec!["p1".into()]).await.unwrap();
        assert_eq!(listed, vec![updated]);
    }
}
