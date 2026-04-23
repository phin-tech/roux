use std::path::Path;
use std::process::Command;

use crate::detect::WtBinary;
use crate::exec::WtError;
use crate::schema::WtItem;

/// Run `wt list --full --format=json` in `repo_path` and parse the result.
///
/// `--full` is required for the `ci` field to populate; without it `wt`
/// omits CI status and URL, which breaks the StatusBar PR link and the
/// CI chips in session cards. Diffstats also come along for free.
pub fn list_worktrees(wt: &WtBinary, repo_path: &Path) -> Result<Vec<WtItem>, WtError> {
    let output = Command::new(&wt.path)
        .args(["list", "--full", "--format=json"])
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
