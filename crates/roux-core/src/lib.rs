//! Shared types and models for Roux.
//!
//! This crate contains the data types that cross the IPC boundary between
//! the Rust backend and the TypeScript frontend. It has no dependency on
//! Tauri, so it can be used by the CLI, tests, and future tooling.

pub mod models;
pub mod worktree;

pub use models::*;
pub use worktree::{create_worktree, remove_worktree, list_worktrees, WorktreeError};
