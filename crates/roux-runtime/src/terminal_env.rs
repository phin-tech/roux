use std::path::{Path, PathBuf};

/// Nono sandbox configuration for a shell PTY. When present, the shell is
/// spawned inside `nono run` with the given profile and directory allowances.
#[derive(Debug, Clone)]
pub struct NonoConfig {
    pub profile: String,
    pub allow_dirs: Vec<String>,
}

impl NonoConfig {
    /// Resolve `~` to the user's home directory and relative paths against
    /// `working_dir`. Nono receives arguments directly, so shell expansion is
    /// not available at spawn time.
    pub fn resolved_allow_dirs(&self, working_dir: &str) -> Vec<String> {
        let home = dirs::home_dir();
        self.allow_dirs
            .iter()
            .filter_map(|d| {
                if d == "~" {
                    home.as_ref().map(|h| h.to_string_lossy().into_owned())
                } else if let Some(tail) = d.strip_prefix("~/") {
                    home.as_ref().map(|h| h.join(tail).to_string_lossy().into_owned())
                } else if Path::new(d).is_absolute() {
                    Some(d.clone())
                } else {
                    Some(Path::new(working_dir).join(d).to_string_lossy().into_owned())
                }
            })
            .collect()
    }
}

/// Smolvm exec configuration for a shell PTY. When present, the shell is
/// spawned inside `smolvm machine exec --name <machine_name> -it -- <guest_shell>`.
#[derive(Debug, Clone)]
pub struct SmolvmExec {
    pub binary: PathBuf,
    pub machine_name: String,
    pub guest_shell: String,
}

/// Pre-computed inputs for the `ROUX_*_NOTES_*` env vars. Callers resolve
/// slugs and project context before handing these values to terminal spawn.
#[derive(Debug, Clone, Default)]
pub struct NotesEnvInputs {
    pub vault_root: String,
    pub session_slug: String,
    pub repo_slug: String,
    pub project_slug: Option<String>,
    pub context_paths: Vec<String>,
    pub project_prompt: String,
}

pub struct RouxEnvInputs<'a> {
    pub user_path: &'a str,
    pub socket_path: &'a str,
    /// `(bin-dir-to-prepend-to-PATH, absolute-roux-path)`.
    pub cli_shim: Option<(&'a str, &'a str)>,
    pub session_id: Option<&'a str>,
    pub pane_id: Option<&'a str>,
    pub pane_alias: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub worktree_path: Option<&'a str>,
    pub notes: Option<&'a NotesEnvInputs>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEnvWarning {
    ProjectContextPathsJoinFailed { path_count: usize, error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouxEnvOutput {
    pub pairs: Vec<(String, String)>,
    pub warnings: Vec<TerminalEnvWarning>,
}

/// Build the PATH value to hand to a PTY child.
pub fn build_pty_path(user_path: &str, roux_cli_bin_dir: Option<&str>) -> String {
    let Some(bin_dir) = roux_cli_bin_dir else {
        return user_path.to_string();
    };

    let mut paths: Vec<_> = std::env::split_paths(user_path).collect();
    let bin_dir_path = PathBuf::from(bin_dir);
    if paths.iter().any(|path| path == &bin_dir_path) {
        return user_path.to_string();
    }

    paths.insert(0, bin_dir_path);
    std::env::join_paths(paths)
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| user_path.to_string())
}

pub fn roux_env_pairs(inputs: RouxEnvInputs<'_>) -> Vec<(String, String)> {
    roux_env_pairs_with_warnings(inputs).pairs
}

pub fn roux_env_pairs_with_warnings(inputs: RouxEnvInputs<'_>) -> RouxEnvOutput {
    let mut pairs: Vec<(String, String)> = vec![
        (
            "PATH".to_string(),
            build_pty_path(inputs.user_path, inputs.cli_shim.map(|(bin_dir, _)| bin_dir)),
        ),
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("ROUX_SESSION".to_string(), "1".to_string()),
        ("ROUX_SOCKET".to_string(), inputs.socket_path.to_string()),
    ];
    if let Some((_, cli_path)) = inputs.cli_shim {
        pairs.push(("ROUX_CLI".to_string(), cli_path.to_string()));
    }
    if let Some(sid) = inputs.session_id {
        pairs.push(("ROUX_SESSION_ID".to_string(), sid.to_string()));
    }
    if let Some(pid) = inputs.pane_id {
        pairs.push(("ROUX_PANE_ID".to_string(), pid.to_string()));
    }
    if let Some(alias) = inputs.pane_alias {
        pairs.push(("ROUX_AGENT_ALIAS".to_string(), alias.to_string()));
    }
    if let Some(pid) = inputs.project_id {
        pairs.push(("ROUX_PROJECT_ID".to_string(), pid.to_string()));
    }
    if let Some(wt) = inputs.worktree_path {
        pairs.push(("ROUX_WORKTREE_PATH".to_string(), wt.to_string()));
    }
    let mut warnings = Vec::new();
    if let Some(n) = inputs.notes {
        warnings.extend(notes_env_pairs(n, &mut pairs));
    }
    RouxEnvOutput { pairs, warnings }
}

