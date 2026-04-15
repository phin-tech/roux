use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use crate::paths::roux_config_dir;
pub use roux_core::RouxSettings;
use roux_core::SettingsKdlError;

const SETTINGS_FILENAME: &str = "settings.kdl";
const LEGACY_JSON_FILENAME: &str = "settings.json";

fn settings_path() -> PathBuf {
    roux_config_dir().join(SETTINGS_FILENAME)
}

fn legacy_json_path() -> PathBuf {
    roux_config_dir().join(LEGACY_JSON_FILENAME)
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("{source}")]
    CreateSettingsDir {
        #[source]
        source: std::io::Error,
    },
    #[error("{source}")]
    KdlSerialize {
        #[source]
        source: SettingsKdlError,
    },
    /// Failed to read the existing settings.kdl during a save. Surfaced
    /// rather than silently treating the file as empty so we never
    /// overwrite an unreadable (e.g. transient IO error, non-UTF8) file
    /// with the default scaffold and lose the user's content/comments.
    #[error("read existing settings: {source}")]
    Read {
        #[source]
        source: std::io::Error,
    },
    #[error("{source}")]
    Write {
        #[source]
        source: std::io::Error,
    },
}

pub fn load_settings() -> RouxSettings {
    load_settings_at(&settings_path(), &legacy_json_path())
}

pub fn save_settings(settings: &RouxSettings) -> Result<(), SettingsError> {
    save_settings_at(&settings_path(), settings)
}

/// Test-friendly variant of [`load_settings`] that takes explicit paths.
///
/// Resolution order:
/// 1. If the KDL file exists, parse it. On parse error, log a diagnostic
///    and fall back to defaults — leave the broken file untouched so the
///    user can inspect or recover it.
/// 2. Otherwise, if the legacy JSON file exists, deserialize it, write a
///    KDL equivalent, and rename the JSON to `<name>.bak` (uniquified if a
///    prior `.bak` exists) for rollback safety. If the JSON read or parse
///    fails, log a diagnostic and return defaults *without* writing KDL or
///    renaming the JSON, so the user can fix the file and retry on the
///    next launch.
/// 3. Otherwise, return [`RouxSettings::default`] without writing anything.
///    The first `save_settings` call will create the file from the seed
///    template.
///
/// Always runs [`RouxSettings::normalized`] before returning so callers
/// see the canonical form regardless of which branch produced the value.
pub fn load_settings_at(kdl_path: &Path, json_path: &Path) -> RouxSettings {
    if kdl_path.exists() {
        return load_kdl_or_default(kdl_path);
    }
    if json_path.exists() {
        return migrate_from_legacy_json(json_path, kdl_path);
    }
    // Defaults are already normalized by construction, but call out to
    // honor the documented contract — cheap and keeps the invariant true.
    RouxSettings::default().normalized()
}

fn load_kdl_or_default(kdl_path: &Path) -> RouxSettings {
    match fs::read_to_string(kdl_path) {
        Ok(content) => match roux_core::parse_settings_kdl(&content) {
            Ok(parsed) => parsed.normalized(),
            Err(e) => {
                eprintln!(
                    "roux: failed to parse {} ({}); falling back to defaults. The file is left in place for inspection.",
                    kdl_path.display(),
                    e,
                );
                RouxSettings::default()
            }
        },
        Err(e) => {
            eprintln!("roux: failed to read {}: {}; using defaults", kdl_path.display(), e);
            RouxSettings::default()
        }
    }
}

fn migrate_from_legacy_json(json_path: &Path, kdl_path: &Path) -> RouxSettings {
    // Don't `unwrap_or_default` either step: if the JSON is unreadable or
    // malformed, migrating *defaults* and renaming the user's JSON to
    // `.bak` would prevent them from fixing the file and retrying. Treat
    // these failures the same as a corrupt KDL file: log, return defaults,
    // leave everything on disk untouched.
    let content = match fs::read_to_string(json_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "roux: failed to read legacy {}: {}; leaving the file in place and using defaults",
                json_path.display(),
                e,
            );
            return RouxSettings::default().normalized();
        }
    };
    let parsed: RouxSettings = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "roux: failed to parse legacy {} ({}); leaving the file in place and using defaults",
                json_path.display(),
                e,
            );
            return RouxSettings::default().normalized();
        }
    };
    let normalized = parsed.normalized();

    // Best-effort write the new KDL alongside the old JSON. If the write
    // fails (permissions, disk full, …) we keep the JSON in place so the
    // next launch tries again, and we still return the parsed settings to
    // the caller — a one-time-migration that fails to persist must never
    // wedge the app.
    match save_settings_at(kdl_path, &normalized) {
        Ok(()) => {
            if let Err(e) = rename_to_uniquified_bak(json_path) {
                eprintln!(
                    "roux: migrated {} to {} but failed to back up the JSON ({}); leaving it in place",
                    json_path.display(),
                    kdl_path.display(),
                    e,
                );
            } else {
                eprintln!(
                    "roux: migrated settings from {} to {} (legacy file renamed to .bak)",
                    json_path.display(),
                    kdl_path.display(),
                );
            }
        }
        Err(e) => {
            eprintln!(
                "roux: failed to write migrated {}: {}; legacy {} kept in place for retry",
                kdl_path.display(),
                e,
                json_path.display(),
            );
        }
    }

    normalized
}

