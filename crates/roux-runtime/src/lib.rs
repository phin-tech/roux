//! Tauri-free runtime services for Roux.
//!
//! This crate hosts stateful backend services that can be embedded by the
//! current Tauri app and, later, by a standalone daemon.

pub mod pane_service;
pub mod process;
pub mod project_service;
pub mod pty_lifecycle;
pub mod pty_output;
pub mod pty_pending_output;
pub mod pty_ready_gate;
pub mod pty_registry;
pub mod pty_session;
pub mod pty_spawn;
pub mod session_service;
pub mod terminal_env;
