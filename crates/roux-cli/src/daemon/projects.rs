use roux_core::{Project, ProjectUpdate};
use roux_runtime::host::RuntimeHost;

use super::optional_string_arg;
use super::protocol::{Request, Response};

pub(super) async fn handle_project_list(host: &RuntimeHost) -> Response {
    match host.project_handle.list().await {
        Ok(projects) => match serde_json::to_value(&projects) {
            Ok(value) => Response::success(value),
            Err(err) => Response::err(format!("failed to serialize projects: {err}")),
        },
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_project_create(req: Request, host: &RuntimeHost) -> Response {
    let Some(name) = optional_string_arg(&req.args, &["name"]) else {
        return Response::err("name required");
    };
    let id =
        optional_string_arg(&req.args, &["id"]).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let project = Project {
        id,
        name,
        repo_roots: Vec::new(),
        context_paths: Vec::new(),
        session_blueprints: Vec::new(),
        project_prompt: String::new(),
    };
    match host.project_handle.add(project.clone()).await {
        Ok(()) => serialize_project(project),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_project_remove(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let removed = match host.project_handle.get(&id).await {
        Ok(project) => project,
        Err(err) => return Response::err(err.to_string()),
    };
    if let Err(err) = host.project_handle.remove(&id).await {
        return Response::err(err.to_string());
    }
    if let Err(err) = host.session_handle.clear_project_refs(&id).await {
        if let Some(project) = removed {
            if let Err(restore_err) = host.project_handle.add(project).await {
                return Response::err(format!(
                    "failed to clear project refs: {err}; failed to restore removed project: {restore_err}"
                ));
            }
        }
        return Response::err(err.to_string());
    }
    Response::success(serde_json::json!({ "id": id }))
}

pub(super) async fn handle_project_rename(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let Some(name) = optional_string_arg(&req.args, &["name"]) else {
        return Response::err("name required");
    };
    match host.project_handle.rename(&id, &name).await {
        Ok(()) => Response::success(serde_json::json!({ "id": id, "name": name })),
        Err(err) => Response::err(err.to_string()),
    }
}

pub(super) async fn handle_project_update(req: Request, host: &RuntimeHost) -> Response {
    let Some(id) = optional_string_arg(&req.args, &["id", "projectId", "project_id"]) else {
        return Response::err("id required");
    };
    let patch_value = req.args.get("patch").cloned().unwrap_or_else(|| req.args.clone());
    let patch: ProjectUpdate = match serde_json::from_value(patch_value) {
        Ok(patch) => patch,
        Err(err) => return Response::err(format!("invalid project patch: {err}")),
    };
    match host.project_handle.update(&id, patch).await {
        Ok(Some(project)) => serialize_project(project),
        Ok(None) => Response::err(format!("project {id} not found")),
        Err(err) => Response::err(err.to_string()),
    }
}

fn serialize_project(project: Project) -> Response {
    match serde_json::to_value(project) {
        Ok(value) => Response::success(value),
        Err(err) => Response::err(format!("failed to serialize project: {err}")),
    }
}
