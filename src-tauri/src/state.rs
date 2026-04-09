use std::sync::Mutex;

use crate::projects::ProjectStore;
use crate::pty::PtyManager;
use crate::session_service::SessionHandle;

pub(crate) struct AppState {
    pub(crate) settings: Mutex<crate::settings::RouxSettings>,
    pub(crate) pty_manager: PtyManager,
    pub(crate) session_handle: SessionHandle,
    pub(crate) project_store: ProjectStore,
    pub(crate) watch_manager: crate::watches::WatchManager,
}
