use std::path::PathBuf;
use std::time::Duration;

use crate::services::external_tools::{
    allocate_localhost_port, render_external_tool, RenderedExternalTool,
};
use crate::state::AppState;
use roux_core::{ExternalTool, ExternalToolSurface, Session};
use roux_runtime::pty_service::PtySpawnRequest;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExternalToolLaunchResult {
    pub(crate) tool_id: String,
    pub(crate) surface: ExternalToolSurface,
    pub(crate) session_id: Option<String>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) runtime_generation: Option<u64>,
    pub(crate) rendered: RenderedExternalTool,
}

struct ExternalToolRuntime {
    id: Option<String>,
    generation: Option<u64>,
}

#[tauri::command]
pub(crate) async fn preview_external_tool(
    tool_id: String,
    session_id: Option<String>,
    port: Option<u16>,
    state: tauri::State<'_, AppState>,
) -> Result<RenderedExternalTool, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone().normalized();
    let tool = find_tool(&settings.external_tools, &tool_id)?;
    let session = resolve_session(&state, session_id.as_deref()).await?;
    let render_port = preview_render_port(&tool, port);
    render_external_tool(&tool, session.as_ref(), render_port).map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn preview_external_tool_config(
    tool: ExternalTool,
    session_id: Option<String>,
    port: Option<u16>,
    state: tauri::State<'_, AppState>,
) -> Result<RenderedExternalTool, String> {
    let session = resolve_session(&state, session_id.as_deref()).await?;
    let render_port = preview_render_port(&tool, port);
    render_external_tool(&tool, session.as_ref(), render_port).map_err(|err| err.to_string())
}

#[tauri::command]
pub(crate) async fn launch_external_tool(
    tool_id: String,
    session_id: Option<String>,
    initial_size: Option<(u16, u16)>,
    state: tauri::State<'_, AppState>,
) -> Result<ExternalToolLaunchResult, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone().normalized();
    let tool = find_tool(&settings.external_tools, &tool_id)?;
    if !tool.enabled {
        return Err(format!("External tool \"{}\" is disabled", tool.name));
    }

    let session = resolve_session(&state, session_id.as_deref()).await?;
    let port = launch_render_port(&tool)?;
    let rendered =
        render_external_tool(&tool, session.as_ref(), port).map_err(|err| err.to_string())?;
    let runtime = match tool.surface {
        ExternalToolSurface::Terminal => {
            launch_terminal_tool(&state, &tool, session.as_ref(), &rendered, initial_size).await?
        }
        ExternalToolSurface::Web => launch_web_tool(&state, &rendered).await?,
    };

    Ok(ExternalToolLaunchResult {
        tool_id: tool.id,
        surface: tool.surface,
        session_id,
        runtime_id: runtime.id,
        runtime_generation: runtime.generation,
        rendered,
    })
}

