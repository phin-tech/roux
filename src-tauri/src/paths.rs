//! Canonical filesystem locations for Roux state.
//!
//! Every module that persists data (settings, sessions, projects, pane
//! state, logs, notes, watches, task overrides, socket, status, roux-cli
//! shim) routes through `roux_config_dir()`, so there is one place to
//! change when the layout moves.
//!
//! History: sessions / settings / logs originally lived under
//! `dirs::config_dir()` which is `~/Library/Application Support/roux` on
//! macOS, but the hook bridge, socket, and roux-cli shim already lived at
//! `~/.config/roux`. That split meant users had two config roots and
//! plenty of ways to look at the wrong one. We now unify on
//! `~/.config/roux` on every platform, with a one-time best-effort
//! migration from the legacy macOS location handled by
//! [`migrate_legacy_config_dir`].

use std::path::{Path, PathBuf};

/// Root directory for all Roux state: `~/.config/roux`.
///
/// Callers append their own subpath, e.g. `roux_config_dir().join("settings.json")`
/// or `roux_config_dir().join("logs").join("roux.log")`.
pub fn roux_config_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("roux")
}

/// Default on-disk location for the Obsidian-compatible notes vault.
///
/// `~/Documents/Roux` on every platform. Users override this via the
/// `notes.vaultRoot` setting (wired in Step 3). The helper always returns
/// an absolute path; callers handle creation lazily on first write.
pub fn default_notes_vault_root() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents").join("Roux")
}

/// Legacy config directory, if it differs from the current one.
///
/// Returns `None` when `dirs::config_dir().join("roux")` is exactly the
/// same path as `roux_config_dir()` (Linux, where `dirs::config_dir()` is
/// `~/.config`). Returns `Some(~/Library/Application Support/roux)` on
/// macOS, which is where settings / sessions / logs lived before the
/// `~/.config/roux` unification.
pub fn legacy_config_dir() -> Option<PathBuf> {
    let legacy = dirs::config_dir()?.join("roux");
    if legacy == roux_config_dir() {
        None
    } else {
        Some(legacy)
    }
}

/// One-time best-effort copy of state from the legacy location to
/// `~/.config/roux`.
///
/// Called from `main()` before any module touches the filesystem so the
/// new location is populated before settings / sessions / logs load.
/// Idempotent by design: files that already exist at the destination are
/// left alone, so re-running the migration is a no-op and any edits made
/// at the new location after the first run are preserved. Any failure
/// (permission denied, disk full, etc.) is logged to stderr and
/// swallowed — a broken migration must not prevent startup.
pub fn migrate_legacy_config_dir() {
    let Some(legacy) = legacy_config_dir() else {
        return;
    };
    if !legacy.exists() {
        return;
    }
    let new = roux_config_dir();
    if let Err(e) = std::fs::create_dir_all(&new) {
        eprintln!("roux: failed to create {new:?}: {e}");
        return;
    }
    match copy_dir_skip_existing(&legacy, &new) {
        Ok(count) if count > 0 => {
            eprintln!(
                "roux: migrated {count} file(s) from {} to {}",
                legacy.display(),
                new.display(),
            );
        }
        Ok(_) => {
            // Legacy dir exists but everything already lives at the new
            // location — the common case after the first migration.
        }
        Err(e) => {
            eprintln!("roux: migration from {} to {} failed: {e}", legacy.display(), new.display(),)
        }
    }
}

