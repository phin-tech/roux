use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;

use crate::notifications::NotificationManager;
use crate::pane_service::PaneHandle;
use crate::project_service::ProjectHandle;
use crate::pty::PtyManager;
use crate::session_service::SessionHandle;

/// Correlation map for socket-driven request/response round-trips with the
/// frontend. Used by `session-panes-list` / `session-panes-create` where the
/// reply data lives in Svelte stores.
pub(crate) type PendingReplies = Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>;

pub(crate) struct AppState {
    pub(crate) settings: Mutex<crate::settings::RouxSettings>,
    pub(crate) pty_manager: PtyManager,
    pub(crate) pane_handle: PaneHandle,
    pub(crate) session_handle: SessionHandle,
    pub(crate) project_handle: ProjectHandle,
    pub(crate) watch_manager: crate::watches::WatchManager,
    pub(crate) notification_manager: NotificationManager,
    pub(crate) pending_replies: PendingReplies,
}
