use crate::services::projects as svc;
use crate::state::AppState;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use roux_core::{Project, ProjectUpdate};
use serde_json::Value;

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Project>, String> {
    state.project_handle.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_project(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<Project, String> {
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        repo_roots: Vec::new(),
        context_paths: Vec::new(),
        session_blueprints: Vec::new(),
        project_prompt: String::new(),
    };
    state.project_handle.add(project.clone()).await.map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_project(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let removed = state.project_handle.get(&id).await.map_err(|e| e.to_string())?;
    state.project_handle.remove(&id).await.map_err(|e| e.to_string())?;
    if let Err(e) = state.session_handle.clear_project_refs(&id).await {
        if let Some(project) = removed {
            let _ = state.project_handle.add(project).await;
        }
        return Err(e.to_string());
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn rename_project(
    id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.project_handle.rename(&id, &name).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn update_project(
    id: String,
    patch: ProjectUpdate,
    state: tauri::State<'_, AppState>,
) -> Result<Project, String> {
    state
        .project_handle
        .update(&id, patch)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {} not found", id))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn set_session_project(
    session_id: String,
    project_id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    svc::set_session_project(&state.session_handle, &session_id, project_id)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) fn render_project_prompt_template_inner(
    template: &str,
    context: Value,
) -> Result<String, String> {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.render_str(template, &context).map_err(|e| e.to_string())
}

// No #[specta::specta]: serde_json::Value produces invalid generated
// TypeScript for this dynamic Minijinja context. The frontend uses a
// manually typed wrapper in src/lib/tauri.ts instead.
#[tauri::command]
pub(crate) async fn render_project_prompt_template(
    template: String,
    context: Value,
) -> Result<String, String> {
    render_project_prompt_template_inner(&template, context)
}

#[cfg(test)]
mod template_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_project_prompt_scalars() {
        let rendered = render_project_prompt_template_inner(
            "Model {{ model.name }} on {{ session.branch }} in {{ session.worktree_name }}",
            json!({
                "model": { "name": "claude-opus-4-6" },
                "session": {
                    "branch": "feature/templates",
                    "worktree_name": "repo-feature"
                }
            }),
        )
        .unwrap();

        assert_eq!(rendered, "Model claude-opus-4-6 on feature/templates in repo-feature");
    }

    #[test]
    fn renders_other_sessions_loop() {
        let rendered = render_project_prompt_template_inner(
            "{% for s in other_sessions %}{{ s.name }}:{{ s.branch }};{% else %}none{% endfor %}",
            json!({
                "other_sessions": [
                    { "name": "api", "branch": "api-work" },
                    { "name": "web", "branch": "web-work" }
                ]
            }),
        )
        .unwrap();

        assert_eq!(rendered, "api:api-work;web:web-work;");
    }

    #[test]
    fn malformed_template_returns_error() {
        let err = render_project_prompt_template_inner("{% for s in other_sessions %}", json!({}))
            .unwrap_err();

        assert!(!err.is_empty());
    }

    #[test]
    fn missing_variables_return_error() {
        let err =
            render_project_prompt_template_inner("{{ session.missing }}", json!({ "session": {} }))
                .unwrap_err();

        assert!(err.contains("undefined value"));
    }
}
