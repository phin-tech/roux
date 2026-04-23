//! Mirror types for `wt list --format=json` output.
//!
//! Every field is `#[serde(default)]` and unknown fields are ignored, so a
//! newer `wt` that adds fields will not break parsing. See
//! `worktrunk/src/commands/list/json_output.rs` for the source schema.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtItem {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub commit: Option<WtCommit>,
    #[serde(default)]
    pub working_tree: Option<WtWorkingTree>,
    #[serde(default)]
    pub main_state: Option<String>,
    #[serde(default)]
    pub operation_state: Option<String>,
    #[serde(default)]
    pub main: Option<WtMain>,
    #[serde(default)]
    pub remote: Option<WtRemote>,
    #[serde(default)]
    pub worktree: Option<WtWorktreeState>,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub is_previous: bool,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub url_active: Option<bool>,
    #[serde(default)]
    pub statusline: Option<String>,
    #[serde(default)]
    pub symbols: Option<String>,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// CI status summary (PR check runs or branch workflow) — absent when
    /// wt hasn't fetched CI state or there is no applicable workflow.
    #[serde(default)]
    pub ci: Option<WtCi>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtCi {
    /// "passed" | "running" | "failed" | "conflicts" | "no-ci" | "error"
    #[serde(default)]
    pub status: String,
    /// "pr" | "branch" — which source surfaced the status.
    #[serde(default)]
    pub source: String,
    /// True when the local HEAD differs from the remote HEAD (unpushed
    /// changes), so the CI status is for an older commit.
    #[serde(default)]
    pub stale: bool,
    /// Link to the PR or workflow run when wt has one.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtCommit {
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub short_sha: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub timestamp: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtWorkingTree {
    #[serde(default)]
    pub staged: bool,
    #[serde(default)]
    pub modified: bool,
    #[serde(default)]
    pub untracked: bool,
    #[serde(default)]
    pub renamed: bool,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtMain {
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtRemote {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtWorktreeState {
    /// One of "branch_worktree_mismatch", "prunable", "locked", "no_worktree"
    /// when `wt` has populated it; `None` is normal.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub detached: bool,
}

impl WtItem {
    pub fn is_dirty(&self) -> bool {
        match &self.working_tree {
            Some(wt) => wt.staged || wt.modified || wt.untracked || wt.renamed || wt.deleted,
            None => false,
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(
            self.worktree.as_ref().and_then(|w| w.state.as_deref()),
            Some("locked")
        )
    }

    pub fn is_prunable(&self) -> bool {
        matches!(
            self.worktree.as_ref().and_then(|w| w.state.as_deref()),
            Some("prunable")
        )
    }

    /// The `reason` string from `wt list` when the worktree is locked.
    /// `None` means either not locked or locked without a recorded reason.
    pub fn lock_reason(&self) -> Option<&str> {
        if self.is_locked() {
            self.worktree.as_ref().and_then(|w| w.reason.as_deref())
        } else {
            None
        }
    }

    /// The `reason` string from `wt list` when the worktree is prunable.
    pub fn prunable_reason(&self) -> Option<&str> {
        if self.is_prunable() {
            self.worktree.as_ref().and_then(|w| w.reason.as_deref())
        } else {
            None
        }
    }

    /// Back-compat helper used by integration tests: `Some(reason)` when
    /// locked (empty string if locked without a reason), `None` otherwise.
    pub fn locked_reason(&self) -> Option<&str> {
        if self.is_locked() {
            Some(self.worktree.as_ref().and_then(|w| w.reason.as_deref()).unwrap_or(""))
        } else {
            None
        }
    }

    pub fn ahead(&self) -> u32 {
        self.main.as_ref().map(|m| m.ahead).unwrap_or(0)
    }

    pub fn behind(&self) -> u32 {
        self.main.as_ref().map(|m| m.behind).unwrap_or(0)
    }
}
