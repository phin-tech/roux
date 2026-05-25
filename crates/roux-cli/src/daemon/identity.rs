use std::path::PathBuf;

use tokio::sync::watch;

use roux_runtime::alias_service::AliasManager;
use roux_runtime::mailbox_service::MailboxManager;
use roux_runtime::subscription_service::SubscriptionManager;

use crate::{paths, platform};

use super::unix_now_ms;

#[derive(Clone)]
pub(super) struct DaemonIdentity {
    pub(super) started_at_ms: u64,
    pub(super) socket: PathBuf,
    pub(super) log_path: PathBuf,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) auth_token: Option<String>,
    pub(super) endpoint: platform::SocketEndpoint,
    pub(super) alias_manager: AliasManager,
    pub(super) subscription_manager: SubscriptionManager,
    pub(super) mailbox_manager: MailboxManager,
    pub(super) shutdown_tx: Option<watch::Sender<bool>>,
}

impl DaemonIdentity {
    pub(super) fn new(
        endpoint: platform::SocketEndpoint,
        log_path: PathBuf,
        auth_token: Option<String>,
    ) -> Self {
        let subscription_manager =
            SubscriptionManager::load_from(paths::roux_config_dir().join("subscriptions.json"));
        let mailbox_manager = MailboxManager::load_from(
            paths::roux_config_dir().join("events.jsonl"),
            paths::roux_config_dir().join("read_state.json"),
        )
        .with_subscriptions(subscription_manager.clone());
        Self {
            started_at_ms: unix_now_ms(),
            socket: endpoint_path(&endpoint),
            log_path,
            auth_token,
            endpoint,
            alias_manager: AliasManager::load_from(paths::roux_config_dir().join("aliases.json")),
            subscription_manager,
            mailbox_manager,
            shutdown_tx: None,
        }
    }

    pub(super) fn with_shutdown(mut self, shutdown_tx: watch::Sender<bool>) -> Self {
        self.shutdown_tx = Some(shutdown_tx);
        self
    }

    pub(super) fn endpoint_display(&self) -> String {
        self.endpoint.display_value()
    }

    pub(super) fn request_shutdown(&self) -> bool {
        self.shutdown_tx.as_ref().map(|tx| tx.send(true).is_ok()).unwrap_or(false)
    }

    #[cfg(test)]
    pub(super) fn new_for_test(socket: impl Into<PathBuf>) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::in_memory(),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    pub(super) fn new_for_test_with_endpoint(
        endpoint: platform::SocketEndpoint,
        auth_token: Option<String>,
    ) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        Self {
            started_at_ms: 1_000,
            socket: endpoint_path(&endpoint),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token,
            endpoint,
            alias_manager: AliasManager::in_memory(),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    pub(super) fn new_for_test_with_alias_path(
        socket: impl Into<PathBuf>,
        alias_path: PathBuf,
    ) -> Self {
        let subscription_manager = SubscriptionManager::in_memory();
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::load_from(alias_path),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::in_memory().with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }

    #[cfg(test)]
    pub(super) fn new_for_test_with_runtime_paths(
        socket: impl Into<PathBuf>,
        alias_path: PathBuf,
        subscription_path: PathBuf,
        mailbox_events_path: PathBuf,
        mailbox_read_state_path: PathBuf,
    ) -> Self {
        let subscription_manager = SubscriptionManager::load_from(subscription_path);
        let socket = socket.into();
        Self {
            started_at_ms: 1_000,
            socket: socket.clone(),
            log_path: PathBuf::from("/tmp/roux-daemon.log"),
            auth_token: None,
            endpoint: platform::SocketEndpoint::Unix(socket),
            alias_manager: AliasManager::load_from(alias_path),
            subscription_manager: subscription_manager.clone(),
            mailbox_manager: MailboxManager::load_from(
                mailbox_events_path,
                mailbox_read_state_path,
            )
            .with_subscriptions(subscription_manager),
            shutdown_tx: None,
        }
    }
}

pub(super) fn endpoint_path(endpoint: &platform::SocketEndpoint) -> PathBuf {
    match endpoint {
        platform::SocketEndpoint::Unix(path) => path.clone(),
        platform::SocketEndpoint::Tcp(_) => PathBuf::from(endpoint.display_value()),
    }
}

pub(super) fn request_authorized(
    req: &super::protocol::Request,
    identity: &DaemonIdentity,
) -> bool {
    match identity.auth_token.as_deref() {
        Some(expected) if !expected.is_empty() => req.auth_token.as_deref() == Some(expected),
        _ => true,
    }
}
