use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use thiserror::Error;

use super::profile::{ProfileSource, SpawnProfile, TerminalEnvRule};

const DEFAULT_THEME: &str = "deep-blue";
const DEFAULT_TERMINAL_THEME: &str = "match-gui";

fn default_terminal_theme() -> String {
    DEFAULT_TERMINAL_THEME.to_string()
}

fn default_ui_font_family() -> String {
    "Geist, Inter, SF Pro Display, Segoe UI, sans-serif".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum TabPosition {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum StatusBarPosition {
    Top,
    #[default]
    Bottom,
}

/// Behavior when a session that owns a worktree is closed.
///
/// - `Never` — leave the worktree on disk
/// - `Prompt` — ask the user via a confirm dialog (current default, matches
///   the legacy `cleanupWorktreesOnClose: false` behavior)
/// - `Always` — remove without asking (matches legacy `true`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeCleanupMode {
    Never,
    #[default]
    Prompt,
    Always,
}

/// Which starting point the "New Worktree" affordance uses when the user
/// clicks the primary action directly (as opposed to hovering to pick a
/// specific base from the flyout). Only affects the default — the three
/// options remain available via the submenu / command palette either way.
///
/// - `CurrentBranch` — the session's current branch (matches legacy behavior)
/// - `Main` — the local `main` branch
/// - `OriginMain` — the remote `origin/main`, with a `git fetch origin` first
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeDefaultBase {
    #[default]
    CurrentBranch,
    Main,
    OriginMain,
}

/// Which backend Roux uses to create worktrees.
///
/// - `Auto` (default) — use `wt` when it is detected on the system;
///   otherwise fall back to native `git worktree add`. This is the
///   recommended setting: users without worktrunk see no change, users
///   with worktrunk get its hooks/templates/project config for free.
/// - `Git` — always use native git. Useful as an escape hatch if a
///   worktrunk hook is misbehaving.
/// - `Worktrunk` — always prefer `wt`. If `wt` fails for any reason,
///   Roux still falls back to native git so worktree creation never
///   breaks entirely — the setting expresses preference, not veto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeProvider {
    #[default]
    Auto,
    Git,
    Worktrunk,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    Repo,
    Project,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExternalToolSurface {
    #[default]
    Terminal,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum ExternalToolWebEmbedder {
    Iframe,
    #[default]
    Webview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExternalTool {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub surface: ExternalToolSurface,
    pub command_template: String,
    #[serde(default)]
    pub cwd_template: String,
    #[serde(default)]
    pub requires_session: bool,
    #[serde(default)]
    pub url_template: Option<String>,
    #[serde(default)]
    pub preferred_port: Option<u16>,
    #[serde(default)]
    pub web_embedder: ExternalToolWebEmbedder,
    #[serde(default)]
    pub keep_webview_alive: bool,
}

fn default_external_tools() -> Vec<ExternalTool> {
    vec![
        ExternalTool {
            id: "lazygit".to_string(),
            name: "Lazygit".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Terminal,
            command_template: "lazygit -p {{ session.worktree_path | shell_quote }}".to_string(),
            cwd_template: "{{ session.worktree_path }}".to_string(),
            requires_session: true,
            url_template: None,
            preferred_port: None,
            web_embedder: ExternalToolWebEmbedder::Webview,
            keep_webview_alive: false,
        },
        ExternalTool {
            id: "difit".to_string(),
            name: "Difit".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Web,
            command_template: "difit . --host 127.0.0.1 --port {{ port }} --no-open --keep-alive"
                .to_string(),
            cwd_template: "{{ session.worktree_path }}".to_string(),
            requires_session: true,
            url_template: Some("http://127.0.0.1:{{ port }}".to_string()),
            preferred_port: Some(4966),
            web_embedder: ExternalToolWebEmbedder::Iframe,
            keep_webview_alive: false,
        },
        ExternalTool {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            enabled: true,
            surface: ExternalToolSurface::Web,
            command_template: "".to_string(),
            cwd_template: "".to_string(),
            requires_session: false,
            url_template: Some("https://github.com".to_string()),
            preferred_port: None,
            web_embedder: ExternalToolWebEmbedder::Webview,
            keep_webview_alive: false,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    PreRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySourceKind {
    #[default]
    LocalRepo,
    GitRepo,
}

/// Whether and how a Library source's skills are written into a
/// Claude-readable `.claude/skills/<name>/SKILL.md` directory.
///
/// - `Off`: Roux does not write skill files outside the Library.
/// - `Copy`: Roux writes a copy of each skill on sync; subsequent edits
///   to the synced file are detected via a content-hash manifest.
/// - `Symlink`: Roux symlinks each `.claude/skills/<name>/` entry back
///   to the source skill file. Edits in either place are the same edit.
///   On Windows, when symlinks are denied, Roux auto-degrades to `Copy`
///   for that sync run and emits a one-time toast event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum SkillSyncMode {
    #[default]
    Off,
    Copy,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySource {
    pub id: String,
    #[serde(default)]
    pub kind: LibrarySourceKind,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Per-source override for skill sync mode. `None` means "inherit
    /// the global default" (`RouxSettings::library_skill_sync_default`).
    #[serde(default)]
    pub skill_sync: Option<SkillSyncMode>,
}

/// What happens to a PTY when its pane is closed.
///
/// - `Kill` — the PTY process is killed immediately.
/// - `Detach` — the PTY keeps running in the background; it can be
///   re-attached to another pane later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum OnPaneCloseMode {
    Detach,
    #[default]
    Kill,
}

/// xterm.js renderer selection. `Auto` (default) tries WebGL and silently
/// falls back to the built-in DOM renderer if construction fails or the
/// WebGL context is lost. `On` is identical to `Auto` today — kept as a
/// distinct option because users have a clear mental model from VSCode's
/// `terminal.integrated.gpuAcceleration`. `Off` skips WebGL entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum GpuAcceleration {
    #[default]
    Auto,
    On,
    Off,
}

/// Runtime feature flags surfaced under Settings → Experiments. Each field is
/// either a `bool` (toggle) or a small enum (multi-choice). Adding a field
/// here also requires adding a registry entry in `src/lib/experiments.ts` so
/// the UI knows how to render it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct ExperimentsConfig {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum KanbanStartupSidebar {
    #[default]
    Restore,
    Sessions,
    Kanban,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum StartupTarget {
    #[default]
    Restore,
    SessionsSidebar,
    LastSession,
    KanbanWide,
    ExternalTool,
    None,
}

/// Profile policy used by the plain Split Horizontal / Split Vertical
/// commands. Profile-picker commands remain explicit regardless of this
/// setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub enum SplitProfileBehavior {
    #[default]
    PlainShell,
    AppDefaultProfile,
    ActivePaneProfile,
    AskEveryTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalDefaults {
    #[serde(default)]
    pub env: Option<std::collections::BTreeMap<String, TerminalEnvRule>>,
    #[serde(default)]
    pub before_shell_starts: Option<String>,
    #[serde(default)]
    pub split_profile_behavior: SplitProfileBehavior,
}

impl Default for TerminalDefaults {
    fn default() -> Self {
        Self {
            env: None,
            before_shell_starts: None,
            split_profile_behavior: SplitProfileBehavior::PlainShell,
        }
    }
}

fn default_agent_profile() -> String {
    "claude".to_string()
}

const DEFAULT_KANBAN_WORKFLOW_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/lib/workItems/defaultWorkflow.json"
));
const KANBAN_WORKFLOW_DEFAULT_ID: &str = "default";
pub const KANBAN_CATEGORY_TODO: &str = "todo";
pub const KANBAN_CATEGORY_PLANNING: &str = "planning";
pub const KANBAN_CATEGORY_DOING: &str = "doing";
pub const KANBAN_CATEGORY_REVIEW: &str = "review";
pub const KANBAN_CATEGORY_DONE: &str = "done";
pub const KANBAN_PHASE_TODO: &str = "todo";
pub const KANBAN_PHASE_PLANNING: &str = "planning";
pub const KANBAN_PHASE_DOING: &str = "doing";
pub const KANBAN_PHASE_REVIEW: &str = "review";
pub const KANBAN_PHASE_DONE: &str = "done";
pub const KANBAN_STAGE_TODO: &str = "todo";
pub const KANBAN_STAGE_PLANNING: &str = "planning";
pub const KANBAN_STAGE_IMPLEMENTATION: &str = "implementation";
pub const KANBAN_STAGE_FIX_CI: &str = "fix_ci";
pub const KANBAN_STAGE_LOCAL_REVIEW: &str = "local_review";
pub const KANBAN_STAGE_PR_REVIEW: &str = "pr_review";
pub const KANBAN_STAGE_DONE: &str = "done";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum KanbanWorkflowPhaseCategory {
    Todo,
    #[default]
    Planning,
    Doing,
    Review,
    Done,
}

impl KanbanWorkflowPhaseCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => KANBAN_CATEGORY_TODO,
            Self::Planning => KANBAN_CATEGORY_PLANNING,
            Self::Doing => KANBAN_CATEGORY_DOING,
            Self::Review => KANBAN_CATEGORY_REVIEW,
            Self::Done => KANBAN_CATEGORY_DONE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum KanbanWorkflowStageKind {
    #[default]
    Manual,
    Work,
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum KanbanWorkflowPromptMode {
    #[default]
    Append,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanWorkflowPromptSettings {
    pub mode: KanbanWorkflowPromptMode,
    pub instructions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum KanbanWorkflowCommandCwd {
    #[default]
    Worktree,
    Project,
    Repo,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum KanbanWorkflowRunnerSettings {
    Agent {
        #[serde(default)]
        agent_profile: Option<String>,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: KanbanWorkflowCommandCwd,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum KanbanWorkflowGateSettings {
    Manual,
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: KanbanWorkflowCommandCwd,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanWorkflowTransitions {
    pub on_complete: Option<String>,
    pub on_passed: Option<String>,
    pub on_failed: Option<String>,
    pub on_changes_requested: Option<String>,
    pub on_ci_failed: Option<String>,
    pub on_review_comments: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanWorkflowStageSettings {
    pub label: String,
    pub action_label: Option<String>,
    pub category: KanbanWorkflowPhaseCategory,
    pub kind: KanbanWorkflowStageKind,
    pub agent_profile: Option<String>,
    pub instructions: String,
    pub prompt: KanbanWorkflowPromptSettings,
    pub runner: Option<KanbanWorkflowRunnerSettings>,
    pub gate: Option<KanbanWorkflowGateSettings>,
    pub env: BTreeMap<String, String>,
    pub transitions: KanbanWorkflowTransitions,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanWorkflowPhaseSettings {
    pub category: KanbanWorkflowPhaseCategory,
    pub label: String,
    pub agent_profile: Option<String>,
    pub instructions: String,
    pub prompt: KanbanWorkflowPromptSettings,
    pub env: BTreeMap<String, String>,
    pub stage_order: Vec<String>,
    pub stages: BTreeMap<String, KanbanWorkflowStageSettings>,
}

impl Default for KanbanWorkflowPhaseSettings {
    fn default() -> Self {
        Self {
            category: KanbanWorkflowPhaseCategory::Planning,
            label: String::new(),
            agent_profile: None,
            instructions: String::new(),
            prompt: KanbanWorkflowPromptSettings::default(),
            env: BTreeMap::new(),
            stage_order: Vec::new(),
            stages: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanWorkflowSettings {
    pub id: String,
    pub label: String,
    pub env: BTreeMap<String, String>,
    pub phase_order: Vec<String>,
    pub phases: BTreeMap<String, KanbanWorkflowPhaseSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundledKanbanWorkflowSettings {
    id: String,
    label: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    phase_order: Vec<String>,
    phases: BTreeMap<String, KanbanWorkflowPhaseSettings>,
}

impl From<BundledKanbanWorkflowSettings> for KanbanWorkflowSettings {
    fn from(value: BundledKanbanWorkflowSettings) -> Self {
        Self {
            id: value.id,
            label: value.label,
            env: value.env,
            phase_order: value.phase_order,
            phases: value.phases,
        }
    }
}

impl Default for KanbanWorkflowSettings {
    fn default() -> Self {
        default_kanban_workflow_from_json()
    }
}

fn default_kanban_workflow() -> KanbanWorkflowSettings {
    default_kanban_workflow_from_json()
}

fn default_kanban_workflow_from_json() -> KanbanWorkflowSettings {
    let workflow: KanbanWorkflowSettings =
        serde_json::from_str::<BundledKanbanWorkflowSettings>(DEFAULT_KANBAN_WORKFLOW_JSON)
            .expect("bundled Kanban workflow JSON must deserialize")
            .into();
    validate_default_kanban_workflow(&workflow)
        .expect("bundled Kanban workflow JSON must satisfy runtime assumptions");
    workflow
}

fn default_kanban_workflow_phases() -> BTreeMap<String, KanbanWorkflowPhaseSettings> {
    default_kanban_workflow_from_json().phases
}

fn validate_default_kanban_workflow(workflow: &KanbanWorkflowSettings) -> Result<(), String> {
    if workflow.id.trim() != KANBAN_WORKFLOW_DEFAULT_ID {
        return Err(format!(
            "workflow id must be {KANBAN_WORKFLOW_DEFAULT_ID:?}, got {:?}",
            workflow.id
        ));
    }
    validate_kanban_workflow_runtime_shape(workflow)
}

fn validate_kanban_workflow_runtime_shape(workflow: &KanbanWorkflowSettings) -> Result<(), String> {
    if workflow.id.trim().is_empty() {
        return Err("workflow id must not be empty".to_string());
    }
    if workflow.label.trim().is_empty() {
        return Err("workflow label must not be empty".to_string());
    }

    let required_phases = [
        (KANBAN_PHASE_TODO, KanbanWorkflowPhaseCategory::Todo),
        (KANBAN_PHASE_PLANNING, KanbanWorkflowPhaseCategory::Planning),
        (KANBAN_PHASE_DOING, KanbanWorkflowPhaseCategory::Doing),
        (KANBAN_PHASE_REVIEW, KanbanWorkflowPhaseCategory::Review),
        (KANBAN_PHASE_DONE, KanbanWorkflowPhaseCategory::Done),
    ];
    if workflow.phases.len() != required_phases.len() {
        return Err("workflow must define exactly todo, planning, doing, review, and done phases"
            .to_string());
    }
    for phase_id in workflow.phases.keys() {
        if !required_phases.iter().any(|(expected_id, _)| phase_id == expected_id) {
            return Err(format!("unknown workflow phase {phase_id:?}"));
        }
    }
    for (phase_id, category) in required_phases {
        let Some(phase) = workflow.phases.get(phase_id) else {
            return Err(format!("missing workflow phase {phase_id:?}"));
        };
        if phase.category != category {
            return Err(format!("workflow phase {phase_id:?} has the wrong category"));
        }
        if phase.label.trim().is_empty() {
            return Err(format!("workflow phase {phase_id:?} label must not be empty"));
        }
        if phase.stage_order.is_empty() {
            return Err(format!("workflow phase {phase_id:?} must define stageOrder"));
        }
        for stage_id in &phase.stage_order {
            let Some(stage) = phase.stages.get(stage_id) else {
                return Err(format!(
                    "workflow phase {phase_id:?} stageOrder references missing stage {stage_id:?}"
                ));
            };
            if stage.label.trim().is_empty() {
                return Err(format!(
                    "workflow stage {stage_id:?} in phase {phase_id:?} label must not be empty"
                ));
            }
            if stage.category != phase.category {
                return Err(format!(
                    "workflow stage {stage_id:?} in phase {phase_id:?} has the wrong category"
                ));
            }
            validate_kanban_stage_execution_shape(stage_id, stage)?;
            validate_kanban_stage_transitions(workflow, stage_id, &stage.transitions)?;
        }
        for stage_id in phase.stages.keys() {
            if !phase.stage_order.iter().any(|ordered| ordered == stage_id) {
                return Err(format!(
                    "workflow phase {phase_id:?} stage {stage_id:?} is missing from stageOrder"
                ));
            }
        }
    }

    Ok(())
}

fn validate_kanban_stage_execution_shape(
    stage_id: &str,
    stage: &KanbanWorkflowStageSettings,
) -> Result<(), String> {
    match stage.kind {
        KanbanWorkflowStageKind::Manual => {
            if stage.runner.is_some() || stage.gate.is_some() {
                return Err(format!(
                    "manual workflow stage {stage_id:?} must not define runner or gate"
                ));
            }
        }
        KanbanWorkflowStageKind::Work => {
            if stage.runner.is_none() {
                return Err(format!("work workflow stage {stage_id:?} must define runner"));
            }
            if stage.gate.is_some() {
                return Err(format!("work workflow stage {stage_id:?} must not define gate"));
            }
        }
        KanbanWorkflowStageKind::Gate => {
            if stage.gate.is_none() {
                return Err(format!("gate workflow stage {stage_id:?} must define gate"));
            }
            if stage.runner.is_some() {
                return Err(format!("gate workflow stage {stage_id:?} must not define runner"));
            }
        }
    }
    if let Some(runner) = &stage.runner {
        validate_kanban_runner(stage_id, runner)?;
    }
    if let Some(gate) = &stage.gate {
        validate_kanban_gate(stage_id, gate)?;
    }
    Ok(())
}

fn validate_kanban_runner(
    stage_id: &str,
    runner: &KanbanWorkflowRunnerSettings,
) -> Result<(), String> {
    match runner {
        KanbanWorkflowRunnerSettings::Agent { agent_profile } => {
            if agent_profile.as_deref().map(str::trim) == Some("") {
                return Err(format!("agent runner stage {stage_id:?} has empty agentProfile"));
            }
        }
        KanbanWorkflowRunnerSettings::Command { command, args, .. } => {
            validate_kanban_command(stage_id, command, args)?;
        }
    }
    Ok(())
}

fn validate_kanban_gate(stage_id: &str, gate: &KanbanWorkflowGateSettings) -> Result<(), String> {
    match gate {
        KanbanWorkflowGateSettings::Manual => {}
        KanbanWorkflowGateSettings::Command { command, args, .. } => {
            validate_kanban_command(stage_id, command, args)?;
        }
    }
    Ok(())
}

fn validate_kanban_command(stage_id: &str, command: &str, args: &[String]) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err(format!("command workflow stage {stage_id:?} command must not be empty"));
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return Err(format!("command workflow stage {stage_id:?} args must not contain NUL"));
    }
    Ok(())
}

fn validate_kanban_stage_transitions(
    workflow: &KanbanWorkflowSettings,
    stage_id: &str,
    transitions: &KanbanWorkflowTransitions,
) -> Result<(), String> {
    for target in [
        transitions.on_complete.as_deref(),
        transitions.on_passed.as_deref(),
        transitions.on_failed.as_deref(),
        transitions.on_changes_requested.as_deref(),
        transitions.on_ci_failed.as_deref(),
        transitions.on_review_comments.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if workflow_stage(workflow, target).is_none() {
            return Err(format!(
                "workflow stage {stage_id:?} transition references missing stage {target:?}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct KanbanSettings {
    #[serde(default)]
    pub startup_sidebar: KanbanStartupSidebar,
    #[serde(default)]
    pub workflow_path: Option<String>,
    #[serde(default)]
    pub workflow_load_error: Option<String>,
    #[serde(default = "default_kanban_workflow")]
    pub workflow: KanbanWorkflowSettings,
}

impl Default for KanbanSettings {
    fn default() -> Self {
        Self {
            startup_sidebar: KanbanStartupSidebar::Restore,
            workflow_path: None,
            workflow_load_error: None,
            workflow: KanbanWorkflowSettings::default(),
        }
    }
}

impl KanbanSettings {
    pub fn phase(&self, id: &str) -> Option<&KanbanWorkflowPhaseSettings> {
        self.workflow.phases.get(id)
    }

    pub fn todo_phase(&self) -> Option<&KanbanWorkflowPhaseSettings> {
        self.phase(KANBAN_PHASE_TODO)
    }

    pub fn planning_phase(&self) -> Option<&KanbanWorkflowPhaseSettings> {
        self.phase(KANBAN_PHASE_PLANNING)
    }

    pub fn doing_phase(&self) -> Option<&KanbanWorkflowPhaseSettings> {
        self.phase(KANBAN_PHASE_DOING)
    }

    pub fn review_phase(&self) -> Option<&KanbanWorkflowPhaseSettings> {
        self.phase(KANBAN_PHASE_REVIEW)
    }

    pub fn done_phase(&self) -> Option<&KanbanWorkflowPhaseSettings> {
        self.phase(KANBAN_PHASE_DONE)
    }

    pub fn stage(
        &self,
        id: &str,
    ) -> Option<(&str, &KanbanWorkflowPhaseSettings, &KanbanWorkflowStageSettings)> {
        workflow_stage(&self.workflow, id)
    }

    pub fn first_stage_in_category(
        &self,
        category: KanbanWorkflowPhaseCategory,
    ) -> Option<(&str, &KanbanWorkflowStageSettings)> {
        for phase_id in &self.workflow.phase_order {
            let Some(phase) = self.workflow.phases.get(phase_id) else {
                continue;
            };
            if phase.category != category {
                continue;
            }
            for stage_id in &phase.stage_order {
                if let Some(stage) = phase.stages.get(stage_id) {
                    return Some((stage_id.as_str(), stage));
                }
            }
        }
        None
    }

    pub fn phase_agent_profile(&self, phase_id: &str) -> Option<&str> {
        self.phase(phase_id)
            .and_then(|phase| phase.agent_profile.as_deref())
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
    }

    pub fn planning_instructions(&self) -> &str {
        self.planning_phase().map(|phase| phase.instructions.as_str()).unwrap_or("")
    }

    pub fn implementation_instructions(&self) -> &str {
        self.doing_phase().map(|phase| phase.instructions.as_str()).unwrap_or("")
    }

    pub fn review_stage(&self, id: &str) -> Option<&KanbanWorkflowStageSettings> {
        self.stage(id).and_then(|(_, _, stage)| {
            (stage.category == KanbanWorkflowPhaseCategory::Review).then_some(stage)
        })
    }

    pub fn review_stage_agent_profile(&self, id: &str) -> Option<&str> {
        self.review_stage(id)
            .and_then(|stage| stage.agent_profile.as_deref())
            .map(str::trim)
            .filter(|profile| !profile.is_empty())
    }

    pub fn review_stage_instructions(&self, id: &str) -> &str {
        self.review_stage(id).map(|stage| stage.instructions.as_str()).unwrap_or("")
    }

    pub fn review_stage_label(&self, id: &str) -> Option<&str> {
        self.review_stage(id)
            .map(|stage| stage.label.as_str())
            .map(str::trim)
            .filter(|label| !label.is_empty())
    }
}

pub fn workflow_stage<'a>(
    workflow: &'a KanbanWorkflowSettings,
    stage_id: &str,
) -> Option<(&'a str, &'a KanbanWorkflowPhaseSettings, &'a KanbanWorkflowStageSettings)> {
    for phase_id in &workflow.phase_order {
        let Some(phase) = workflow.phases.get(phase_id) else {
            continue;
        };
        if let Some(stage) = phase.stages.get(stage_id) {
            return Some((phase_id.as_str(), phase, stage));
        }
    }
    for (phase_id, phase) in &workflow.phases {
        if let Some(stage) = phase.stages.get(stage_id) {
            return Some((phase_id.as_str(), phase, stage));
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouxSettings {
    pub tab_position: TabPosition,
    pub tab_width: u32,
    pub font_size: u32,
    pub font_family: String,
    #[serde(default = "default_ui_font_family")]
    pub ui_font_family: String,
    pub line_height: f64,
    pub scrollback: u32,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub default_project_path: Option<String>,
    #[serde(default)]
    pub repo_roots: Vec<String>,
    #[serde(default = "default_true")]
    pub exclude_worktrees_from_repo_roots: bool,
    pub confirm_on_close: bool,
    pub restore_sessions_on_launch: bool,
    pub worktree_base_path: Option<String>,
    /// Legacy boolean kept for backward compatibility with settings files
    /// written before `worktree_cleanup_on_close` existed. The frontend no
    /// longer reads this directly; `normalized()` migrates it into the new
    /// enum and keeps the two in sync for older readers.
    pub cleanup_worktrees_on_close: bool,
    #[serde(default)]
    pub worktree_cleanup_on_close: WorktreeCleanupMode,
    /// Default starting point when the user clicks "New Worktree" directly.
    /// The flyout / command palette still let them override per-invocation.
    #[serde(default)]
    pub worktree_default_base: WorktreeDefaultBase,
    pub theme: String,
    /// Terminal color palette. `"match-gui"` (default) follows the GUI
    /// theme's bundled terminal palette; any other value names a standalone
    /// palette (one of the GUI-matching IDs or a built-in editor scheme like
    /// `dracula`, `solarized-dark`, etc.). Unknown IDs normalize back to
    /// `"match-gui"` so a future schema addition cannot brick old clients.
    #[serde(default = "default_terminal_theme")]
    pub terminal_theme: String,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_binary_path: Option<String>,
    /// Absolute path to the `gh` (GitHub CLI) binary. When set and non-empty,
    /// Roux uses it directly instead of resolving `gh` from `PATH`. macOS
    /// GUI apps inherit a minimal PATH that often excludes `/opt/homebrew/bin`
    /// and other shell-managed prefixes, so users on fish/zsh with Homebrew
    /// typically need to set this explicitly.
    #[serde(default)]
    pub gh_binary_path: Option<String>,
    /// Absolute path to the `git` binary. When set and non-empty, Roux uses
    /// it for git-backed Library sources instead of resolving `git` from the
    /// login-shell PATH or process PATH.
    #[serde(default)]
    pub git_binary_path: Option<String>,
    /// Absolute path to the `wt` (worktrunk) binary. When set and non-empty,
    /// Roux uses it directly instead of resolving `wt` from `PATH`. Same
    /// motivation as `gh_binary_path` — macOS GUI apps inherit a minimal
    /// PATH that often excludes `/opt/homebrew/bin`. Leave unset to resolve
    /// via the login-shell PATH and fall back to "no worktrunk available"
    /// when nothing is found.
    #[serde(default)]
    pub worktrunk_binary_path: Option<String>,
    /// Absolute path to the shell binary for terminal panes and login-shell
    /// PATH discovery. When set and non-empty, overrides automatic resolution
    /// from the OS login shell, then $SHELL.
    #[serde(default)]
    pub shell_binary_path: Option<String>,
    /// Which backend Roux uses to create worktrees. Default `Auto` prefers
    /// `wt` when available and falls back to git when not.
    #[serde(default)]
    pub worktree_provider: WorktreeProvider,
    pub additional_flags: Vec<String>,
    pub task_panel_split: f64,
    pub task_panel_collapsed: bool,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub enable_logging: bool,
    #[serde(default)]
    pub group_by: GroupBy,
    #[serde(default = "default_true")]
    pub confirm_on_quit: bool,
    /// Master kill-switch for the notification service's OS-notification
    /// fan-out. When false, notifications still land in the in-app pane but
    /// `tauri-plugin-notification` is never invoked. Defaults to true.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Per-pane agent completion notifications. When false, the
    /// generating→idle transition never produces a notification card or OS
    /// fan-out. Error notifications are unaffected. Defaults to true.
    #[serde(default = "default_true")]
    pub agent_completion_notifications_enabled: bool,
    /// When a background agent leaves the "attention" (waiting-for-answer)
    /// state, also clear the pane's `permissionInfo` so the Claude
    /// Allow/Deny affordance disappears alongside the notification.
    /// Defaults to true — rollback insurance only.
    #[serde(default = "default_true")]
    pub auto_clear_attention_state: bool,
    /// Whether Roux checks for updates silently on launch. Manual checks via
    /// Settings / command palette remain available regardless.
    #[serde(default = "default_true")]
    pub update_check_on_launch: bool,
    /// Experimental multi-scoped notes — when true (the default), the
    /// timestamped-append CLI verb prefixes each entry with an inline
    /// `<a id="entry-...">` HTML anchor so the entry is deep-linkable
    /// from any mainstream static-site generator. Disable for cleaner
    /// raw markdown if you only read the vault in Obsidian.
    #[serde(default = "default_true")]
    pub notes_include_web_anchors: bool,
    /// Experimental multi-scoped notes — override for the vault root
    /// directory. `None` means "use the default `~/Documents/Roux`".
    #[serde(default)]
    pub notes_vault_root: Option<String>,
    /// Experimental multi-scoped notes — set to `true` once the legacy
    /// `<project_id>.txt` files have been migrated into the vault. Guards
    /// against re-running the migration on every app launch.
    #[serde(default)]
    pub notes_migrated_v1: bool,
    /// Which update channel this user is subscribed to. `Stable` pulls from the
    /// standard `latest.json` manifest; `PreRelease` resolves the newest
    /// GitHub prerelease at check time and pulls its `latest-prerelease.json`.
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// User-defined spawn profiles. Edited as raw JSON in the settings file
    /// in v1 — the "Save as user profile" UI is a later addition. The settings
    /// loader force-sets `source: "user"` on each entry regardless of what
    /// the file says, so users can't forge a `"builtin"` marker.
    #[serde(default)]
    pub spawn_profiles: Vec<SpawnProfile>,
    /// Environment and preflight rules applied to all newly-spawned terminal
    /// shells before profile-specific rules.
    #[serde(default)]
    pub terminal_defaults: TerminalDefaults,
    /// Default autonomous agent profile used by agent-starting surfaces.
    /// Card-level/profile-specific overrides still win.
    #[serde(default = "default_agent_profile")]
    pub default_agent_profile: String,
    /// Main UI destination to show after app startup settings are loaded.
    #[serde(default)]
    pub startup_target: StartupTarget,
    /// External tool id used when `startup_target` is `ExternalTool`.
    /// Only enabled global tools are accepted.
    #[serde(default)]
    pub startup_external_tool_id: Option<String>,
    /// Absolute paths of workspaces the user has marked trusted for the
    /// future project-profile loader (`.roux/profiles.json`). Reserved in
    /// phase 3; the loader that consumes this list ships later. Storing the
    /// field now means the trust prompt and its toggles will not require a
    /// settings schema bump when they arrive.
    #[serde(default)]
    pub trusted_workspaces: Vec<String>,
    /// Ordered git repositories that contribute reusable Library items
    /// under `.roux/library/prompts/` and `.roux/library/skills/`.
    /// The active repo is layered separately at runtime and wins over these.
    #[serde(default)]
    pub library_pinned_repos: Vec<String>,
    /// Ordered Library sources. Local repo sources read `.roux/library` from
    /// an existing checkout; Git repo sources are managed by Roux and synced
    /// through native Git operations.
    #[serde(default)]
    pub library_sources: Vec<LibrarySource>,
    #[serde(default)]
    pub status_bar_position: StatusBarPosition,
    /// Reveal the pane-number overlay while Option (⌥) is held. The
    /// `Option+digit` / `Option+HJKL` chord shortcuts are unaffected either way.
    #[serde(default)]
    pub show_pane_hints_on_option: bool,
    /// Reveal the session-shortcut overlay while the primary modifier
    /// (⌘ on macOS, Ctrl elsewhere) is held. Chord shortcuts are unaffected.
    #[serde(default = "default_true")]
    pub show_session_hints_on_command: bool,
    /// What happens to a PTY when its pane is closed.
    /// `Kill` (default): the process is killed immediately.
    /// `Detach`: the process keeps running and can be re-attached.
    #[serde(default)]
    pub on_pane_close: OnPaneCloseMode,
    /// Set to `true` once the global Library skills have been rewritten to
    /// the SKILL.md-compatible format (adds a `name:` field, strips legacy
    /// `variables:` blocks). Guards against re-running on every launch.
    #[serde(default)]
    pub library_skill_format_v2_migrated: bool,
    /// Default skill-sync mode applied to Library sources that don't
    /// override it (`LibrarySource::skill_sync`), to the global vault,
    /// and to the active session repo. Defaults to `Off` so existing
    /// users see no behavior change after upgrade.
    #[serde(default)]
    pub library_skill_sync_default: SkillSyncMode,
    /// xterm.js renderer hint. `Auto` (default) tries WebGL with DOM fallback.
    /// Setting changes apply to terminals created afterward; existing panes
    /// keep their renderer until reopened.
    #[serde(default)]
    pub gpu_acceleration: GpuAcceleration,
    /// When true, sessions whose worktree branch resolves to an open GitHub
    /// PR get a session-scoped `GithubPr` watch created automatically. The
    /// status-bar PR link is shown regardless of this setting; only the
    /// background watch creation is gated. Defaults to `false` so users
    /// don't get unexpected new watches on upgrade.
    #[serde(default)]
    pub auto_watch_session_pr: bool,
    /// Master switch for the gh-backed session-PR lookup. When true (the
    /// default), session activation triggers a `gh pr list --head <branch>`
    /// call to populate the status-bar PR chip and feed the auto-watch
    /// flow. When false, no gh call is made, the chip falls back to
    /// worktrunk's `ciUrl` only, and `auto_watch_session_pr` becomes a
    /// no-op. Useful for users who don't use GitHub PRs or want to avoid
    /// the gh subprocess on every session switch.
    #[serde(default = "default_true")]
    pub auto_lookup_session_pr: bool,
    /// User-facing MCP integration switch. The MCP server is still launched
    /// by MCP hosts via `roux mcp`; this controls Roux's setup/status UX
    /// and whether host configuration buttons are presented as enabled.
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Last MCP host that Roux successfully configured, if any. Stored for
    /// Settings status only; host config files remain the source of truth.
    #[serde(default)]
    pub mcp_last_configured_host: Option<String>,
    /// Unix epoch milliseconds for the last successful MCP host config write.
    #[serde(default)]
    pub mcp_last_configured_at_ms: Option<u64>,
    /// User-configured external tools that Roux can launch into a main-view
    /// surface. Terminal tools run in daemon PTYs; web tools run as daemon
    /// processes and render a local URL in the app chrome.
    #[serde(default = "default_external_tools")]
    pub external_tools: Vec<ExternalTool>,
    #[serde(default)]
    pub kanban: KanbanSettings,
    /// Runtime feature flags. See `ExperimentsConfig`.
    #[serde(default)]
    pub experiments: ExperimentsConfig,
}

impl Default for RouxSettings {
    fn default() -> Self {
        Self {
            tab_position: TabPosition::Left,
            tab_width: 260,
            font_size: 14,
            font_family: "JetBrains Mono, IBM Plex Mono, SFMono-Regular, monospace".to_string(),
            ui_font_family: default_ui_font_family(),
            line_height: 1.2,
            scrollback: 5000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            default_project_path: None,
            repo_roots: Vec::new(),
            exclude_worktrees_from_repo_roots: true,
            confirm_on_close: true,
            restore_sessions_on_launch: true,
            worktree_base_path: None,
            cleanup_worktrees_on_close: false,
            worktree_cleanup_on_close: WorktreeCleanupMode::Prompt,
            worktree_default_base: WorktreeDefaultBase::CurrentBranch,
            theme: DEFAULT_THEME.to_string(),
            terminal_theme: DEFAULT_TERMINAL_THEME.to_string(),
            default_model: None,
            claude_binary_path: None,
            gh_binary_path: None,
            git_binary_path: None,
            worktrunk_binary_path: None,
            shell_binary_path: None,
            worktree_provider: WorktreeProvider::default(),
            additional_flags: Vec::new(),
            task_panel_split: 0.5,
            task_panel_collapsed: true,
            sidebar_collapsed: false,
            enable_logging: false,
            group_by: GroupBy::Repo,
            confirm_on_quit: true,
            notifications_enabled: true,
            agent_completion_notifications_enabled: true,
            auto_clear_attention_state: true,
            update_check_on_launch: true,
            notes_include_web_anchors: true,
            notes_vault_root: None,
            notes_migrated_v1: false,
            update_channel: UpdateChannel::default(),
            spawn_profiles: Vec::new(),
            terminal_defaults: TerminalDefaults::default(),
            default_agent_profile: default_agent_profile(),
            startup_target: StartupTarget::Restore,
            startup_external_tool_id: None,
            trusted_workspaces: Vec::new(),
            library_pinned_repos: Vec::new(),
            library_sources: Vec::new(),
            status_bar_position: StatusBarPosition::Bottom,
            show_pane_hints_on_option: false,
            show_session_hints_on_command: true,
            on_pane_close: OnPaneCloseMode::Kill,
            library_skill_format_v2_migrated: false,
            library_skill_sync_default: SkillSyncMode::Off,
            gpu_acceleration: GpuAcceleration::Auto,
            auto_watch_session_pr: false,
            auto_lookup_session_pr: true,
            mcp_enabled: false,
            mcp_last_configured_host: None,
            mcp_last_configured_at_ms: None,
            external_tools: default_external_tools(),
            kanban: KanbanSettings::default(),
            experiments: ExperimentsConfig::default(),
        }
    }
}

impl RouxSettings {
    pub fn normalized(&self) -> Self {
        let mut s = self.clone();
        s.theme = normalize_theme(&s.theme);
        s.terminal_theme = normalize_terminal_theme(&s.terminal_theme);
        // Force-set source on user profiles regardless of what the JSON says,
        // so a malicious or copy-pasted profile cannot masquerade as built-in.
        for profile in &mut s.spawn_profiles {
            profile.source = ProfileSource::User;
        }
        s.terminal_defaults.before_shell_starts = s
            .terminal_defaults
            .before_shell_starts
            .as_ref()
            .map(|cmd| cmd.trim().to_string())
            .filter(|cmd| !cmd.is_empty());
        s.default_agent_profile = s.default_agent_profile.trim().to_string();
        if s.default_agent_profile.is_empty() {
            s.default_agent_profile = default_agent_profile();
        }
        s.kanban.workflow_path = normalize_optional_string(&s.kanban.workflow_path);
        s.kanban.workflow_load_error = None;
        s.kanban.workflow = normalize_kanban_workflow(&s.kanban.workflow);
        s.repo_roots = normalize_repo_roots(&s.repo_roots);
        s.library_pinned_repos = normalize_repo_roots(&s.library_pinned_repos);
        if s.library_sources.is_empty() && !s.library_pinned_repos.is_empty() {
            s.library_sources = s
                .library_pinned_repos
                .iter()
                .enumerate()
                .map(|(index, path)| LibrarySource {
                    id: stable_source_id("local", path),
                    kind: LibrarySourceKind::LocalRepo,
                    name: source_name_from_path(path),
                    enabled: true,
                    order: index as u32,
                    path: Some(path.clone()),
                    url: None,
                    branch: None,
                    skill_sync: None,
                })
                .collect();
        }
        s.library_sources = normalize_library_sources(&s.library_sources);
        // One-way migration: if an older settings file only has the legacy
        // `cleanupWorktreesOnClose: true` flag, promote the new enum to
        // Always. The `Prompt` default already matches legacy `false`, so
        // no migration is needed in that direction. Keep the legacy bool
        // in sync so older code paths and pre-migration consumers agree.
        if s.cleanup_worktrees_on_close
            && s.worktree_cleanup_on_close == WorktreeCleanupMode::Prompt
        {
            s.worktree_cleanup_on_close = WorktreeCleanupMode::Always;
        }
        s.cleanup_worktrees_on_close = s.worktree_cleanup_on_close == WorktreeCleanupMode::Always;
        s.external_tools = normalize_external_tools(&s.external_tools);
        if s.startup_target == StartupTarget::Restore {
            s.startup_target = match s.kanban.startup_sidebar {
                KanbanStartupSidebar::Restore => StartupTarget::Restore,
                KanbanStartupSidebar::Sessions => StartupTarget::SessionsSidebar,
                KanbanStartupSidebar::Kanban => StartupTarget::KanbanWide,
                KanbanStartupSidebar::None => StartupTarget::None,
            };
        }
        s.startup_external_tool_id =
            s.startup_external_tool_id.as_ref().map(|id| id.trim().to_string()).filter(|id| {
                !id.is_empty()
                    && s.external_tools
                        .iter()
                        .any(|tool| tool.id == *id && tool.enabled && !tool.requires_session)
            });
        if s.startup_target == StartupTarget::ExternalTool && s.startup_external_tool_id.is_none() {
            s.startup_target = StartupTarget::Restore;
        }
        s.kanban.startup_sidebar = match s.startup_target {
            StartupTarget::Restore | StartupTarget::LastSession | StartupTarget::ExternalTool => {
                KanbanStartupSidebar::Restore
            }
            StartupTarget::SessionsSidebar => KanbanStartupSidebar::Sessions,
            StartupTarget::KanbanWide => KanbanStartupSidebar::Kanban,
            StartupTarget::None => KanbanStartupSidebar::None,
        };
        s
    }
}

#[derive(Debug, Error)]
pub enum WorkflowLoadError {
    #[error("invalid workflow JSON: {0}")]
    InvalidJson(String),
    #[error("{0}")]
    InvalidWorkflow(String),
    #[error("failed to read workflow JSON {path}: {message}")]
    Read { path: String, message: String },
    #[error("failed to load workflow JSON {path}: {source}")]
    Load { path: String, source: Box<WorkflowLoadError> },
}

impl WorkflowLoadError {
    pub fn read(path: impl AsRef<Path>, source: impl std::fmt::Display) -> Self {
        Self::Read { path: path.as_ref().display().to_string(), message: source.to_string() }
    }

    pub fn with_path(self, path: impl AsRef<Path>) -> Self {
        match self {
            Self::Read { .. } | Self::Load { .. } => self,
            source => {
                Self::Load { path: path.as_ref().display().to_string(), source: Box::new(source) }
            }
        }
    }
}

pub fn parse_kanban_workflow_json(json: &str) -> Result<KanbanWorkflowSettings, WorkflowLoadError> {
    let workflow: KanbanWorkflowSettings =
        serde_json::from_str::<BundledKanbanWorkflowSettings>(json)
            .map_err(|err| WorkflowLoadError::InvalidJson(err.to_string()))?
            .into();
    validate_kanban_workflow_runtime_shape(&workflow)
        .map_err(WorkflowLoadError::InvalidWorkflow)?;
    Ok(normalize_kanban_workflow(&workflow))
}

pub fn load_settings_json_with_kanban_workflow<F>(
    settings_json: &str,
    mut load_workflow: F,
) -> RouxSettings
where
    F: FnMut(&str) -> Result<KanbanWorkflowSettings, WorkflowLoadError>,
{
    let settings = serde_json::from_str::<RouxSettings>(settings_json).unwrap_or_default();
    load_kanban_workflow_for_settings(settings, |path| load_workflow(path))
}

pub fn load_kanban_workflow_for_settings<F>(
    settings: RouxSettings,
    mut load_workflow: F,
) -> RouxSettings
where
    F: FnMut(&str) -> Result<KanbanWorkflowSettings, WorkflowLoadError>,
{
    let settings = settings.normalized();
    let Some(path) = settings.kanban.workflow_path.as_deref() else {
        return settings;
    };
    let result = load_workflow(path);
    apply_kanban_workflow_load_result(settings, result)
}

pub fn apply_kanban_workflow_load_result(
    settings: RouxSettings,
    result: Result<KanbanWorkflowSettings, WorkflowLoadError>,
) -> RouxSettings {
    let mut settings = settings;
    match result {
        Ok(workflow) => {
            settings.kanban.workflow = normalize_kanban_workflow(&workflow);
            settings.kanban.workflow_load_error = None;
        }
        Err(err) => {
            settings.kanban.workflow_load_error = Some(err.to_string());
        }
    }
    settings
}

fn normalize_kanban_workflow(workflow: &KanbanWorkflowSettings) -> KanbanWorkflowSettings {
    let defaults = KanbanWorkflowSettings::default();
    let mut normalized = workflow.clone();
    normalized.id = normalized.id.trim().to_string();
    if normalized.id.is_empty() {
        normalized.id = defaults.id;
    }
    normalized.label = normalized.label.trim().to_string();
    if normalized.label.is_empty() {
        normalized.label = defaults.label;
    }
    normalized.env = normalize_string_map(&normalized.env);
    normalized.phase_order =
        normalize_order(&normalized.phase_order, &defaults.phase_order, &defaults.phase_order);

    let mut phases = BTreeMap::new();
    for (id, default_phase) in default_kanban_workflow_phases() {
        let mut phase = normalized.phases.remove(&id).unwrap_or_else(|| default_phase.clone());
        phase.category = default_phase.category;
        phase.label = phase.label.trim().to_string();
        if phase.label.is_empty() {
            phase.label = default_phase.label.clone();
        }
        phase.agent_profile = normalize_optional_string(&phase.agent_profile);
        phase.instructions = phase.instructions.trim().to_string();
        phase.prompt = normalize_prompt(&phase.prompt);
        phase.env = normalize_string_map(&phase.env);
        phase.stage_order = normalize_order(
            &phase.stage_order,
            &default_phase.stage_order,
            &default_phase.stage_order,
        );
        phase.stages = normalize_kanban_stages(&phase.stages, &default_phase);
        phases.insert(id, phase);
    }
    normalized.phases = phases;
    normalized
}

fn normalize_kanban_stages(
    stages: &BTreeMap<String, KanbanWorkflowStageSettings>,
    default_phase: &KanbanWorkflowPhaseSettings,
) -> BTreeMap<String, KanbanWorkflowStageSettings> {
    let mut normalized = BTreeMap::new();
    for (id, default_stage) in &default_phase.stages {
        let mut stage = stages.get(id).cloned().unwrap_or_else(|| default_stage.clone());
        stage.label = stage.label.trim().to_string();
        if stage.label.is_empty() {
            stage.label = default_stage.label.clone();
        }
        stage.action_label = normalize_optional_string(&stage.action_label);
        stage.category = default_phase.category;
        stage.agent_profile = normalize_optional_string(&stage.agent_profile);
        stage.instructions = stage.instructions.trim().to_string();
        stage.prompt = normalize_prompt(&stage.prompt);
        stage.runner = normalize_runner(stage.runner);
        stage.gate = normalize_gate(stage.gate);
        stage.env = normalize_string_map(&stage.env);
        stage.transitions = normalize_transitions(&stage.transitions);
        stage.terminal = default_stage.terminal || stage.terminal;
        normalized.insert(id.clone(), stage);
    }
    normalized
}

fn normalize_prompt(prompt: &KanbanWorkflowPromptSettings) -> KanbanWorkflowPromptSettings {
    KanbanWorkflowPromptSettings {
        mode: prompt.mode,
        instructions: prompt.instructions.trim().to_string(),
    }
}

fn normalize_runner(
    runner: Option<KanbanWorkflowRunnerSettings>,
) -> Option<KanbanWorkflowRunnerSettings> {
    runner.map(|runner| match runner {
        KanbanWorkflowRunnerSettings::Agent { agent_profile } => {
            KanbanWorkflowRunnerSettings::Agent {
                agent_profile: normalize_optional_string(&agent_profile),
            }
        }
        KanbanWorkflowRunnerSettings::Command { command, args, cwd, timeout_seconds } => {
            KanbanWorkflowRunnerSettings::Command {
                command: command.trim().to_string(),
                args: normalize_string_vec(args),
                cwd,
                timeout_seconds,
            }
        }
    })
}

fn normalize_gate(gate: Option<KanbanWorkflowGateSettings>) -> Option<KanbanWorkflowGateSettings> {
    gate.map(|gate| match gate {
        KanbanWorkflowGateSettings::Manual => KanbanWorkflowGateSettings::Manual,
        KanbanWorkflowGateSettings::Command { command, args, cwd, timeout_seconds } => {
            KanbanWorkflowGateSettings::Command {
                command: command.trim().to_string(),
                args: normalize_string_vec(args),
                cwd,
                timeout_seconds,
            }
        }
    })
}

fn normalize_transitions(transitions: &KanbanWorkflowTransitions) -> KanbanWorkflowTransitions {
    KanbanWorkflowTransitions {
        on_complete: normalize_optional_string(&transitions.on_complete),
        on_passed: normalize_optional_string(&transitions.on_passed),
        on_failed: normalize_optional_string(&transitions.on_failed),
        on_changes_requested: normalize_optional_string(&transitions.on_changes_requested),
        on_ci_failed: normalize_optional_string(&transitions.on_ci_failed),
        on_review_comments: normalize_optional_string(&transitions.on_review_comments),
    }
}

fn normalize_order(order: &[String], fallback: &[String], allowed: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for id in order {
        let id = id.trim();
        if !id.is_empty()
            && allowed.iter().any(|allowed_id| allowed_id == id)
            && seen.insert(id.to_string())
        {
            normalized.push(id.to_string());
        }
    }
    if normalized.is_empty() {
        return fallback.to_vec();
    }
    for id in fallback {
        if !seen.contains(id) {
            normalized.push(id.clone());
        }
    }
    normalized
}

fn normalize_string_map(map: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    map.iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            if key.is_empty() {
                None
            } else {
                Some((key.to_string(), value.trim().to_string()))
            }
        })
        .collect()
}

fn normalize_string_vec(values: Vec<String>) -> Vec<String> {
    values.into_iter().map(|value| value.trim().to_string()).collect()
}

fn normalize_optional_string(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn normalize_external_tools(tools: &[ExternalTool]) -> Vec<ExternalTool> {
    let mut seen = HashSet::new();
    let mut cleaned = Vec::new();
    for tool in tools {
        let mut next = tool.clone();
        next.id = next.id.trim().to_string();
        next.name = next.name.trim().to_string();
        next.command_template = next.command_template.trim().to_string();
        next.cwd_template = next.cwd_template.trim().to_string();
        next.url_template =
            next.url_template.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        next.preferred_port = next.preferred_port.filter(|port| *port > 0);
        if next.id.is_empty() || next.name.is_empty() || !seen.insert(next.id.clone()) {
            continue;
        }
        if next.surface == ExternalToolSurface::Terminal && next.command_template.is_empty() {
            continue;
        }
        if next.surface == ExternalToolSurface::Web && next.url_template.is_none() {
            continue;
        }
        if next.surface == ExternalToolSurface::Terminal {
            next.url_template = None;
            next.preferred_port = None;
            next.web_embedder = ExternalToolWebEmbedder::Webview;
            next.keep_webview_alive = false;
        }
        if next.surface == ExternalToolSurface::Web
            && next.web_embedder != ExternalToolWebEmbedder::Webview
        {
            next.keep_webview_alive = false;
        }
        cleaned.push(next);
    }
    cleaned
}

fn normalize_library_sources(sources: &[LibrarySource]) -> Vec<LibrarySource> {
    let mut dedup = HashSet::new();
    let mut cleaned = Vec::new();
    for (fallback_order, source) in sources.iter().enumerate() {
        let mut next = source.clone();
        next.name = next.name.trim().to_string();
        next.path = next.path.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        next.url = next.url.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        next.branch = next.branch.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let identity = match next.kind {
            LibrarySourceKind::LocalRepo => next.path.clone(),
            LibrarySourceKind::GitRepo => next.url.clone(),
        };
        let Some(identity) = identity else {
            continue;
        };
        if next.id.trim().is_empty() {
            let prefix = match next.kind {
                LibrarySourceKind::LocalRepo => "local",
                LibrarySourceKind::GitRepo => "git",
            };
            let branch = next.branch.clone().unwrap_or_default();
            next.id = stable_source_id(prefix, &format!("{identity}@{branch}"));
        } else {
            next.id = next.id.trim().to_string();
        }
        if next.name.is_empty() {
            next.name = match next.kind {
                LibrarySourceKind::LocalRepo => source_name_from_path(&identity),
                LibrarySourceKind::GitRepo => source_name_from_url(&identity),
            };
        }
        if next.order == 0 && fallback_order > 0 {
            next.order = fallback_order as u32;
        }
        if dedup.insert(next.id.clone()) {
            cleaned.push(next);
        }
    }
    cleaned.sort_by_key(|source| source.order);
    for (index, source) in cleaned.iter_mut().enumerate() {
        source.order = index as u32;
    }
    cleaned
}

fn stable_source_id(prefix: &str, value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}

fn source_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Library Source")
        .to_string()
}

fn source_name_from_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    trimmed
        .rsplit(['/', ':'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("Git Library")
        .to_string()
}

fn normalize_repo_roots(roots: &[String]) -> Vec<String> {
    let mut dedup = HashSet::new();
    let mut cleaned = Vec::new();
    for root in roots {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        if dedup.insert(trimmed.to_string()) {
            cleaned.push(trimmed.to_string());
        }
    }
    cleaned
}

fn normalize_theme(theme: &str) -> String {
    match theme {
        "dark" | "deep-blue" => DEFAULT_THEME.to_string(),
        "midnight-copper" | "steel-amber" | "slate-emerald" | "graphite-rose" | "nordic-night"
        | "cyber-audit" | "mocha-soft" | "paper-ink" | "github-day" | "warm-burnout-dark"
        | "warm-burnout-light" => theme.to_string(),
        _ => DEFAULT_THEME.to_string(),
    }
}

fn normalize_terminal_theme(theme: &str) -> String {
    // User-supplied themes from `~/.config/roux/themes/*.itermcolors` are
    // identified by `user:<stem>`. The validator can't enumerate them
    // (they live on disk and the file may be temporarily missing on
    // load), so accept any non-empty `user:*` ID and let the resolver
    // fall back to "match-gui" at render time if the file is gone.
    if let Some(rest) = theme.strip_prefix("user:") {
        if !rest.is_empty() {
            return theme.to_string();
        }
    }
    match theme {
        // Sentinel: follow the GUI theme's bundled terminal palette.
        "match-gui"
        // GUI-matching palettes (one per GUI preset).
        | "deep-blue" | "midnight-copper" | "steel-amber" | "slate-emerald"
        | "graphite-rose" | "nordic-night" | "cyber-audit" | "mocha-soft"
        | "paper-ink" | "github-day" | "warm-burnout-dark" | "warm-burnout-light"
        // Editor-style palettes (iterm2colorschemes-inspired).
        | "dracula" | "solarized-dark" | "solarized-light" | "monokai"
        | "nord" | "gruvbox-dark" | "tokyo-night" | "one-dark"
        | "catppuccin-mocha" | "github-dark"
        // Light editor palettes.
        | "github-light" | "one-light" | "catppuccin-latte" | "tokyo-night-day"
        | "gruvbox-light" | "tomorrow" | "ayu-light" => theme.to_string(),
        _ => DEFAULT_TERMINAL_THEME.to_string(),
    }
}

#[cfg(test)]
mod terminal_theme_tests {
    use super::normalize_terminal_theme;

    #[test]
    fn user_prefix_passes_through() {
        assert_eq!(normalize_terminal_theme("user:dracula-mod"), "user:dracula-mod");
        assert_eq!(normalize_terminal_theme("user:my fav"), "user:my fav");
    }

    #[test]
    fn empty_user_prefix_falls_back() {
        assert_eq!(normalize_terminal_theme("user:"), "match-gui");
    }

    #[test]
    fn unknown_falls_back_to_match_gui() {
        assert_eq!(normalize_terminal_theme("not-a-theme"), "match-gui");
    }
}

#[cfg(test)]
mod theme_tests {
    use super::{normalize_theme, DEFAULT_THEME};

    #[test]
    fn known_presets_round_trip() {
        for preset in [
            "midnight-copper",
            "steel-amber",
            "slate-emerald",
            "graphite-rose",
            "nordic-night",
            "cyber-audit",
            "mocha-soft",
            "paper-ink",
            "github-day",
            "warm-burnout-dark",
            "warm-burnout-light",
        ] {
            assert_eq!(normalize_theme(preset), preset, "preset {preset} should round-trip");
        }
    }

    #[test]
    fn legacy_dark_alias_normalizes_to_default() {
        assert_eq!(normalize_theme("dark"), DEFAULT_THEME);
        assert_eq!(normalize_theme("deep-blue"), DEFAULT_THEME);
    }

    #[test]
    fn unknown_falls_back_to_default() {
        assert_eq!(normalize_theme("not-a-theme"), DEFAULT_THEME);
        assert_eq!(normalize_theme(""), DEFAULT_THEME);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        stable_source_id, ExternalToolSurface, ExternalToolWebEmbedder, KanbanSettings,
        KanbanWorkflowPhaseCategory, KanbanWorkflowPhaseSettings, KanbanWorkflowPromptMode,
        KanbanWorkflowSettings, KanbanWorkflowStageSettings, LibrarySource, LibrarySourceKind,
        RouxSettings, SkillSyncMode, UpdateChannel,
    };

    #[test]
    fn hint_overlay_defaults_preserve_command_drop_option() {
        let defaults = RouxSettings::default();
        assert!(!defaults.show_pane_hints_on_option);
        assert!(defaults.show_session_hints_on_command);

        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert!(!parsed.show_pane_hints_on_option);
        assert!(parsed.show_session_hints_on_command);
    }

    #[test]
    fn normalized_repo_roots_trims_empty_and_duplicates() {
        let settings = RouxSettings {
            repo_roots: vec![
                "  /tmp/src  ".to_string(),
                "".to_string(),
                " /tmp/src ".to_string(),
                "/tmp/other".to_string(),
            ],
            ..RouxSettings::default()
        };

        let normalized = settings.normalized();
        assert_eq!(normalized.repo_roots, vec!["/tmp/src", "/tmp/other"]);
    }

    #[test]
    fn settings_without_agent_completion_field_defaults_to_true() {
        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert!(parsed.agent_completion_notifications_enabled);
    }

    #[test]
    fn settings_without_kanban_defaults_to_usable_board_defaults() {
        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.kanban.startup_sidebar, super::KanbanStartupSidebar::Restore);
        assert_eq!(parsed.kanban.workflow.id, "default");
        assert_eq!(parsed.kanban.workflow.label, "Default");
        assert_eq!(
            parsed.kanban.workflow.phases["planning"].category,
            KanbanWorkflowPhaseCategory::Planning
        );
        assert_eq!(parsed.kanban.workflow.phases["planning"].label, "Planning");
        assert_eq!(
            parsed.kanban.workflow.phases["doing"].stages["implementation"].label,
            "Implementation"
        );
        assert_eq!(parsed.kanban.workflow.phases["review"].label, "Review");
        assert_eq!(
            parsed.kanban.workflow.phases["review"].stages["local_review"].label,
            "Local Review"
        );
        assert_eq!(parsed.kanban.workflow.phases["review"].stages["pr_review"].label, "PR Review");
    }

    #[test]
    fn legacy_kanban_agent_profile_is_ignored() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true,
            "kanban": {
                "defaultAgentProfile": "codex",
                "startupSidebar": "restore"
            }
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        let normalized = settings.normalized();

        assert_eq!(normalized.default_agent_profile, "claude");
        assert_eq!(normalized.kanban.workflow.id, "default");
    }

    #[test]
    fn kanban_workflow_normalizes_known_phases_and_drops_unknowns() {
        let settings = RouxSettings {
            kanban: KanbanSettings {
                workflow: KanbanWorkflowSettings {
                    id: " custom ".into(),
                    label: " Team Flow ".into(),
                    env: BTreeMap::from([(" NO_COLOR ".into(), " 1 ".into())]),
                    phase_order: vec![" review ".into(), "planning".into()],
                    phases: BTreeMap::from([
                        (
                            "planning".into(),
                            KanbanWorkflowPhaseSettings {
                                category: KanbanWorkflowPhaseCategory::Review,
                                label: " Plan It ".into(),
                                agent_profile: Some(" codex ".into()),
                                instructions: " Ask first. ".into(),
                                stage_order: vec![" should_drop ".into()],
                                stages: BTreeMap::from([(
                                    "planning".into(),
                                    KanbanWorkflowStageSettings {
                                        label: "Drop".into(),
                                        agent_profile: Some("drop".into()),
                                        instructions: "drop".into(),
                                        ..KanbanWorkflowStageSettings::default()
                                    },
                                )]),
                                ..KanbanWorkflowPhaseSettings::default()
                            },
                        ),
                        (
                            "review".into(),
                            KanbanWorkflowPhaseSettings {
                                category: KanbanWorkflowPhaseCategory::Doing,
                                label: " Review It ".into(),
                                agent_profile: Some(" ".into()),
                                instructions: " ".into(),
                                stage_order: vec!["local_review".into(), "security_review".into()],
                                stages: BTreeMap::from([
                                    (
                                        "local_review".into(),
                                        KanbanWorkflowStageSettings {
                                            label: " Local Gate ".into(),
                                            agent_profile: Some(" claude ".into()),
                                            instructions: " Check locally. ".into(),
                                            ..KanbanWorkflowStageSettings::default()
                                        },
                                    ),
                                    (
                                        "security_review".into(),
                                        KanbanWorkflowStageSettings {
                                            label: "Security".into(),
                                            agent_profile: None,
                                            instructions: "drop".into(),
                                            ..KanbanWorkflowStageSettings::default()
                                        },
                                    ),
                                ]),
                                ..KanbanWorkflowPhaseSettings::default()
                            },
                        ),
                        (
                            "deploy".into(),
                            KanbanWorkflowPhaseSettings {
                                label: "Deploy".into(),
                                ..KanbanWorkflowPhaseSettings::default()
                            },
                        ),
                    ]),
                },
                ..KanbanSettings::default()
            },
            ..RouxSettings::default()
        };

        let normalized = settings.normalized();

        assert_eq!(normalized.kanban.workflow.id, "custom");
        assert_eq!(normalized.kanban.workflow.label, "Team Flow");
        assert_eq!(normalized.kanban.workflow.env["NO_COLOR"], "1");
        assert_eq!(
            normalized.kanban.workflow.phases.keys().cloned().collect::<Vec<_>>(),
            vec!["doing", "done", "planning", "review", "todo"]
        );
        let planning = &normalized.kanban.workflow.phases["planning"];
        assert_eq!(planning.category, KanbanWorkflowPhaseCategory::Planning);
        assert_eq!(planning.label, "Plan It");
        assert_eq!(planning.agent_profile.as_deref(), Some("codex"));
        assert_eq!(planning.instructions, "Ask first.");
        assert_eq!(planning.stage_order, vec!["planning"]);
        assert_eq!(planning.stages["planning"].label, "Drop");

        let doing = &normalized.kanban.workflow.phases["doing"];
        assert_eq!(doing.category, KanbanWorkflowPhaseCategory::Doing);
        assert_eq!(doing.label, "Doing");

        let review = &normalized.kanban.workflow.phases["review"];
        assert_eq!(review.category, KanbanWorkflowPhaseCategory::Review);
        assert_eq!(review.label, "Review It");
        assert!(review.agent_profile.is_none());
        assert_eq!(review.stage_order, vec!["local_review", "pr_review"]);
        assert_eq!(
            review.stages.keys().cloned().collect::<Vec<_>>(),
            vec!["local_review", "pr_review"]
        );
        assert_eq!(review.stages["local_review"].label, "Local Gate");
        assert_eq!(review.stages["local_review"].agent_profile.as_deref(), Some("claude"));
        assert_eq!(review.stages["local_review"].instructions, "Check locally.");
        assert_eq!(review.stages["pr_review"].label, "PR Review");
    }

    #[test]
    fn kanban_workflow_serde_uses_json_native_shape() {
        let settings = KanbanSettings::default();
        let value = serde_json::to_value(&settings).unwrap();

        assert_eq!(value["startupSidebar"], "restore");
        assert_eq!(value["workflow"]["id"], "default");
        assert_eq!(value["workflow"]["label"], "Default");
        assert_eq!(value["workflow"]["phases"]["planning"]["category"], "planning");
        assert_eq!(
            value["workflow"]["phases"]["planning"]["agentProfile"],
            serde_json::Value::Null
        );
        assert_eq!(value["workflow"]["phaseOrder"][0], "todo");
        assert_eq!(value["workflow"]["phases"]["doing"]["stageOrder"][0], "implementation");
        assert_eq!(
            value["workflow"]["phases"]["doing"]["stages"]["fix_ci"]["prompt"]["mode"],
            "replace"
        );
        assert_eq!(
            value["workflow"]["phases"]["review"]["stages"]["local_review"]["label"],
            "Local Review"
        );
        assert!(value.get("planningPromptAppend").is_none());
        assert!(value.get("implementationPromptAppend").is_none());
        assert!(value.get("reviewPromptAppend").is_none());
        assert!(value.get("defaultAgentProfile").is_none());
    }

    #[test]
    fn kanban_settings_round_trips_workflow_path_without_load_error() {
        let settings = KanbanSettings {
            workflow_path: Some(" /tmp/roux-workflow.json ".into()),
            workflow_load_error: Some("stale error".into()),
            ..KanbanSettings::default()
        };

        let normalized = RouxSettings { kanban: settings, ..RouxSettings::default() }.normalized();
        assert_eq!(normalized.kanban.workflow_path.as_deref(), Some("/tmp/roux-workflow.json"));
        assert!(normalized.kanban.workflow_load_error.is_none());

        let value = serde_json::to_value(&normalized.kanban).unwrap();
        assert_eq!(value["workflowPath"], "/tmp/roux-workflow.json");
        assert!(value["workflowLoadError"].is_null());
    }

    #[test]
    fn parse_kanban_workflow_json_accepts_custom_phase_labels_agents_and_instructions() {
        let json = r#"{
            "id": "personal",
            "label": "Personal",
            "env": { "NO_COLOR": "1" },
            "phaseOrder": ["todo", "planning", "doing", "review", "done"],
            "phases": {
                "todo": {
                    "category": "todo",
                    "label": "Queue",
                    "agentProfile": null,
                    "instructions": "",
                    "prompt": { "mode": "append", "instructions": "" },
                    "env": {},
                    "stageOrder": ["todo"],
                    "stages": {
                        "todo": {
                            "label": "Todo",
                            "category": "todo",
                            "kind": "manual",
                            "transitions": { "onComplete": "planning" }
                        }
                    }
                },
                "planning": {
                    "category": "planning",
                    "label": "Shape",
                    "agentProfile": "claude-plan",
                    "instructions": "Clarify scope.",
                    "prompt": { "mode": "append", "instructions": "Plan carefully." },
                    "env": {},
                    "stageOrder": ["planning"],
                    "stages": {
                        "planning": {
                            "label": "Shape",
                            "category": "planning",
                            "kind": "work",
                            "runner": { "type": "agent", "agentProfile": "claude-plan" },
                            "transitions": { "onComplete": "implementation" }
                        }
                    }
                },
                "doing": {
                    "category": "doing",
                    "label": "Build",
                    "agentProfile": "codex",
                    "instructions": "Keep commits small.",
                    "prompt": { "mode": "append", "instructions": "" },
                    "env": {},
                    "stageOrder": ["implementation", "fix_ci"],
                    "stages": {
                        "implementation": {
                            "label": "Build",
                            "category": "doing",
                            "kind": "work",
                            "runner": { "type": "agent", "agentProfile": null },
                            "transitions": { "onComplete": "local_review", "onCiFailed": "fix_ci" }
                        },
                        "fix_ci": {
                            "label": "Fix CI",
                            "category": "doing",
                            "kind": "work",
                            "prompt": { "mode": "replace", "instructions": "Fix CI only." },
                            "runner": {
                                "type": "command",
                                "command": "gh",
                                "args": ["pr", "checks", "--watch"],
                                "cwd": "worktree",
                                "timeoutSeconds": 900
                            },
                            "transitions": { "onComplete": "pr_review" }
                        }
                    }
                },
                "review": {
                    "category": "review",
                    "label": "Review",
                    "agentProfile": "claude-review",
                    "instructions": "",
                    "prompt": { "mode": "append", "instructions": "" },
                    "env": {},
                    "stageOrder": ["local_review", "pr_review"],
                    "stages": {
                        "local_review": {
                            "label": "Local QA",
                            "category": "review",
                            "kind": "gate",
                            "agentProfile": null,
                            "instructions": "Run local checks.",
                            "gate": { "type": "manual" },
                            "transitions": {
                                "onPassed": "pr_review",
                                "onChangesRequested": "implementation"
                            }
                        },
                        "pr_review": {
                            "label": "Team Review",
                            "category": "review",
                            "kind": "gate",
                            "agentProfile": "claude-pr",
                            "instructions": "Check CI and review comments.",
                            "gate": { "type": "manual" },
                            "transitions": {
                                "onPassed": "done",
                                "onChangesRequested": "implementation",
                                "onCiFailed": "fix_ci"
                            }
                        }
                    }
                },
                "done": {
                    "category": "done",
                    "label": "Done",
                    "agentProfile": null,
                    "instructions": "",
                    "prompt": { "mode": "append", "instructions": "" },
                    "env": {},
                    "stageOrder": ["done"],
                    "stages": {
                        "done": {
                            "label": "Done",
                            "category": "done",
                            "kind": "manual",
                            "terminal": true
                        }
                    }
                }
            }
        }"#;

        let workflow = super::parse_kanban_workflow_json(json).unwrap();

        assert_eq!(workflow.id, "personal");
        assert_eq!(workflow.label, "Personal");
        assert_eq!(workflow.env["NO_COLOR"], "1");
        assert_eq!(workflow.phases["planning"].label, "Shape");
        assert_eq!(workflow.phases["planning"].agent_profile.as_deref(), Some("claude-plan"));
        assert_eq!(workflow.phases["doing"].instructions, "Keep commits small.");
        assert_eq!(
            workflow.phases["doing"].stages["fix_ci"].prompt.mode,
            KanbanWorkflowPromptMode::Replace
        );
        assert_eq!(workflow.phases["review"].stages["local_review"].label, "Local QA");
        assert_eq!(
            workflow.phases["review"].stages["pr_review"].agent_profile.as_deref(),
            Some("claude-pr")
        );
    }

    #[test]
    fn example_kanban_workflow_json_matches_loader_shape() {
        let workflow = super::parse_kanban_workflow_json(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/examples/kanban-workflow.json"
        )))
        .unwrap();

        assert_eq!(workflow.id, "personal");
        assert_eq!(workflow.phases["planning"].agent_profile.as_deref(), Some("claude"));
        assert_eq!(workflow.phases["doing"].agent_profile.as_deref(), Some("codex"));
        assert_eq!(workflow.phases["review"].stages["pr_review"].label, "PR Review");
    }

    #[test]
    fn apply_kanban_workflow_load_result_replaces_workflow_or_records_error() {
        let settings = RouxSettings {
            kanban: KanbanSettings {
                workflow_path: Some("/tmp/flow.json".into()),
                workflow: KanbanWorkflowSettings {
                    id: "inline".into(),
                    label: "Inline".into(),
                    ..KanbanWorkflowSettings::default()
                },
                ..KanbanSettings::default()
            },
            ..RouxSettings::default()
        };

        let loaded = KanbanWorkflowSettings {
            id: "loaded".into(),
            label: "Loaded".into(),
            ..KanbanWorkflowSettings::default()
        };
        let applied =
            super::apply_kanban_workflow_load_result(settings.clone(), Ok(loaded.clone()));
        assert_eq!(applied.kanban.workflow.id, "loaded");
        assert_eq!(applied.kanban.workflow.label, "Loaded");
        assert!(applied.kanban.workflow_load_error.is_none());

        let failed = super::apply_kanban_workflow_load_result(
            settings,
            Err(super::WorkflowLoadError::InvalidWorkflow(
                "failed to load /tmp/flow.json: nope".into(),
            )),
        );
        assert_eq!(failed.kanban.workflow.id, "inline");
        assert_eq!(
            failed.kanban.workflow_load_error.as_deref(),
            Some("failed to load /tmp/flow.json: nope")
        );
    }

    #[test]
    fn default_kanban_workflow_matches_bundled_json() {
        let from_json = super::default_kanban_workflow_from_json();

        assert_eq!(KanbanWorkflowSettings::default(), from_json);
    }

    #[test]
    fn bundled_kanban_workflow_has_required_runtime_categories() {
        let workflow = super::default_kanban_workflow_from_json();

        super::validate_default_kanban_workflow(&workflow)
            .expect("bundled Kanban workflow must satisfy runtime assumptions");
    }

    #[test]
    fn legacy_kanban_startup_sidebar_promotes_to_global_startup_target() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true,
            "kanban": {
                "startupSidebar": "kanban"
            }
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        let normalized = settings.normalized();

        assert_eq!(normalized.startup_target, super::StartupTarget::KanbanWide);
        assert_eq!(normalized.kanban.startup_sidebar, super::KanbanStartupSidebar::Kanban);
    }

    #[test]
    fn empty_global_default_agent_normalizes_to_claude() {
        let settings =
            RouxSettings { default_agent_profile: "   ".to_string(), ..RouxSettings::default() };

        let normalized = settings.normalized();

        assert_eq!(normalized.default_agent_profile, "claude");
    }

    #[test]
    fn settings_without_update_channel_defaults_to_stable() {
        let legacy = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.5,
            "taskPanelCollapsed": true
        }"#;
        let parsed: RouxSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.update_channel, UpdateChannel::Stable);
    }

    #[test]
    fn settings_without_git_binary_path_deserializes_as_none() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.git_binary_path, None);
    }

    #[test]
    fn stable_source_id_is_deterministic() {
        assert_eq!(
            stable_source_id("git", "https://example.com/team/lib@main"),
            "git-abf3a29bca551e36"
        );
    }

    #[test]
    fn settings_without_repo_roots_deserializes_with_default() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert!(settings.repo_roots.is_empty());
    }

    #[test]
    fn settings_default_skill_sync_is_off() {
        let settings = RouxSettings::default();
        assert_eq!(settings.library_skill_sync_default, SkillSyncMode::Off);
    }

    #[test]
    fn settings_default_external_tools_seed_lazygit_difit_and_github() {
        let settings = RouxSettings::default();
        let ids: Vec<_> = settings.external_tools.iter().map(|tool| tool.id.as_str()).collect();
        assert_eq!(ids, vec!["lazygit", "difit", "github"]);

        let lazygit = &settings.external_tools[0];
        assert_eq!(lazygit.name, "Lazygit");
        assert!(lazygit.enabled);
        assert_eq!(lazygit.surface, ExternalToolSurface::Terminal);
        assert!(lazygit.requires_session);
        assert_eq!(
            lazygit.command_template,
            "lazygit -p {{ session.worktree_path | shell_quote }}"
        );
        assert_eq!(lazygit.cwd_template, "{{ session.worktree_path }}");

        let difit = &settings.external_tools[1];
        assert_eq!(difit.name, "Difit");
        assert!(difit.enabled);
        assert_eq!(difit.surface, ExternalToolSurface::Web);
        assert!(difit.requires_session);
        assert_eq!(difit.preferred_port, Some(4966));
        assert_eq!(difit.url_template.as_deref(), Some("http://127.0.0.1:{{ port }}"));
        assert_eq!(difit.web_embedder, ExternalToolWebEmbedder::Iframe);
        assert!(!difit.keep_webview_alive);

        let github = &settings.external_tools[2];
        assert_eq!(github.name, "GitHub");
        assert!(github.enabled);
        assert_eq!(github.surface, ExternalToolSurface::Web);
        assert!(!github.requires_session);
        assert_eq!(github.command_template, "");
        assert_eq!(github.url_template.as_deref(), Some("https://github.com"));
        assert_eq!(github.preferred_port, None);
        assert_eq!(github.web_embedder, ExternalToolWebEmbedder::Webview);
        assert!(!github.keep_webview_alive);
    }

    #[test]
    fn settings_without_external_tools_uses_seed_defaults() {
        let json = r#"{
            "tabPosition": "left",
            "tabWidth": 260,
            "fontSize": 14,
            "fontFamily": "monospace",
            "lineHeight": 1.2,
            "scrollback": 5000,
            "cursorStyle": "block",
            "cursorBlink": true,
            "defaultProjectPath": null,
            "confirmOnClose": true,
            "restoreSessionsOnLaunch": true,
            "worktreeBasePath": null,
            "cleanupWorktreesOnClose": false,
            "theme": "deep-blue",
            "defaultModel": null,
            "additionalFlags": [],
            "taskPanelSplit": 0.4,
            "taskPanelCollapsed": false
        }"#;

        let settings: RouxSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.external_tools.len(), 3);
        assert_eq!(settings.external_tools[0].id, "lazygit");
    }

    #[test]
    fn normalized_external_tools_trims_and_drops_invalid_rows() {
        let settings = RouxSettings {
            external_tools: vec![
                super::ExternalTool {
                    id: "  my-tool  ".to_string(),
                    name: "  My Tool ".to_string(),
                    enabled: true,
                    surface: ExternalToolSurface::Web,
                    command_template: "  serve --port {{ port }} ".to_string(),
                    cwd_template: "  {{ session.worktree_path }} ".to_string(),
                    requires_session: true,
                    url_template: Some(" http://127.0.0.1:{{ port }} ".to_string()),
                    preferred_port: Some(0),
                    web_embedder: ExternalToolWebEmbedder::Iframe,
                    keep_webview_alive: true,
                },
                super::ExternalTool {
                    id: "blank-command".to_string(),
                    name: "Remote".to_string(),
                    enabled: true,
                    surface: ExternalToolSurface::Web,
                    command_template: " ".to_string(),
                    cwd_template: "".to_string(),
                    requires_session: false,
                    url_template: Some(" https://github.com ".to_string()),
                    preferred_port: None,
                    web_embedder: ExternalToolWebEmbedder::Webview,
                    keep_webview_alive: true,
                },
                super::ExternalTool {
                    id: "blank-terminal".to_string(),
                    name: "Blank Terminal".to_string(),
                    enabled: true,
                    surface: ExternalToolSurface::Terminal,
                    command_template: " ".to_string(),
                    cwd_template: "".to_string(),
                    requires_session: false,
                    url_template: None,
                    preferred_port: None,
                    web_embedder: ExternalToolWebEmbedder::Webview,
                    keep_webview_alive: true,
                },
                super::ExternalTool {
                    id: "terminal-tool".to_string(),
                    name: "Terminal Tool".to_string(),
                    enabled: true,
                    surface: ExternalToolSurface::Terminal,
                    command_template: " lazygit ".to_string(),
                    cwd_template: " {{ session.worktree_path }} ".to_string(),
                    requires_session: true,
                    url_template: Some(" http://127.0.0.1:{{ port }} ".to_string()),
                    preferred_port: Some(4966),
                    web_embedder: ExternalToolWebEmbedder::Iframe,
                    keep_webview_alive: true,
                },
            ],
            ..RouxSettings::default()
        };

        let normalized = settings.normalized();
        assert_eq!(normalized.external_tools.len(), 3);
        assert_eq!(normalized.external_tools[0].id, "my-tool");
        assert_eq!(normalized.external_tools[0].name, "My Tool");
        assert_eq!(normalized.external_tools[0].command_template, "serve --port {{ port }}");
        assert_eq!(normalized.external_tools[0].cwd_template, "{{ session.worktree_path }}");
        assert_eq!(
            normalized.external_tools[0].url_template.as_deref(),
            Some("http://127.0.0.1:{{ port }}")
        );
        assert_eq!(normalized.external_tools[0].preferred_port, None);
        assert_eq!(normalized.external_tools[0].web_embedder, ExternalToolWebEmbedder::Iframe);
        assert!(!normalized.external_tools[0].keep_webview_alive);
        assert_eq!(normalized.external_tools[1].id, "blank-command");
        assert_eq!(normalized.external_tools[1].command_template, "");
        assert_eq!(
            normalized.external_tools[1].url_template.as_deref(),
            Some("https://github.com")
        );
        assert!(normalized.external_tools[1].keep_webview_alive);
        assert_eq!(normalized.external_tools[2].id, "terminal-tool");
        assert_eq!(normalized.external_tools[2].command_template, "lazygit");
        assert_eq!(normalized.external_tools[2].cwd_template, "{{ session.worktree_path }}");
        assert_eq!(normalized.external_tools[2].url_template, None);
        assert_eq!(normalized.external_tools[2].preferred_port, None);
        assert_eq!(normalized.external_tools[2].web_embedder, ExternalToolWebEmbedder::Webview);
        assert!(!normalized.external_tools[2].keep_webview_alive);
    }

    #[test]
    fn library_source_skill_sync_defaults_to_none_when_missing() {
        let json = r#"{
            "id": "src-1",
            "kind": "localRepo",
            "name": "Repo",
            "enabled": true,
            "order": 0,
            "path": "/repo"
        }"#;
        let source: LibrarySource = serde_json::from_str(json).unwrap();
        assert_eq!(source.skill_sync, None);
    }

    #[test]
    fn library_source_skill_sync_round_trips() {
        let source = LibrarySource {
            id: "src-1".into(),
            kind: LibrarySourceKind::LocalRepo,
            name: "Repo".into(),
            enabled: true,
            order: 0,
            path: Some("/repo".into()),
            url: None,
            branch: None,
            skill_sync: Some(SkillSyncMode::Symlink),
        };
        let json = serde_json::to_string(&source).unwrap();
        let parsed: LibrarySource = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skill_sync, Some(SkillSyncMode::Symlink));
    }
}
