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
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Vec<String>,
    pub initial_size: Option<(u16, u16)>,
    pub project_id: Option<String>,
    pub blueprint_id: Option<String>,
    pub smol_machine_name: Option<String>,
    pub notes: Option<NotesEnv>,
}

#[derive(Debug, Clone)]
pub struct ReconnectSessionShell {
    pub id: String,
    pub profile: Option<String>,
    pub nono_profile: Option<String>,
    pub nono_allow_dirs: Vec<String>,
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