#[tauri::command]
pub(crate) async fn probe_external_tool_url(url: String) -> Result<bool, String> {
    let url =
        reqwest::Url::parse(&url).map_err(|err| format!("invalid external tool URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("unsupported external tool URL scheme: {scheme}")),
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|err| format!("failed to build external tool URL probe: {err}"))?;

    Ok(client.get(url).send().await.is_ok())
}

fn find_tool(tools: &[ExternalTool], tool_id: &str) -> Result<ExternalTool, String> {
    tools
        .iter()
        .find(|tool| tool.id == tool_id)
        .cloned()
        .ok_or_else(|| format!("External tool \"{}\" not found", tool_id))
}

async fn resolve_session(
    state: &tauri::State<'_, AppState>,
    session_id: Option<&str>,
) -> Result<Option<Session>, String> {
    let Some(id) = session_id else {
        return Ok(None);
    };
    if let Some(client) = &state.daemon_client {
        return client.get_session(id.to_string()).await.map(Some).map_err(Into::into);
    }
    state
        .runtime
        .session_handle
        .get(id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("Session \"{}\" not found", id))
        .map(Some)
}

async fn launch_terminal_tool(
    state: &tauri::State<'_, AppState>,
    tool: &ExternalTool,
    session: Option<&Session>,
    rendered: &RenderedExternalTool,
    initial_size: Option<(u16, u16)>,
) -> Result<ExternalToolRuntime, String> {
    let pty_id = format!("external-tool-{}-{}", tool.id, uuid::Uuid::new_v4());
    let session_id = session.map(|s| s.id.clone());
    if let Some(client) = &state.daemon_client {
        let record = client
            .spawn_daemon_pty_task(
                rendered.command.clone(),
                Some(pty_id),
                Some(rendered.cwd.clone()),
                session_id,
                None,
                None,
                initial_size,
            )
            .await
            .map_err(|err| err.to_string())?;
        return Ok(ExternalToolRuntime {
            id: Some(record.id),
            generation: Some(record.generation),
        });
    }

    let record = state
        .runtime
        .pty_handle
        .spawn_task(
            rendered.command.clone(),
            PtySpawnRequest {
                id: Some(pty_id),
                working_dir: Some(PathBuf::from(&rendered.cwd)),
                session_id,
                project_id: session.and_then(|s| s.project_id.clone()),
                worktree_path: session.map(|s| s.worktree_path.clone()),
                initial_size,
                ..PtySpawnRequest::default()
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(ExternalToolRuntime { id: Some(record.id), generation: Some(record.generation) })
}

async fn launch_web_tool(
    state: &tauri::State<'_, AppState>,
    rendered: &RenderedExternalTool,
) -> Result<ExternalToolRuntime, String> {
    if rendered.command.trim().is_empty() {
        return Ok(ExternalToolRuntime { id: None, generation: None });
    }

    if let Some(client) = &state.daemon_client {
        let record = client
            .start_daemon_process(rendered.command.clone(), Some(rendered.cwd.clone()))
            .await
            .map_err(|err| err.to_string())?;
        return Ok(ExternalToolRuntime { id: Some(record.id), generation: None });
    }

    let record = state
        .runtime
        .process_handle
        .start(rendered.command.clone(), Some(PathBuf::from(&rendered.cwd)))
        .await
        .map_err(|err| err.to_string())?;
    Ok(ExternalToolRuntime { id: Some(record.id), generation: None })
}

fn preview_render_port(tool: &ExternalTool, requested_port: Option<u16>) -> Option<u16> {
    if tool.surface == ExternalToolSurface::Web && tool_uses_port(tool) {
        requested_port.or(tool.preferred_port).or(Some(4966))
    } else {
        None
    }
}

fn launch_render_port(tool: &ExternalTool) -> Result<Option<u16>, String> {
    if tool.surface == ExternalToolSurface::Web && tool_uses_port(tool) {
        allocate_localhost_port(tool.preferred_port).map(Some).map_err(|err| err.to_string())
    } else {
        Ok(None)
    }
}

fn tool_uses_port(tool: &ExternalTool) -> bool {
    template_uses_port(&tool.command_template)
        || tool.url_template.as_deref().is_some_and(template_uses_port)
}

fn template_uses_port(template: &str) -> bool {
    let mut remaining = template;
    while let Some(open) = remaining.find("{{") {
        let after_open = &remaining[open + 2..];
        let Some(close) = after_open.find("}}") else {
            return false;
        };
        if expression_uses_port(after_open[..close].trim_start()) {
            return true;
        }
        remaining = &after_open[close + 2..];
    }
    false
}

fn expression_uses_port(expression: &str) -> bool {
    let Some(rest) = expression.strip_prefix("port") else {
        return false;
    };
    rest.chars().next().is_none_or(|ch| !is_identifier_continue(ch))
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::ExternalToolWebEmbedder;

    fn web_tool(command_template: &str, url_template: Option<&str>) -> ExternalTool {
        ExternalTool {
            id: "test".into(),
            name: "Test".into(),
            enabled: true,
            surface: ExternalToolSurface::Web,
            command_template: command_template.into(),
            cwd_template: ".".into(),
            requires_session: false,
            url_template: url_template.map(str::to_string),
            preferred_port: None,
            web_embedder: ExternalToolWebEmbedder::Webview,
        }
    }

    #[test]
    fn tool_uses_port_detects_minijinja_port_without_spaces() {
        assert!(tool_uses_port(&web_tool("serve --port {{port}}", None)));
        assert!(tool_uses_port(&web_tool("", Some("http://127.0.0.1:{{port}}"))));
    }

    #[test]
    fn tool_uses_port_detects_port_before_filter_expression() {
        assert!(tool_uses_port(&web_tool("serve --port {{port | string}}", None)));
    }

    #[test]
    fn tool_uses_port_ignores_other_identifiers() {
        assert!(!tool_uses_port(&web_tool("echo {{airport}}", Some("https://github.com"))));
    }
}
