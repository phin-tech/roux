//! Diagnostics surface: hook definitions (from `wt config show`) and
//! per-repo log files (from `wt config state logs`).
//!
//! Consumers (Tauri commands and tests) get typed structs with enough
//! fidelity to render a read-only sidebar panel without re-parsing
//! worktrunk's full config schema.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::detect::WtBinary;
use crate::exec::WtError;

/// Shape of `wt config state logs --format=json`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtLogs {
    #[serde(default)]
    pub command_log: Vec<WtLogEntry>,
    #[serde(default)]
    pub hook_output: Vec<WtHookOutputEntry>,
    #[serde(default)]
    pub diagnostic: Vec<WtLogEntry>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtLogEntry {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: u64,
    /// Unix seconds.
    #[serde(default)]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtHookOutputEntry {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified_at: Option<u64>,
    #[serde(default)]
    pub branch: String,
    /// "user" | "project" | "internal"
    #[serde(default)]
    pub source: String,
    /// "post-start", "pre-merge", etc. `None` for internal operations.
    #[serde(default)]
    pub hook_type: Option<String>,
    #[serde(default)]
    pub name: String,
}

/// Shape of `wt config show --format=json`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtConfigShow {
    #[serde(default)]
    pub user: WtConfigFile,
    #[serde(default)]
    pub project: WtConfigFile,
    #[serde(default)]
    pub system: WtConfigFile,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WtConfigFile {
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub path: String,
    /// The TOML-as-JSON body when `exists` is true and the file parsed
    /// cleanly. `None` otherwise.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// A hook definition extracted from a user or project config.
#[derive(Debug, Clone, Serialize)]
pub struct WtHookDef {
    /// "user" or "project".
    pub source: String,
    /// Absolute path to the config file this hook lives in.
    pub config_path: String,
    /// e.g. "post-start", "pre-merge".
    pub name: String,
    /// Displayable value. A plain string for simple hooks; for
    /// array-of-steps or object values we JSON-encode the raw value so
    /// the UI can show it without a structured renderer.
    pub command: String,
}

/// Run `wt config state logs --format=json` in `repo_path`.
pub fn list_logs(
    wt: &WtBinary,
    repo_path: &Path,
    env: &[(String, OsString)],
) -> Result<WtLogs, WtError> {
    let mut cmd = Command::new(&wt.path);
    cmd.current_dir(repo_path)
        .args(["config", "state", "logs", "--format=json"]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|source| WtError::Spawn { source })?;
    if !out.status.success() {
        return Err(WtError::NonZeroExit {
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    serde_json::from_slice::<WtLogs>(&out.stdout).map_err(|source| WtError::Parse { source })
}

/// Run `wt config show --format=json` in `repo_path`.
pub fn show_config(
    wt: &WtBinary,
    repo_path: &Path,
    env: &[(String, OsString)],
) -> Result<WtConfigShow, WtError> {
    let mut cmd = Command::new(&wt.path);
    cmd.current_dir(repo_path)
        .args(["config", "show", "--format=json"]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|source| WtError::Spawn { source })?;
    if !out.status.success() {
        return Err(WtError::NonZeroExit {
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    serde_json::from_slice::<WtConfigShow>(&out.stdout)
        .map_err(|source| WtError::Parse { source })
}

/// Read a log file's contents, capped at `max_bytes` to protect the UI
/// from accidentally loading a multi-MB file. Returns `None` when the
/// file doesn't exist.
pub fn read_log_file(path: &Path, max_bytes: u64) -> std::io::Result<Option<String>> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
            let mut content = std::fs::read(path)?;
            if (content.len() as u64) > max_bytes {
                content.truncate(max_bytes as usize);
                let suffix = format!(
                    "\n\n… truncated at {} bytes ({} total)\n",
                    max_bytes,
                    meta.len()
                );
                content.extend_from_slice(suffix.as_bytes());
            }
            Ok(Some(String::from_utf8_lossy(&content).into_owned()))
        }
        Ok(_) | Err(_) => Ok(None),
    }
}

/// Pull hook definitions out of a parsed `wt config show`. Looks at the
/// top-level keys of each config body and picks entries whose key
/// matches worktrunk's hook naming (`pre-*` / `post-*`).
pub fn extract_hook_defs(show: &WtConfigShow) -> Vec<WtHookDef> {
    let mut out = Vec::new();
    for (source, cfg) in [("user", &show.user), ("project", &show.project)] {
        if !cfg.exists {
            continue;
        }
        let Some(body) = cfg.config.as_ref() else {
            continue;
        };
        let Some(obj) = body.as_object() else {
            continue;
        };
        for (key, value) in obj {
            if !is_hook_key(key) {
                continue;
            }
            let command = value_to_display_string(value);
            out.push(WtHookDef {
                source: source.to_string(),
                config_path: cfg.path.clone(),
                name: key.clone(),
                command,
            });
        }
    }
    // Stable order: by source then by name so repeated fetches don't
    // cause the UI to re-order rows.
    out.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| a.name.cmp(&b.name)));
    out
}

fn is_hook_key(key: &str) -> bool {
    // Matches worktrunk's hook names: pre-start, post-start, pre-switch,
    // post-switch, pre-commit, post-commit, pre-merge, post-merge,
    // pre-remove, plus the deprecated post-create alias.
    matches!(
        key,
        "pre-start"
            | "post-start"
            | "pre-switch"
            | "post-switch"
            | "pre-commit"
            | "post-commit"
            | "pre-merge"
            | "post-merge"
            | "pre-remove"
            | "post-create"
    )
}

fn value_to_display_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        _ => serde_json::to_string(v).unwrap_or_else(|_| format!("{v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(exists: bool, body: serde_json::Value) -> WtConfigFile {
        WtConfigFile {
            exists,
            path: "/path/to/wt.toml".into(),
            config: if exists { Some(body) } else { None },
        }
    }

    #[test]
    fn extract_hook_defs_picks_known_hook_keys_only() {
        let show = WtConfigShow {
            user: cfg(
                true,
                json!({
                    "post-start": "npm run dev",
                    "pre-merge": "npm test",
                    "worktree-path": "~/wt/{{ branch }}",
                    "projects": {},
                }),
            ),
            project: cfg(false, json!(null)),
            system: WtConfigFile::default(),
        };
        let hooks = extract_hook_defs(&show);
        let names: Vec<_> = hooks.iter().map(|h| h.name.clone()).collect();
        assert_eq!(names, vec!["post-start".to_string(), "pre-merge".to_string()]);
    }

    #[test]
    fn extract_hook_defs_serializes_non_string_values_as_json() {
        let show = WtConfigShow {
            user: cfg(
                true,
                json!({
                    "post-start": ["npm ci", "npm run dev"],
                }),
            ),
            project: cfg(false, json!(null)),
            system: WtConfigFile::default(),
        };
        let hooks = extract_hook_defs(&show);
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].command, r#"["npm ci","npm run dev"]"#);
    }

    #[test]
    fn extract_hook_defs_combines_user_and_project_sources() {
        let show = WtConfigShow {
            user: cfg(true, json!({ "post-start": "U" })),
            project: cfg(true, json!({ "post-start": "P", "pre-commit": "lint" })),
            system: WtConfigFile::default(),
        };
        let hooks = extract_hook_defs(&show);
        assert_eq!(hooks.len(), 3);
        // Sorted by source then name: project/pre-commit, project/post-start, user/post-start
        assert_eq!(
            hooks.iter().map(|h| (h.source.clone(), h.name.clone())).collect::<Vec<_>>(),
            vec![
                ("project".into(), "post-start".into()),
                ("project".into(), "pre-commit".into()),
                ("user".into(), "post-start".into()),
            ]
        );
    }

    #[test]
    fn extract_hook_defs_skips_config_files_that_do_not_exist() {
        let show = WtConfigShow {
            user: cfg(false, json!(null)),
            project: cfg(false, json!(null)),
            system: WtConfigFile::default(),
        };
        assert!(extract_hook_defs(&show).is_empty());
    }

    #[test]
    fn read_log_file_truncates_at_max_bytes() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "abcdefghij").unwrap();
        let got = read_log_file(tmp.path(), 4).unwrap().unwrap();
        assert!(got.starts_with("abcd"));
        assert!(got.contains("truncated at 4 bytes"));
    }

    #[test]
    fn read_log_file_returns_none_for_missing_file() {
        let got = read_log_file(Path::new("/tmp/definitely/not/a/real/log.txt"), 1024).unwrap();
        assert!(got.is_none());
    }
}
