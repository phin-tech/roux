use thiserror::Error;

#[derive(Debug, Error)]
pub enum WtError {
    #[error("failed to run `wt`: {source}")]
    Spawn {
        #[source]
        source: std::io::Error,
    },
    #[error("`wt` exited with status {status}: {stderr}")]
    NonZeroExit { status: i32, stderr: String },
    #[error("failed to parse `wt` JSON output: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
    /// `wt` refused to remove a worktree because it is locked. Callers
    /// that want to proceed anyway must pass `force = true` explicitly.
    /// The GUI never does this silently — issue #101 "GUI cleanup
    /// defaults must be more conservative than terminal cleanup".
    #[error("worktree is locked: {reason}")]
    Locked { reason: String },
    /// The caller asked to operate on a path that `wt` does not
    /// consider a worktree (not in `wt list`).
    #[error("no worktree registered at {path}")]
    NotFound { path: String },
}
