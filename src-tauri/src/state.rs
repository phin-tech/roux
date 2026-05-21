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

pub(crate) struct AppState {
    pub(crate) settings: Mutex<crate::settings::RouxSettings>,
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
