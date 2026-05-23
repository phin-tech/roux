use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

use roux_lib::aliases::AliasManager;
use roux_lib::mailbox::MailboxManager;
use roux_lib::subscriptions::SubscriptionManager;
use roux_runtime::host::RuntimeHost;

use crate::notifications::NotificationManager;
use crate::pty::PtyManager;

/// Correlation map for socket-driven request/response round-trips with the
/// frontend. Used by `session-panes-list` / `session-panes-create` where the
/// reply data lives in Svelte stores.
pub(crate) type PendingReplies = Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>;

pub(crate) struct DaemonPtyAttachTask {
    pub(crate) token: u64,
    pub(crate) handle: tauri::async_runtime::JoinHandle<()>,
}

pub(crate) struct AppState {
    pub(crate) settings: Mutex<crate::settings::RouxSettings>,
    pub(crate) daemon_client: Option<crate::daemon_client::DaemonClient>,
    pub(crate) daemon_pty_attach_tasks: Mutex<HashMap<String, DaemonPtyAttachTask>>,
    pub(crate) pty_manager: Arc<PtyManager>,
    pub(crate) runtime: RuntimeHost,
    pub(crate) watch_manager: crate::watches::WatchManager,
    pub(crate) automation_hooks: crate::automation_hooks::AutomationHookManager,
    pub(crate) notification_manager: NotificationManager,
    pub(crate) alias_manager: AliasManager,
    pub(crate) mailbox_manager: MailboxManager,
    pub(crate) subscription_manager: SubscriptionManager,
    pub(crate) pending_replies: PendingReplies,
    pub(crate) managed_proxy: Arc<crate::services::managed_proxy::ManagedProxyState>,
}

pub(crate) fn required_daemon_client(
    state: &AppState,
) -> Result<crate::daemon_client::DaemonClient, String> {
    state
        .daemon_client
        .clone()
        .ok_or_else(|| "Roux daemon is required but not connected".to_string())
}

pub(crate) fn required_daemon_client_ref(
    state: &AppState,
) -> Result<&crate::daemon_client::DaemonClient, String> {
    state
        .daemon_client
        .as_ref()
        .ok_or_else(|| "Roux daemon is required but not connected".to_string())
}
