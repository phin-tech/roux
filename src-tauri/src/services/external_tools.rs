use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use roux_core::{providers::shell_quote, ExternalTool, ExternalToolSurface, Session};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExternalToolError {
    #[error("External tool \"{0}\" requires an active session")]
    RequiresSession(String),
    #[error("Web external tool \"{0}\" requires a url template")]
    MissingUrlTemplate(String),
    #[error("Failed to render {field}: {source}")]
    Render { field: &'static str, source: minijinja::Error },
    #[error("Failed to allocate localhost port: {0}")]
    Port(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderedExternalTool {
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) url: Option<String>,
    pub(crate) port: Option<u16>,
}

pub(crate) fn render_external_tool(
    tool: &ExternalTool,
    session: Option<&Session>,
    port: Option<u16>,
) -> Result<RenderedExternalTool, ExternalToolError> {
    if tool.requires_session && session.is_none() {
        return Err(ExternalToolError::RequiresSession(tool.name.clone()));
    }
    if tool.surface == ExternalToolSurface::Web && tool.url_template.as_deref().is_none() {
        return Err(ExternalToolError::MissingUrlTemplate(tool.name.clone()));
    }

    let context = render_context(tool, session, port);
    let command = render_template("command", &tool.command_template, &context)?;
    let cwd = render_template("cwd", &tool.cwd_template, &context)?;
    let cwd = if cwd.trim().is_empty() { default_cwd() } else { cwd };
    let url = match tool.surface {
        ExternalToolSurface::Terminal => None,
        ExternalToolSurface::Web => Some(render_template(
            "url",
            tool.url_template.as_deref().unwrap_or_default(),
            &context,
        )?),
    };

    Ok(RenderedExternalTool { command, cwd, url, port })
}

pub(crate) fn allocate_localhost_port(preferred: Option<u16>) -> Result<u16, ExternalToolError> {
    if let Some(port) = preferred.filter(|port| *port > 0) {
        let upper = port.saturating_add(200);
        for candidate in port..=upper {
            if port_available(candidate) {
                return Ok(candidate);
            }
        }
    }

    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|err| ExternalToolError::Port(err.to_string()))?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|err| ExternalToolError::Port(err.to_string()))
}

fn render_template(
    field: &'static str,
    template: &str,
    context: &Value,
) -> Result<String, ExternalToolError> {
    if template.trim().is_empty() {
        return Ok(String::new());
    }
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_filter("shell_quote", shell_quote_filter);
    env.render_str(template, context)
        .map_err(|source| ExternalToolError::Render { field, source })
}

fn shell_quote_filter(value: String) -> String {
    shell_quote(&value)
}

fn render_context(tool: &ExternalTool, session: Option<&Session>, port: Option<u16>) -> Value {
    let session_value = session.map(session_ctx).unwrap_or(Value::Null);
    json!({
        "tool": {
            "id": tool.id,
            "name": tool.name,
            "surface": match tool.surface {
                ExternalToolSurface::Terminal => "terminal",
                ExternalToolSurface::Web => "web",
            },
        },
        "session": session_value,
        "port": port,
        "paths": {
            "home": home_dir_string(),
        },
    })
}

fn session_ctx(s: &Session) -> Value {
    json!({
        "id": s.id,
        "name": s.name,
        "repo_root": s.repo_root,
        "worktree_path": s.worktree_path,
        "worktree_name": last_path_segment(&s.worktree_path),
        "branch": if s.branch.is_empty() { Value::Null } else { Value::String(s.branch.clone()) },
        "is_worktree": s.is_worktree,
        "blueprint_id": s.blueprint_id,
        "project_id": s.project_id,
    })
}

fn last_path_segment(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .rfind(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn default_cwd() -> String {
    home_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .to_string_lossy()
        .to_string()
}

fn home_dir_string() -> String {
    home_dir().map(|path| path.to_string_lossy().to_string()).unwrap_or_default()
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{ExternalTool, ExternalToolSurface, Session, SessionStatus};
    use std::net::TcpListener;

    fn session() -> Session {
        Session {
            id: "s-1".to_string(),
            name: "Feature Work".to_string(),
            repo_root: "/repo".to_string(),
            worktree_path: "/repo/work trees/feat".to_string(),
            branch: "feat".to_string(),
            is_worktree: true,
            status: SessionStatus::Idle,
            model: None,
            cost: None,
            created_at: 0,
            project_id: Some("p-1".to_string()),
            is_git_repo: true,
            name_override: None,
            primary_pty_id: None,
            archived: false,
            ended_at: None,
            blueprint_id: Some("bp-1".to_string()),
            pinned_pr_url: None,
        }
    }

    #[test]
    fn renders_session_port_and_shell_quote_context() {
        let tool = ExternalTool {
            id: "quoted".to_string(),
            name: "Quoted".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Web,
            command_template: "serve {{ session.worktree_path | shell_quote }} --port {{ port }}"
                .to_string(),
            cwd_template: "{{ session.worktree_path }}".to_string(),
            requires_session: true,
            url_template: Some("http://127.0.0.1:{{ port }}/{{ session.worktree_name }}".into()),
            preferred_port: Some(4966),
        };

        let rendered = render_external_tool(&tool, Some(&session()), Some(4999)).unwrap();

        assert_eq!(rendered.command, "serve '/repo/work trees/feat' --port 4999");
        assert_eq!(rendered.cwd, "/repo/work trees/feat");
        assert_eq!(rendered.url.as_deref(), Some("http://127.0.0.1:4999/feat"));
    }

    #[test]
    fn session_required_tool_errors_without_session() {
        let tool = ExternalTool {
            id: "needs-session".to_string(),
            name: "Needs Session".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Terminal,
            command_template: "lazygit".to_string(),
            cwd_template: "{{ session.worktree_path }}".to_string(),
            requires_session: true,
            url_template: None,
            preferred_port: None,
        };

        let err = render_external_tool(&tool, None, None).unwrap_err();
        assert!(err.to_string().contains("requires an active session"));
    }

    #[test]
    fn web_tool_requires_url_template() {
        let tool = ExternalTool {
            id: "bad-web".to_string(),
            name: "Bad Web".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Web,
            command_template: "serve".to_string(),
            cwd_template: "".to_string(),
            requires_session: false,
            url_template: None,
            preferred_port: Some(4966),
        };

        let err = render_external_tool(&tool, None, Some(4966)).unwrap_err();
        assert!(err.to_string().contains("url template"));
    }

    #[test]
    fn port_allocator_skips_busy_preferred_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = listener.local_addr().unwrap().port();

        let allocated = allocate_localhost_port(Some(busy)).unwrap();

        assert_ne!(allocated, busy);
        assert!(allocated > 0);
    }
}
