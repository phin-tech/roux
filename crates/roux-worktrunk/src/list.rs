use std::path::Path;
use std::process::Command;

use crate::detect::WtBinary;
use crate::exec::WtError;
use crate::schema::WtItem;

/// Run `wt list --format=json` in `repo_path` and parse the result.
pub fn list_worktrees(wt: &WtBinary, repo_path: &Path) -> Result<Vec<WtItem>, WtError> {
    let output = Command::new(&wt.path)
        .args(["list", "--format=json"])
        .current_dir(repo_path)
        .output()
        .map_err(|source| WtError::Spawn { source })?;

    if !output.status.success() {
        return Err(WtError::NonZeroExit {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    serde_json::from_slice::<Vec<WtItem>>(&output.stdout).map_err(|source| WtError::Parse { source })
}
