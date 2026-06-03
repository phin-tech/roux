use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesEnv {
    pub vault_root: String,
    pub session_slug: String,
    pub repo_slug: String,
    pub project_slug: Option<String>,
    pub context_paths: Vec<String>,
    pub project_prompt: String,
}

#[derive(Debug, Clone)]
pub struct CreateSessionShell {
    pub id: String,
    pub repo_path: String,
    pub name: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fetch_first: bool,
    pub profile: Option<String>,
    pub profile_data: Option<roux_core::SpawnProfile>,
    pub env_overrides: Option<BTreeMap<String, roux_core::TerminalEnvRule>>,
    pub initial_size: Option<(u16, u16)>,
    pub project_id: Option<String>,
    pub blueprint_id: Option<String>,
    pub notes: Option<NotesEnv>,
}

#[derive(Debug, Clone)]
pub struct ReconnectSessionShell {
    pub id: String,
    pub profile: Option<String>,
    pub profile_data: Option<roux_core::SpawnProfile>,
    pub env_overrides: Option<BTreeMap<String, roux_core::TerminalEnvRule>>,
    pub initial_size: Option<(u16, u16)>,
    pub notes: Option<NotesEnv>,
}

#[derive(Debug, Clone, Default)]
pub struct MailboxPost {
    pub to: Option<String>,
    pub topic: Option<String>,
    pub body: String,
    pub subject: Option<String>,
    pub kind: Option<String>,
    pub project_id: Option<String>,
    pub correlation_id: Option<String>,
    pub structured: Option<Value>,
    pub from: Option<String>,
}

impl CreateSessionShell {
    pub(crate) fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        args.insert("id".into(), Value::String(self.id));
        args.insert("repoPath".into(), Value::String(self.repo_path));
        args.insert("name".into(), Value::String(self.name));
        insert_optional_string(&mut args, "worktreePath", self.worktree_path);
        insert_optional_string(&mut args, "branch", self.branch);
        insert_optional_string(&mut args, "base", self.base);
        if self.fetch_first {
            args.insert("fetchFirst".into(), Value::Bool(true));
        }
        insert_optional_string(&mut args, "profile", self.profile);
        insert_optional_profile(&mut args, "profileData", self.profile_data);
        insert_optional_env_overrides(&mut args, self.env_overrides);
        insert_initial_size(&mut args, self.initial_size);
        insert_optional_string(&mut args, "projectId", self.project_id);
        insert_optional_string(&mut args, "blueprintId", self.blueprint_id);
        insert_notes_env(&mut args, self.notes);
        Value::Object(args)
    }
}

impl ReconnectSessionShell {
    pub(crate) fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        insert_optional_string(&mut args, "profile", self.profile);
        insert_optional_profile(&mut args, "profileData", self.profile_data);
        insert_optional_env_overrides(&mut args, self.env_overrides);
        insert_initial_size(&mut args, self.initial_size);
        insert_notes_env(&mut args, self.notes);
        Value::Object(args)
    }
}

impl MailboxPost {
    pub(crate) fn into_args(self) -> Value {
        let mut args = serde_json::Map::new();
        insert_optional_string(&mut args, "to", self.to);
        insert_optional_string(&mut args, "topic", self.topic);
        args.insert("body".into(), Value::String(self.body));
        insert_optional_string(&mut args, "subject", self.subject);
        insert_optional_string(&mut args, "kind", self.kind);
        insert_optional_string(&mut args, "project_id", self.project_id);
        insert_optional_string(&mut args, "correlation_id", self.correlation_id);
        if let Some(structured) = self.structured {
            args.insert("structured".into(), structured);
        }
        insert_optional_string(&mut args, "from", self.from);
        Value::Object(args)
    }
}

fn insert_optional_string(
    args: &mut serde_json::Map<String, Value>,
    key: &'static str,
    value: Option<String>,
) {
    if let Some(value) = value {
        args.insert(key.into(), Value::String(value));
    }
}

fn insert_optional_profile(
    args: &mut serde_json::Map<String, Value>,
    key: &'static str,
    value: Option<roux_core::SpawnProfile>,
) {
    if let Some(value) = value.and_then(|value| serde_json::to_value(value).ok()) {
        args.insert(key.into(), value);
    }
}

fn insert_optional_env_overrides(
    args: &mut serde_json::Map<String, Value>,
    value: Option<BTreeMap<String, roux_core::TerminalEnvRule>>,
) {
    if let Some(value) = value.and_then(|value| serde_json::to_value(value).ok()) {
        args.insert("envOverrides".into(), value);
    }
}

fn insert_initial_size(
    args: &mut serde_json::Map<String, Value>,
    initial_size: Option<(u16, u16)>,
) {
    if let Some((cols, rows)) = initial_size {
        args.insert("initialSize".into(), serde_json::json!([cols, rows]));
    }
}

fn insert_notes_env(args: &mut serde_json::Map<String, Value>, notes: Option<NotesEnv>) {
    if let Some(notes) = notes {
        args.insert(
            "notesEnv".into(),
            serde_json::json!({
                "vaultRoot": notes.vault_root,
                "sessionSlug": notes.session_slug,
                "repoSlug": notes.repo_slug,
                "projectSlug": notes.project_slug,
                "contextPaths": notes.context_paths,
                "projectPrompt": notes.project_prompt,
            }),
        );
    }
}
