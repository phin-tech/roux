//! Server-side rendering of a project's prompt template — the text injected
//! into an agent via `--append-system-prompt` (Claude) / `-c instructions=`
//! (Codex). Mirrors the frontend `projectPromptTemplates` context shape and
//! minijinja render so headless work-item dispatch injects the same prompt the
//! desktop would.

use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use roux_core::{Project, Provider, RouxSettings, Session, SpawnProfile};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ProjectPromptError {
    #[error("failed to render project prompt: {0}")]
    Render(#[from] minijinja::Error),
}

fn last_path_segment(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .rfind(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn provider_family(profile: Option<&SpawnProfile>) -> Option<&'static str> {
    match profile.and_then(|p| p.provider.as_ref()) {
        Some(Provider::Claude) => Some("claude"),
        Some(Provider::Codex) => Some("codex"),
        None => None,
    }
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
    })
}

/// Build the minijinja context, matching the frontend `buildProjectPromptContext`.
pub fn build_context(
    project: &Project,
    session: &Session,
    profile: Option<&SpawnProfile>,
    settings: &RouxSettings,
    other_sessions: &[Session],
) -> Value {
    let family = provider_family(profile);
    let model_name = settings.default_model.as_deref().map(str::trim).filter(|s| !s.is_empty());
    json!({
        "project": {
            "id": project.id,
            "name": project.name,
            "repo_roots": project.repo_roots,
            "context_paths": project.context_paths,
        },
        "session": session_ctx(session),
        "profile": {
            "id": profile.map(|p| p.id.clone()),
            "name": profile.map(|p| p.name.clone()),
            "provider": family,
        },
        "model": { "name": model_name, "family": family },
        "paths": { "sessions_folder": session.worktree_path },
        "other_sessions": other_sessions.iter().map(session_ctx).collect::<Vec<_>>(),
    })
}

/// Render a template against a context. Strict undefined behavior so a template
/// referencing an unknown variable errors loudly (same as the desktop path).
pub fn render(template: &str, context: &Value) -> Result<String, ProjectPromptError> {
    if template.trim().is_empty() {
        return Ok(String::new());
    }
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.render_str(template, context).map_err(ProjectPromptError::from)
}

/// Render `project.project_prompt` for a dispatched session. Empty prompt →
/// empty string (no injection).
pub fn render_for_session(
    project: &Project,
    session: &Session,
    profile: Option<&SpawnProfile>,
    settings: &RouxSettings,
    other_sessions: &[Session],
) -> Result<String, ProjectPromptError> {
    if project.project_prompt.trim().is_empty() {
        return Ok(String::new());
    }
    let context = build_context(project, session, profile, settings, other_sessions);
    render(&project.project_prompt, &context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let json = json!({
            "id": "s-1", "name": "Ship board", "repoRoot": "/repo",
            "worktreePath": "/repo/.worktrees/feat", "branch": "feat",
            "isWorktree": true, "status": "idle", "createdAt": 0,
        });
        serde_json::from_value(json).unwrap()
    }

    fn project() -> Project {
        serde_json::from_value(json!({
            "id": "p-1", "name": "Roux", "repoRoots": ["/repo"], "contextPaths": [],
            "projectPrompt": "Project {{ project.name }} on {{ session.worktree_name }}",
        }))
        .unwrap()
    }

    #[test]
    fn renders_project_and_session_variables() {
        let out = render_for_session(&project(), &session(), None, &RouxSettings::default(), &[])
            .unwrap();
        assert_eq!(out, "Project Roux on feat");
    }

    #[test]
    fn empty_prompt_renders_empty() {
        let mut p = project();
        p.project_prompt = "   ".into();
        let out = render_for_session(&p, &session(), None, &RouxSettings::default(), &[]).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn unknown_variable_is_a_strict_error() {
        let mut p = project();
        p.project_prompt = "{{ nope.missing }}".into();
        assert!(render_for_session(&p, &session(), None, &RouxSettings::default(), &[]).is_err());
    }
}
