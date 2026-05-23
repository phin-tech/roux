use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub is_main: bool,
    /// Optional metadata sourced from `wt list --format=json` when the
    /// worktrunk CLI is available. `null` means either `wt` is not
    /// installed or the current command did not attempt enrichment.
    pub worktrunk: Option<WorktrunkMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktrunkMetadata {
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub prunable: bool,
    pub prunable_reason: Option<String>,
    pub is_current: bool,
    pub is_previous: bool,
    pub dev_server_url: Option<String>,
    /// Branch's relationship to the default branch as reported by wt.
    /// Common values: "is_main" (same as main), "integrated" (merged),
    /// "diverged", "ahead", "behind", "same_commit", "would_conflict".
    /// Absent for entries wt can't classify.
    pub main_state: Option<String>,
    /// CI status summary — "passed" | "failed" | "running" | "conflicts"
    /// | "no-ci" | "error". `null` when wt has no CI data for the branch.
    pub ci_status: Option<String>,
    /// Link to the PR or workflow run when wt surfaced one.
    pub ci_url: Option<String>,
    /// True when the CI status is for an older commit than local HEAD
    /// (there are unpushed changes).
    pub ci_stale: bool,
}