/// Rename `path` to `path` with `.bak` appended. If that name already
/// exists (a previous failed-and-recovered migration left a file behind),
/// append a numeric suffix so we never clobber an existing backup.
fn rename_to_uniquified_bak(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("settings.json");
    let mut candidate = parent.join(format!("{file_name}.bak"));
    let mut n = 1u32;
    while candidate.exists() {
        candidate = parent.join(format!("{file_name}.bak.{n}"));
        n = n.saturating_add(1);
        if n > 1000 {
            // Defensive: refuse to spin forever if the directory is somehow
            // packed with .bak.N files. The caller treats Err as "leave the
            // JSON in place" which is the safe fallback.
            return Err(std::io::Error::other("too many existing .bak files"));
        }
    }
    fs::rename(path, &candidate)?;
    Ok(candidate)
}

/// Test-friendly variant of [`save_settings`].
///
/// Reads any existing KDL document at `kdl_path`, splices `settings` into
/// it (preserving comments and unrelated nodes), and writes the result via
/// a tmp-file + rename so a crash mid-write cannot leave a half-written
/// settings file.
pub fn save_settings_at(kdl_path: &Path, settings: &RouxSettings) -> Result<(), SettingsError> {
    if let Some(parent) = kdl_path.parent() {
        fs::create_dir_all(parent).map_err(|source| SettingsError::CreateSettingsDir { source })?;
    }

    let normalized = settings.clone().normalized();

    let existing = if kdl_path.exists() {
        // Surface read errors rather than treating them as an empty
        // document. If the existing file is unreadable (transient IO
        // error, non-UTF8) and we silently fell back to the default
        // scaffold, the next write would overwrite the user's content
        // and comments with defaults — exactly what we're trying to
        // protect against.
        fs::read_to_string(kdl_path).map_err(|source| SettingsError::Read { source })?
    } else {
        // Seed with the section-commented default scaffold so a brand-new
        // file is hand-editable from the start.
        roux_core::render_settings_kdl_default()
    };

    let updated = roux_core::apply_settings_kdl(&existing, &normalized)
        .map_err(|source| SettingsError::KdlSerialize { source })?;

    write_atomic(kdl_path, updated.as_bytes())
        .map_err(|source| SettingsError::Write { source })
}