/// True for env keys that are meaningful inside a smolvm guest.
pub fn is_guest_safe_env_key(key: &str) -> bool {
    matches!(
        key,
        "TERM"
            | "COLORTERM"
            | "ROUX_SESSION"
            | "ROUX_SESSION_ID"
            | "ROUX_PANE_ID"
            | "ROUX_PROJECT_ID"
            | "ROUX_AGENT_ALIAS"
    )
}

pub fn notes_env_pairs(
    n: &NotesEnvInputs,
    pairs: &mut Vec<(String, String)>,
) -> Vec<TerminalEnvWarning> {
    let mut warnings = Vec::new();
    let root = Path::new(&n.vault_root);
    let global_dir = root.join("global");
    let repo_dir = root.join("repos").join(&n.repo_slug);
    let session_dir = root.join("sessions").join(&n.session_slug);

    pairs.push(("ROUX_NOTES_ROOT".to_string(), root.to_string_lossy().to_string()));
    pairs.push(("ROUX_GLOBAL_NOTES_DIR".to_string(), global_dir.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_GLOBAL_NOTES_FILE".to_string(),
        global_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    pairs.push(("ROUX_REPO_SLUG".to_string(), n.repo_slug.clone()));
    pairs.push(("ROUX_REPO_NOTES_DIR".to_string(), repo_dir.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_REPO_NOTES_FILE".to_string(),
        repo_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    pairs.push(("ROUX_SESSION_DIR".to_string(), session_dir.to_string_lossy().to_string()));
    pairs.push((
        "ROUX_SESSION_NOTES_FILE".to_string(),
        session_dir.join("notes.md").to_string_lossy().to_string(),
    ));
    if let Some(project_slug) = n.project_slug.as_deref() {
        let project_dir = root.join("projects").join(project_slug);
        pairs.push(("ROUX_SESSION_PROJECT".to_string(), project_slug.to_string()));
        pairs.push((
            "ROUX_SESSION_PROJECT_NOTES_DIR".to_string(),
            project_dir.to_string_lossy().to_string(),
        ));
        pairs.push((
            "ROUX_SESSION_PROJECT_NOTES_FILE".to_string(),
            project_dir.join("notes.md").to_string_lossy().to_string(),
        ));
    }

    if !n.context_paths.is_empty() {
        match std::env::join_paths(n.context_paths.iter().map(Path::new)) {
            Ok(joined) => {
                pairs.push((
                    "ROUX_PROJECT_CONTEXT_PATHS".to_string(),
                    joined.to_string_lossy().to_string(),
                ));
            }
            Err(e) => warnings.push(TerminalEnvWarning::ProjectContextPathsJoinFailed {
                path_count: n.context_paths.len(),
                error: e.to_string(),
            }),
        }
    }
    if !n.project_prompt.is_empty() {
        pairs.push(("ROUX_PROJECT_PROMPT".to_string(), n.project_prompt.clone()));
    }
    warnings
}

#[cfg(not(windows))]
pub fn resolve_default_shell_from_sources(
    setting_shell: Option<&str>,
    login_shell: Option<&str>,
    env_shell: Option<&str>,
) -> String {
    setting_shell
        .and_then(nonempty_trimmed)
        .or_else(|| login_shell.and_then(nonempty_trimmed))
        .or_else(|| env_shell.and_then(nonempty_trimmed))
        .unwrap_or("/bin/zsh")
        .to_string()
}

pub fn nonempty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(unix)]
pub fn login_shell_for_current_user() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    if buf_size < 1024 {
        buf_size = 16 * 1024;
    }
    let mut buf = vec![0 as libc::c_char; buf_size as usize];

    loop {
        let mut passwd = std::mem::MaybeUninit::<libc::passwd>::zeroed();
        let rc = unsafe {
            libc::getpwuid_r(uid, passwd.as_mut_ptr(), buf.as_mut_ptr(), buf.len(), &mut result)
        };
        if rc == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if rc != 0 || result.is_null() {
            return None;
        }

        let passwd = unsafe { passwd.assume_init() };
        if passwd.pw_shell.is_null() {
            return None;
        }
        let shell = unsafe { std::ffi::CStr::from_ptr(passwd.pw_shell) };
        return shell.to_str().ok().and_then(nonempty_trimmed).map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_pty_path_prepends_shim_once() {
        let path = build_pty_path("/usr/bin:/bin", Some("/roux/bin"));
        assert_eq!(path, "/roux/bin:/usr/bin:/bin");

        let path = build_pty_path(&path, Some("/roux/bin"));
        assert_eq!(path, "/roux/bin:/usr/bin:/bin");
    }

    #[test]
    fn roux_env_pairs_includes_identity_socket_cli_and_notes() {
        let notes = NotesEnvInputs {
            vault_root: "/vault".to_string(),
            session_slug: "session-a".to_string(),
            repo_slug: "repo-a".to_string(),
            project_slug: Some("project-a".to_string()),
            context_paths: vec!["/specs/a.md".to_string(), "/specs/b.md".to_string()],
            project_prompt: "Follow the spec".to_string(),
        };

        let pairs = roux_env_pairs(RouxEnvInputs {
            user_path: "/usr/bin",
            socket_path: "/tmp/roux.sock",
            cli_shim: Some(("/roux/bin", "/roux/bin/roux")),
            session_id: Some("session-a"),
            pane_id: Some("pane-a"),
            pane_alias: Some("agent-a"),
            project_id: Some("project-id"),
            worktree_path: Some("/repo"),
            notes: Some(&notes),
        });

        assert!(pairs.contains(&("ROUX_SOCKET".to_string(), "/tmp/roux.sock".to_string())));
        assert!(pairs.contains(&("ROUX_CLI".to_string(), "/roux/bin/roux".to_string())));
        assert!(pairs.contains(&("ROUX_AGENT_ALIAS".to_string(), "agent-a".to_string())));
        assert!(pairs.contains(&("ROUX_PROJECT_PROMPT".to_string(), "Follow the spec".to_string())));
        assert!(pairs.iter().any(|(k, v)| k == "PATH" && v.starts_with("/roux/bin:")));
    }

    #[test]
    fn roux_env_pairs_reports_invalid_context_paths() {
        #[cfg(windows)]
        let invalid_context_path = "bad;path".to_string();
        #[cfg(not(windows))]
        let invalid_context_path = "bad:path".to_string();

        let notes = NotesEnvInputs {
            vault_root: "/vault".to_string(),
            session_slug: "session-a".to_string(),
            repo_slug: "repo-a".to_string(),
            project_slug: None,
            context_paths: vec![invalid_context_path],
            project_prompt: String::new(),
        };

        let output = roux_env_pairs_with_warnings(RouxEnvInputs {
            user_path: "/usr/bin",
            socket_path: "/tmp/roux.sock",
            cli_shim: None,
            session_id: None,
            pane_id: None,
            pane_alias: None,
            project_id: None,
            worktree_path: None,
            notes: Some(&notes),
        });

        assert!(!output.pairs.iter().any(|(k, _)| k == "ROUX_PROJECT_CONTEXT_PATHS"));
        assert_eq!(output.warnings.len(), 1);
        assert!(matches!(
            &output.warnings[0],
            TerminalEnvWarning::ProjectContextPathsJoinFailed { path_count: 1, error }
                if !error.is_empty()
        ));
    }

    #[test]
    fn guest_safe_env_excludes_host_paths() {
        assert!(is_guest_safe_env_key("ROUX_SESSION_ID"));
        assert!(is_guest_safe_env_key("ROUX_AGENT_ALIAS"));
        assert!(!is_guest_safe_env_key("PATH"));
        assert!(!is_guest_safe_env_key("ROUX_SOCKET"));
        assert!(!is_guest_safe_env_key("ROUX_CLI"));
        assert!(!is_guest_safe_env_key("ROUX_NOTES_ROOT"));
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_prefers_explicit_setting_over_login_shell_and_env() {
        let shell = resolve_default_shell_from_sources(
            Some(" /custom/fish "),
            Some("/bin/zsh"),
            Some("/bin/bash"),
        );

        assert_eq!(shell, "/custom/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_prefers_login_shell_over_env_shell() {
        let shell = resolve_default_shell_from_sources(
            None,
            Some("/opt/homebrew/bin/fish"),
            Some("/bin/zsh"),
        );

        assert_eq!(shell, "/opt/homebrew/bin/fish");
    }

    #[cfg(not(windows))]
    #[test]
    fn default_shell_uses_env_shell_when_login_shell_is_unavailable() {
        let shell = resolve_default_shell_from_sources(None, None, Some("/bin/bash"));

        assert_eq!(shell, "/bin/bash");
    }

    #[test]
    fn resolved_allow_dirs_expands_tilde() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["~/data".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved[0].starts_with('/'), "should be absolute: {}", resolved[0]);
        assert!(resolved[0].ends_with("/data"), "should end with /data: {}", resolved[0]);
        assert!(!resolved[0].contains('~'), "should not contain tilde: {}", resolved[0]);
    }

    #[test]
    fn resolved_allow_dirs_resolves_relative() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["local/dir".into()] };
        let resolved = nono.resolved_allow_dirs("/work/project");
        assert_eq!(resolved[0], "/work/project/local/dir");
    }

    #[test]
    fn resolved_allow_dirs_passes_absolute_through() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["/tmp/scratch".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert_eq!(resolved[0], "/tmp/scratch");
    }

    #[test]
    fn resolved_allow_dirs_handles_bare_tilde() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec!["~".into()] };
        let resolved = nono.resolved_allow_dirs("/work");
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolved[0], home.to_string_lossy());
    }

    #[test]
    fn resolved_allow_dirs_handles_empty() {
        let nono = NonoConfig { profile: "test".into(), allow_dirs: vec![] };
        let resolved = nono.resolved_allow_dirs("/work");
        assert!(resolved.is_empty());
    }
}
