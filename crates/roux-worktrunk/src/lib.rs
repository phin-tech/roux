//! Shell-out wrapper around the worktrunk (`wt`) CLI.
//!
//! Roux integrates with whatever `wt` the user has on PATH rather than
//! linking the `worktrunk` crate. This keeps the integration opt-in,
//! honors the user's installed CLI version + `.config/wt.toml`, and lets
//! users without `wt` installed fall through to the native git path in
//! `roux_core::worktree` with no regression.

pub mod create;
pub mod detect;
pub mod diagnostics;
pub mod exec;
pub mod list;
pub mod remove;
pub mod schema;

pub use create::{create_worktree, CreateOpts};
pub use detect::{detect_wt, detect_wt_config, WtBinary, MIN_WT_VERSION};
pub use diagnostics::{
    extract_hook_defs, list_logs, read_log_file, show_config, WtConfigFile, WtConfigShow,
    WtHookDef, WtHookOutputEntry, WtLogEntry, WtLogs,
};
pub use exec::WtError;
pub use list::list_worktrees;
pub use remove::{remove_worktree, RemoveOpts};
pub use schema::WtItem;
