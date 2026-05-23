//! Tauri-free runtime services for Roux.
//!
//! This crate hosts stateful backend services that can be embedded by the
//! current Tauri app and, later, by a standalone daemon.

pub mod alias_persistence;
pub mod alias_service;
pub mod alias_store;
pub mod automation_hooks;
pub mod host;
pub mod mailbox_persistence;
pub mod mailbox_service;
pub mod mailbox_store;
pub mod notes_service;
pub mod pane_service;
pub mod process;
pub mod process_service;
pub mod project_service;
pub mod pty_lifecycle;
pub mod pty_live;
pub mod pty_output;
pub mod pty_pending_output;
pub mod pty_ready_gate;
pub mod pty_registry;
pub mod pty_service;
pub mod pty_session;
pub mod pty_spawn;
pub mod session_service;
pub mod subscription_persistence;
pub mod subscription_service;
pub mod subscription_store;
pub mod terminal_env;
pub mod watch_checks;
pub mod watch_runner;
pub mod watch_service;
