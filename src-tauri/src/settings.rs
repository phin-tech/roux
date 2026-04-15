use std::fs;
use std::path::{Path, PathBuf};
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
///    prior `.bak` exists) for rollback safety.
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
    RouxSettings::default()
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
    let content = fs::read_to_string(json_path).unwrap_or_default();
    let parsed: RouxSettings = serde_json::from_str(&content).unwrap_or_default();
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
        fs::read_to_string(kdl_path).unwrap_or_default()
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

/// Write `bytes` to `path` atomically: write to `path.tmp` first, then
/// rename. POSIX rename is atomic within a filesystem; on Windows
/// `fs::rename` will refuse to overwrite, so on that platform we remove
/// the target first as a known trade-off (the tiny window between remove
/// and rename is the price of "no half-written files" on Windows).
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{ext}.tmp"),
        None => "tmp".to_string(),
    });
    fs::write(&tmp, bytes)?;
    #[cfg(windows)]
    {
        if path.exists() {
            // Best-effort; if remove fails the rename below will surface it.
            let _ = fs::remove_file(path);
        }
    }
    fs::rename(&tmp, path)
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

        fs::write(&json, "{\"theme\":\"deep-blue\"}").unwrap();
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