/// Per-process counter that uniquifies tmp filenames inside a single
/// process. PID alone is not enough — two debounced `update_settings`
/// commands can run concurrently in the same process and would otherwise
/// race on the same `settings.kdl.tmp` path.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bytes` to `path` atomically: write to a uniquely-named tmp file
/// in the same directory, then rename. POSIX rename is atomic within a
/// filesystem; on Windows `fs::rename` refuses to overwrite, so on that
/// platform we remove the target first as a known trade-off (the tiny
/// window between remove and rename is the price of "no half-written
/// files" on Windows; a follow-up could swap-via-backup for full
/// atomicity but needs Windows testing).
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or("settings.kdl");
    let pid = std::process::id();
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!("{stem}.tmp.{pid}.{n}"));

    let write_result = fs::write(&tmp, bytes);
    if let Err(e) = write_result {
        // Best-effort cleanup so a failed write doesn't litter the dir.
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    #[cfg(windows)]
    {
        if path.exists() {
            // Best-effort; if remove fails the rename below will surface it.
            let _ = fs::remove_file(path);
        }
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use tempfile::TempDir;

    fn paths(dir: &TempDir) -> (PathBuf, PathBuf) {
        (dir.path().join("settings.kdl"), dir.path().join("settings.json"))
    }

    #[test]
    fn default_settings_has_no_claude_binary_path() {
        let settings = RouxSettings::default();
        assert_eq!(settings.claude_binary_path, None);
    }

    #[test]
    fn settings_error_display_keeps_user_facing_message_shape() {
        let error = SettingsError::Write { source: io::Error::other("disk full") };

        assert_eq!(error.to_string(), "disk full");
    }

    #[test]
    fn load_with_neither_file_returns_defaults() {
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);
        let s = load_settings_at(&kdl, &json);
        assert_eq!(s, RouxSettings::default());
        assert!(!kdl.exists(), "should not have written anything");
        assert!(!json.exists());
    }

    #[test]
    fn load_then_save_round_trip() {
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);
        let mut s = RouxSettings::default();
        s.theme = "graphite-rose".to_string();
        s.font_size = 18;
        s.claude_binary_path = Some("/tmp/claude".to_string());

        save_settings_at(&kdl, &s).unwrap();
        let loaded = load_settings_at(&kdl, &json);
        // normalized() runs on load; compare normalized forms so the
        // legacy `cleanup_worktrees_on_close` bool agrees on both sides.
        assert_eq!(loaded, s.normalized());
    }

    #[test]
    fn migrates_legacy_json_to_kdl_and_renames_backup() {
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);

        // Minimal legacy JSON: serde defaults fill in everything else.
        let legacy = serde_json::to_string(&RouxSettings {
            theme: "mocha-soft".to_string(),
            font_size: 22,
            ..RouxSettings::default()
        })
        .unwrap();
        fs::write(&json, legacy).unwrap();

        let loaded = load_settings_at(&kdl, &json);
        assert_eq!(loaded.theme, "mocha-soft");
        assert_eq!(loaded.font_size, 22);
        assert!(kdl.exists(), "kdl file should have been written");
        assert!(!json.exists(), "json file should have been renamed");
        assert!(
            json.with_file_name("settings.json.bak").exists(),
            "expected settings.json.bak to exist; dir contents: {:?}",
            fs::read_dir(dir.path()).unwrap().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn bak_filename_uniquified_when_collision() {
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);
        let bak = dir.path().join("settings.json.bak");

        let legacy = serde_json::to_string(&RouxSettings::default()).unwrap();
        fs::write(&json, legacy).unwrap();
        fs::write(&bak, "previous backup").unwrap();

        let _ = load_settings_at(&kdl, &json);

        assert!(kdl.exists());
        assert_eq!(
            fs::read_to_string(&bak).unwrap(),
            "previous backup",
            "existing .bak must not be clobbered",
        );
        assert!(
            dir.path().join("settings.json.bak.1").exists(),
            "expected uniquified .bak.1; dir contents: {:?}",
            fs::read_dir(dir.path()).unwrap().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn corrupt_kdl_returns_defaults_without_touching_file() {
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);
        let corrupt = "ui {\n    theme \"unterminated\n";
        fs::write(&kdl, corrupt).unwrap();

        let s = load_settings_at(&kdl, &json);
        assert_eq!(s, RouxSettings::default());
        assert_eq!(
            fs::read_to_string(&kdl).unwrap(),
            corrupt,
            "broken KDL must be preserved on disk for inspection",
        );
    }

    #[test]
    fn malformed_legacy_json_does_not_clobber_or_rename() {
        // If the legacy JSON is unreadable / unparseable we must not
        // migrate defaults on top of it — the user might still recover
        // the file. Defaults are returned, but the JSON stays put with no
        // `.kdl` and no `.bak` produced.
        let dir = TempDir::new().unwrap();
        let (kdl, json) = paths(&dir);
        fs::write(&json, "not { valid: json").unwrap();

        let s = load_settings_at(&kdl, &json);
        assert_eq!(s, RouxSettings::default());
        assert!(!kdl.exists(), "must not write KDL when JSON is unparseable");
        assert!(json.exists(), "must leave the broken JSON in place");
        assert!(
            !dir.path().join("settings.json.bak").exists(),
            "must not rename to .bak when migration didn't actually run",
        );
    }

    #[test]
    fn save_surfaces_read_error_rather_than_overwriting() {
        // If we can't read the existing file but it exists, we must
        // refuse to write rather than fall back to the default scaffold —
        // otherwise a transient IO error would clobber the user's file.
        // Simulated via a directory at the kdl path: read_to_string fails
        // with EISDIR, kdl_path.exists() returns true.
        let dir = TempDir::new().unwrap();
        let kdl = dir.path().join("settings.kdl");
        fs::create_dir(&kdl).unwrap();

        let err = save_settings_at(&kdl, &RouxSettings::default()).unwrap_err();
        assert!(
            matches!(err, SettingsError::Read { .. }),
            "expected Read error, got {err:?}",
        );
    }

    #[test]
    fn save_preserves_user_added_comment() {
        let dir = TempDir::new().unwrap();
        let (kdl, _json) = paths(&dir);

        let initial = "// my hand-written note\nui {\n    theme \"deep-blue\"\n}\n";
        fs::write(&kdl, initial).unwrap();

        let mut s = RouxSettings::default();
        s.theme = "graphite-rose".to_string();
        save_settings_at(&kdl, &s).unwrap();

        let written = fs::read_to_string(&kdl).unwrap();
        assert!(written.contains("// my hand-written note"), "comment lost: {written}");
        assert!(written.contains("graphite-rose"));
    }
}
