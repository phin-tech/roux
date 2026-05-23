pub(crate) mod agent_notifications;
pub(crate) mod docs;
pub(crate) mod library;
pub(crate) mod library_sync;
pub(crate) mod managed_proxy;
pub(crate) mod mcp_config;
pub(crate) mod notes;
pub(crate) mod projects;
// Local session service remains for tests and staged daemon migration cleanup.
#[allow(dead_code)]
pub(crate) mod sessions;
pub(crate) mod settings;
pub(crate) mod setup;
pub(crate) mod smolvm;
pub(crate) mod worktrees;
