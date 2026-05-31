use std::path::PathBuf;

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
    pub(crate) runtime_id: String,
    pub(crate) rendered: RenderedExternalTool,
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
    let render_port = match tool.surface {
        ExternalToolSurface::Terminal => None,
        ExternalToolSurface::Web => port.or(tool.preferred_port).or(Some(4966)),
    };
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
    let render_port = match tool.surface {
        ExternalToolSurface::Terminal => None,
        ExternalToolSurface::Web => port.or(tool.preferred_port).or(Some(4966)),
    };
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
    let port = match tool.surface {
        ExternalToolSurface::Terminal => None,
        ExternalToolSurface::Web => Some(
            allocate_localhost_port(tool.preferred_port).map_err(|err| err.to_string())?,
        ),
    };
    let rendered =
        render_external_tool(&tool, session.as_ref(), port).map_err(|err| err.to_string())?;
    let runtime_id = match tool.surface {
        ExternalToolSurface::Terminal => {
            launch_terminal_tool(&state, &tool, session.as_ref(), &rendered, initial_size).await?
        }
        ExternalToolSurface::Web => launch_web_tool(&state, &rendered).await?,
    };

    Ok(ExternalToolLaunchResult {
        tool_id: tool.id,
        surface: tool.surface,
        session_id,
        runtime_id,
        rendered,
    })
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
) -> Result<String, String> {
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
        return Ok(record.id);
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
    Ok(record.id)
}

async fn launch_web_tool(
    state: &tauri::State<'_, AppState>,
    rendered: &RenderedExternalTool,
) -> Result<String, String> {
    if let Some(client) = &state.daemon_client {
        let record = client
            .start_daemon_process(rendered.command.clone(), Some(rendered.cwd.clone()))
            .await
            .map_err(|err| err.to_string())?;
        return Ok(record.id);
    }

    let record = state
        .runtime
        .process_handle
        .start(rendered.command.clone(), Some(PathBuf::from(&rendered.cwd)))
        .await
        .map_err(|err| err.to_string())?;
    Ok(record.id)
}