/// Recursively copy `src` into `dst`, skipping any file that already
/// exists at the destination path. Returns the number of files copied.
///
/// Symlinks are explicitly refused with `symlink_metadata` before we ever
/// look at `is_file` / `is_dir`. Roux never writes symlinks to its state
/// dir, so anything symlinked in the legacy tree is either stale, planted,
/// or a sign the user knows what they're doing — in all three cases we
/// leave the link alone rather than copying through it into the new
/// `~/.config/roux` tree (a `settings.json -> /etc/passwd` symlink would
/// otherwise leak contents out of the source).
fn copy_dir_skip_existing(src: &Path, dst: &Path) -> std::io::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // `symlink_metadata` never traverses the final component, so a
        // symlink returns the symlink's own file type. Using this instead
        // of `entry.file_type()` makes the symlink guard unambiguous even
        // on platforms where `DirEntry::file_type` quietly follows links.
        let meta = std::fs::symlink_metadata(&src_path)?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            eprintln!(
                "roux: skipping symlink {} during legacy config migration",
                src_path.display(),
            );
            continue;
        }

        if ft.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copied += copy_dir_skip_existing(&src_path, &dst_path)?;
        } else if ft.is_file() {
            if dst_path.exists() {
                continue;
            }
            std::fs::copy(&src_path, &dst_path)?;
            copied += 1;
        }
        // Other file types (sockets, fifos, block/char devices) are
        // ignored — Roux never writes them to its state dir.
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn roux_config_dir_is_under_dotconfig_roux() {
        let dir = roux_config_dir();
        // The last two components are always `.config` then `roux`
        // regardless of whose $HOME we're running under.
        let tail: Vec<_> = dir
            .components()
            .rev()
            .take(2)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(tail, vec!["roux".to_string(), ".config".to_string()]);
    }

    #[test]
    fn copy_dir_skip_existing_copies_nested_files() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        fs::write(src.path().join("top.json"), "{}").unwrap();
        fs::create_dir_all(src.path().join("nested")).unwrap();
        fs::write(src.path().join("nested/inner.txt"), "x").unwrap();

        let count = copy_dir_skip_existing(src.path(), dst.path()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(fs::read_to_string(dst.path().join("top.json")).unwrap(), "{}");
        assert_eq!(fs::read_to_string(dst.path().join("nested/inner.txt")).unwrap(), "x",);
    }

    #[test]
    fn copy_dir_skip_existing_leaves_destination_files_alone() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        fs::write(src.path().join("settings.json"), "{\"from\":\"legacy\"}").unwrap();
        fs::write(dst.path().join("settings.json"), "{\"from\":\"new\"}").unwrap();

        let count = copy_dir_skip_existing(src.path(), dst.path()).unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            fs::read_to_string(dst.path().join("settings.json")).unwrap(),
            "{\"from\":\"new\"}",
        );
    }

    #[test]
    fn copy_dir_skip_existing_is_idempotent() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();

        fs::write(src.path().join("a.json"), "1").unwrap();
        let first = copy_dir_skip_existing(src.path(), dst.path()).unwrap();
        let second = copy_dir_skip_existing(src.path(), dst.path()).unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_skip_existing_rejects_symlinked_files() {
        // A malicious or buggy legacy tree could contain a symlink named
        // like a real state file (`settings.json -> /etc/passwd`). We must
        // never follow it into the new `~/.config/roux` tree.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        fs::write(outside.path().join("secret.txt"), "leaked").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            src.path().join("settings.json"),
        )
        .unwrap();
        fs::write(src.path().join("normal.json"), "ok").unwrap();

        let count = copy_dir_skip_existing(src.path(), dst.path()).unwrap();

        assert_eq!(count, 1, "only the non-symlink file should be copied");
        assert!(!dst.path().join("settings.json").exists());
        assert_eq!(fs::read_to_string(dst.path().join("normal.json")).unwrap(), "ok");
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_skip_existing_rejects_symlinked_directories() {
        // A symlink pointing a whole directory outside the source tree
        // (`logs -> /var/log`) would otherwise cause `read_dir` recursion
        // to copy files the user never placed in the legacy config dir.
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        fs::write(outside.path().join("leaked.log"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), src.path().join("logs")).unwrap();

        let count = copy_dir_skip_existing(src.path(), dst.path()).unwrap();

        assert_eq!(count, 0);
        assert!(!dst.path().join("logs").exists());
    }
}
