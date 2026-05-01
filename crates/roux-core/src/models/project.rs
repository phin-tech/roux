use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub repo_roots: Vec<String>,
    #[serde(default)]
    pub context_paths: Vec<String>,
    #[serde(default)]
    pub session_blueprints: Vec<SessionBlueprint>,
    /// Free-form text injected at agent spawn time. Surfaced as
    /// `--append-system-prompt` for Claude profiles and `-c instructions=…`
    /// for Codex profiles. Empty string = no prompt.
    #[serde(default)]
    pub project_prompt: String,
}

/// Partial patch sent to `update_project`. Any field set to `Some` replaces
/// the corresponding field on the stored project; `None` leaves it untouched.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub repo_roots: Option<Vec<String>>,
    #[serde(default)]
    pub context_paths: Option<Vec<String>>,
    #[serde(default)]
    pub session_blueprints: Option<Vec<SessionBlueprint>>,
    #[serde(default)]
    pub project_prompt: Option<String>,
}

/// A saved session template attached to a project. When the user spawns a
/// blueprint, the frontend calls `create_session_shell` with these values
/// and stamps the resulting `Session.blueprint_id` so the dimmed sidebar
/// row collapses behind the live session.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionBlueprint {
    pub id: String,
    pub name: String,
    pub repo_root: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    pub spawn_profile: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub fetch_first: bool,
    #[serde(default)]
    pub nono_profile: Option<String>,
    #[serde(default)]
    pub nono_allow_dirs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_project_json_deserializes_with_empty_collections() {
        // Locks the migration contract: existing `projects.json` files written
        // before repo_roots / context_paths / session_blueprints existed must
        // still deserialize cleanly. Breaking this would silently lose old
        // projects on launch.
        let json = r#"{"id":"abc","name":"Legacy"}"#;
        let p: Project = serde_json::from_str(json).expect("legacy project must deserialize");
        assert_eq!(p.id, "abc");
        assert_eq!(p.name, "Legacy");
        assert!(p.repo_roots.is_empty());
        assert!(p.context_paths.is_empty());
        assert!(p.session_blueprints.is_empty());
        assert!(p.project_prompt.is_empty());
    }

    #[test]
    fn project_round_trips_through_camel_case() {
        let p = Project {
            id: "p1".into(),
            name: "Demo".into(),
            repo_roots: vec!["/a".into()],
            context_paths: vec!["/b/c.md".into()],
            session_blueprints: vec![SessionBlueprint {
                id: "bp1".into(),
                name: "api".into(),
                repo_root: "/a".into(),
                branch: Some("feat".into()),
                worktree_path: None,
                spawn_profile: "claude".into(),
                base: None,
                fetch_first: false,
                nono_profile: None,
                nono_allow_dirs: Vec::new(),
            }],
            project_prompt: "always cite the spec".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        // camelCase wire shape — frontend types depend on this.
        assert!(json.contains("\"repoRoots\""));
        assert!(json.contains("\"sessionBlueprints\""));
        assert!(json.contains("\"contextPaths\""));
        assert!(json.contains("\"projectPrompt\":\"always cite the spec\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.repo_roots, p.repo_roots);
        assert_eq!(back.session_blueprints.len(), 1);
        assert_eq!(back.session_blueprints[0].id, "bp1");
    }
}
